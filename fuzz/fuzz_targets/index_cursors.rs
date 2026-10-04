#![no_main]
#![forbid(unsafe_code)]

use emilybase_index::{BPlusTree, IndexSnapshot, Key, RecordPointer};
use libfuzzer_sys::fuzz_target;
use std::collections::{BTreeMap, VecDeque};

fn key(bytes: &[u8]) -> Key {
    if bytes.first().copied().unwrap_or(0) & 1 == 0 {
        return Key::Integer(i64::from(i16::from_le_bytes([
            bytes.get(1).copied().unwrap_or(0),
            bytes.get(2).copied().unwrap_or(0),
        ])));
    }
    let mut text = String::from_utf8_lossy(&bytes[..bytes.len().min(16)]).into_owned();
    if bytes.first().copied().unwrap_or(0) & 2 != 0 {
        text.push('\0');
    }
    if bytes.first().copied().unwrap_or(0) & 4 != 0 {
        text.push_str(&"z".repeat(256 - text.len()));
    }
    Key::Text(text)
}

fn owned(entry: (&Key, RecordPointer)) -> (Key, RecordPointer) {
    (entry.0.clone(), entry.1)
}

fn verify(tree: &BPlusTree, model: &BTreeMap<Key, RecordPointer>, input: &[u8]) {
    let start = key(&input[..input.len().min(16)]);
    let end = key(&input[input.len().saturating_sub(16)..]);
    for (lower, upper) in [
        (None, None),
        (Some(&start), None),
        (None, Some(&end)),
        (Some(&start), Some(&end)),
        (Some(&end), Some(&start)),
    ] {
        let expected = model
            .iter()
            .filter(|(k, _)| lower.is_none_or(|v| *k >= v) && upper.is_none_or(|v| *k < v))
            .map(|(k, v)| (k.clone(), *v))
            .collect::<VecDeque<_>>();
        let mut cursor = tree.cursor(lower, upper).unwrap();
        let mut remaining = expected.clone();
        for choice in input.iter().take(64) {
            let (actual, want) = if choice & 1 == 0 {
                (cursor.next(), remaining.pop_front())
            } else {
                (cursor.next_back(), remaining.pop_back())
            };
            assert_eq!(actual.map(|v| owned(v.unwrap())), want);
        }
        assert_eq!(
            cursor.map(|v| owned(v.unwrap())).collect::<Vec<_>>(),
            remaining.into_iter().collect::<Vec<_>>()
        );
        assert_eq!(
            tree.cursor(lower, upper)
                .unwrap()
                .rev()
                .map(|v| owned(v.unwrap()))
                .collect::<Vec<_>>(),
            expected.into_iter().rev().collect::<Vec<_>>()
        );
        let mut empty = tree.cursor(Some(&start), Some(&start)).unwrap();
        assert!(empty.next().is_none());
        assert!(empty.next_back().is_none());
    }
}

fuzz_target!(|input: &[u8]| {
    if input.len() > 2048 {
        return;
    }
    let count = usize::from(input.first().copied().unwrap_or(0)) * 2;
    let mut model = (0..count)
        .map(|i| {
            (
                Key::Integer(i as i64 - 250),
                RecordPointer {
                    page_id: i as u64 + 1,
                    slot_id: 0,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for (i, chunk) in input.chunks(25).enumerate() {
        model.insert(
            key(chunk),
            RecordPointer {
                page_id: i as u64 + 1000,
                slot_id: i as u16,
            },
        );
    }
    let entries = model
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect::<Vec<_>>();
    let mut tree = if input.len().is_multiple_of(2) {
        BPlusTree::from_sorted_stable(&entries)
    } else {
        BPlusTree::from_sorted(&entries)
    }
    .unwrap();
    let before = tree.page_images().unwrap();
    verify(&tree, &model, input);
    assert_eq!(tree.page_images().unwrap(), before);
    let keys = model
        .keys()
        .step_by(3)
        .take(24)
        .cloned()
        .collect::<Vec<_>>();
    for key in keys {
        assert_eq!(tree.remove(&key).unwrap(), model.remove(&key).unwrap());
    }
    let restored = if tree.has_stable_ids() {
        let snapshot = IndexSnapshot { revision: 1, tree };
        IndexSnapshot::decode(&snapshot.encode().unwrap())
            .unwrap()
            .tree
    } else {
        BPlusTree::from_pages(tree.root_id(), &tree.page_images().unwrap()).unwrap()
    };
    verify(&restored, &model, input);
});
