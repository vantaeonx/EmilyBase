#![no_main]
#![forbid(unsafe_code)]

use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Event, EventKind, MAX_ROWS, Snapshot};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeSet;

fn text(bytes: &[u8], mode: u8) -> String {
    let mut value = String::from_utf8_lossy(&bytes[..bytes.len().min(24)]).into_owned();
    match mode % 6 {
        0 => value,
        1 => format!("{value}\0"),
        mode => {
            let length = [255, 256, 257, 3072][usize::from(mode - 2)];
            while value.len() > length {
                value.pop();
            }
            value.push_str(&"z".repeat(length - value.len()));
            value
        }
    }
}

fn check(
    snapshot: &Snapshot,
    model: &BTreeSet<String>,
    lower: Option<&str>,
    upper: Option<&str>,
    limit: usize,
) {
    let expected = model
        .iter()
        .filter(|key| {
            lower.is_none_or(|bound| key.as_str() >= bound)
                && upper.is_none_or(|bound| key.as_str() < bound)
        })
        .take(limit)
        .cloned()
        .map(|key| vec![Value::Text(key)])
        .collect::<Vec<_>>();
    assert_eq!(
        snapshot
            .scan_text_range("words", lower, upper, limit)
            .unwrap(),
        expected
    );
}

fuzz_target!(|input: &[u8]| {
    if input.len() > 4096 {
        return;
    }
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "words".into(),
                columns: vec![Column {
                    name: "id".into(),
                    data_type: DataType::Text,
                    nullable: false,
                }],
                primary_key: 0,
            }),
        })
        .unwrap();
    let mut model = ["", "\0", "a", "a\0", "b", "界", "😀"]
        .map(str::to_owned)
        .into_iter()
        .collect::<BTreeSet<_>>();
    for chunk in input.chunks(25).take(12) {
        model.insert(text(chunk, chunk[0]));
    }
    for key in &model {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Text(key.clone())]),
            })
            .unwrap();
    }
    let before = snapshot.page_fingerprint();
    let historical = snapshot.clone();
    let lower = text(input, input.first().copied().unwrap_or(0));
    let upper = text(
        &input[input.len().saturating_sub(24)..],
        input.last().copied().unwrap_or(0),
    );
    let limit = usize::from(input.get(1).copied().unwrap_or(0) % 20);
    for (lower, upper) in [
        (Some(lower.as_str()), Some(upper.as_str())),
        (Some(upper.as_str()), Some(lower.as_str())),
        (Some(lower.as_str()), None),
        (None, Some(upper.as_str())),
        (None, None),
    ] {
        check(&snapshot, &model, lower, upper, limit);
        check(&snapshot, &model, lower, upper, MAX_ROWS);
    }
    let too_long = "z".repeat(emilybase_catalog::MAX_VALUE_BYTES + 1);
    assert!(
        snapshot
            .scan_text_range("words", Some(&too_long), Some(""), 0)
            .is_err()
    );
    assert!(
        snapshot
            .scan_text_range("words", None, None, MAX_ROWS + 1)
            .is_err()
    );
    assert!(snapshot.scan_integer_range("words", None, None, 0).is_err());
    let tree = snapshot.export_primary_tree("words").unwrap();
    snapshot.install_primary_tree("words", tree).unwrap();
    check(&snapshot, &model, Some(&lower), Some(&upper), limit);
    assert_eq!(snapshot.page_fingerprint(), before);
    let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
    check(&replay, &model, Some(&lower), Some(&upper), MAX_ROWS);
    let old_model = model.clone();
    if let Some(key) = model.pop_first() {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Delete(Key::Text(key)),
            })
            .unwrap();
        check(&snapshot, &model, None, None, MAX_ROWS);
    }
    check(&historical, &old_model, None, None, MAX_ROWS);
    assert_eq!(historical.page_fingerprint(), before);
});
