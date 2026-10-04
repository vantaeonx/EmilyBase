#![no_main]
#![forbid(unsafe_code)]

use emilybase_catalog::{DataType, Key, Row};
use emilybase_database::{MAX_ROWS, Snapshot};
use emilybase_transactions::recover_snapshot;
use libfuzzer_sys::fuzz_target;
use std::collections::VecDeque;
mod support;

fn bounds(kind: DataType, input: &[u8]) -> (Key, Key) {
    if kind == DataType::Integer {
        let mut bytes = [0; 16];
        let length = input.len().min(16);
        bytes[..length].copy_from_slice(&input[..length]);
        return (
            Key::Integer(i64::from_le_bytes(bytes[..8].try_into().unwrap())),
            Key::Integer(i64::from_le_bytes(bytes[8..].try_into().unwrap())),
        );
    }
    let length = input.len().min(16);
    let mut lower = String::from_utf8_lossy(&input[..length]).into_owned();
    let mut upper = String::from_utf8_lossy(&input[input.len() - length..]).into_owned();
    if input.first().copied().unwrap_or(0) & 1 != 0 {
        lower.push_str(&"x".repeat(3072 - lower.len()));
    }
    if input.last().copied().unwrap_or(0) & 1 != 0 {
        upper.push_str(&"z".repeat(257 - upper.len()));
    }
    (Key::Text(lower), Key::Text(upper))
}

fn verify(snapshot: Snapshot, input: &[u8]) {
    let digest = snapshot.page_fingerprint();
    for schema in snapshot.schemas() {
        let source = snapshot.scan(&schema.name, MAX_ROWS).unwrap();
        let (lower, upper) = bounds(
            schema.columns[usize::from(schema.primary_key)].data_type,
            input,
        );
        for (lower, upper) in [
            (None, None),
            (Some(&lower), None),
            (None, Some(&upper)),
            (Some(&lower), Some(&upper)),
            (Some(&upper), Some(&lower)),
        ] {
            let expected = source
                .iter()
                .filter(|row| {
                    let key = schema.key(row).unwrap();
                    lower.is_none_or(|bound| &key >= bound)
                        && upper.is_none_or(|bound| &key < bound)
                })
                .cloned()
                .collect::<VecDeque<Row>>();
            let mut cursor = snapshot.primary_rows(&schema.name, lower, upper).unwrap();
            let mut remaining = expected.clone();
            for side in input.iter().take(16) {
                let (actual, want) = if side & 1 == 0 {
                    (cursor.next(), remaining.pop_front())
                } else {
                    (cursor.next_back(), remaining.pop_back())
                };
                assert_eq!(actual.map(|row| row.unwrap().clone()), want);
            }
            assert_eq!(
                cursor.map(|row| row.unwrap().clone()).collect::<Vec<_>>(),
                remaining.into_iter().collect::<Vec<_>>()
            );
            let reverse = snapshot
                .primary_rows(&schema.name, lower, upper)
                .unwrap()
                .rev()
                .take(3)
                .map(|row| row.unwrap().clone())
                .collect::<Vec<_>>();
            assert_eq!(
                reverse,
                expected.into_iter().rev().take(3).collect::<Vec<_>>()
            );
        }
        let tree = snapshot.export_primary_tree(&schema.name).unwrap();
        snapshot.verify_primary_tree(&schema.name, &tree).unwrap();
    }
    assert_eq!(snapshot.page_fingerprint(), digest);
    let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
    for schema in snapshot.schemas() {
        assert_eq!(
            snapshot
                .primary_rows(&schema.name, None, None)
                .unwrap()
                .rev()
                .map(|row| row.unwrap().clone())
                .collect::<Vec<_>>(),
            replay
                .primary_rows(&schema.name, None, None)
                .unwrap()
                .rev()
                .map(|row| row.unwrap().clone())
                .collect::<Vec<_>>()
        );
    }
}

fuzz_target!(|input: &[u8]| {
    if input.len() > 20000 {
        return;
    }
    if let Ok(snapshot) = recover_snapshot(input, None) {
        verify(snapshot, input);
    }
    if let Some(repaired) = support::repaired_wal(input)
        && let Ok(snapshot) = recover_snapshot(&repaired, Some([7; 16]))
    {
        verify(snapshot, input);
    }
});
