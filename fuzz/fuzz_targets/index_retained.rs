#![no_main]
#![forbid(unsafe_code)]
use emilybase_index::{BPlusTree, Error, IndexPage, IndexSnapshot, Key, RecordPointer};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;

fn text(number: u8, capacity: usize) -> Key {
    let mut value = String::with_capacity(capacity);
    match number % 4 {
        0 => value.push_str(&format!("{number:03}я\0")),
        1 => value.push_str(&format!("{number:03}{}\0", "λ".repeat(126))),
        2 => value.push_str(""),
        _ => value.push('🌌'),
    }
    Key::Text(value)
}
fn key_shape(key: &Key) {
    if let Key::Text(value) = key {
        assert_eq!(value.capacity(), value.len());
    }
}
fn verify(tree: &BPlusTree, expected: &BTreeMap<Key, RecordPointer>) {
    assert_eq!(tree.validate().unwrap(), expected.len());
    let mut cursor = tree.cursor(None, None).unwrap();
    for (key, pointer) in expected {
        let (actual, value) = cursor.next().unwrap().unwrap();
        key_shape(actual);
        assert_eq!(actual, key);
        assert_eq!(value, *pointer);
    }
    assert!(cursor.next().is_none());
}

fuzz_target!(|input: &[u8]| {
    if input.len() < 4 || input.len() > 512 {
        return;
    }
    let capacity = 1024 * (usize::from(input[0] % 8) + 1);
    let pointer = RecordPointer {
        page_id: u64::from(input[1]) + 1,
        slot_id: u16::from(input[2]),
    };
    let mut entries = Vec::with_capacity(capacity);
    entries.push((text(input[1], capacity), pointer));
    let page = IndexPage::leaf(1, entries, None).unwrap();
    key_shape(&page.keys()[0]);
    assert_eq!(IndexPage::decode(&page.encode().unwrap(), 1).unwrap(), page);
    let mut keys = Vec::with_capacity(capacity);
    keys.push(text(input[1], capacity));
    let mut children = Vec::with_capacity(capacity);
    children.extend([1, 2]);
    let branch = IndexPage::branch(3, keys, children).unwrap();
    key_shape(&branch.keys()[0]);
    assert_eq!(
        IndexPage::decode(&branch.encode().unwrap(), 3).unwrap(),
        branch
    );

    let mut tree = if input[0] & 1 == 0 {
        BPlusTree::new_stable()
    } else {
        BPlusTree::new()
    };
    let mut expected = BTreeMap::new();
    let mut retained = Vec::new();
    for command in input[3..].as_chunks::<4>().0.iter().take(80) {
        let key = if command[0] & 8 == 0 {
            text(command[1], capacity)
        } else {
            Key::Integer(i64::from(command[1] as i8))
        };
        let canonical = key.clone();
        let pointer = RecordPointer {
            page_id: u64::from(command[2]) + 1,
            slot_id: u16::from(command[3]),
        };
        let old = tree.clone();
        let old_images = old.page_images().unwrap();
        let previous = expected.get(&key).copied();
        match command[0] % 4 {
            0 => {
                assert_eq!(
                    tree.insert(key, pointer),
                    previous.map_or(Ok(()), |_| Err(Error::Duplicate))
                );
                if previous.is_none() {
                    expected.insert(canonical, pointer);
                }
            }
            1 => {
                assert_eq!(tree.replace(&key, pointer), previous.ok_or(Error::NoKey));
                if previous.is_some() {
                    expected.insert(canonical, pointer);
                }
            }
            2 => {
                assert_eq!(tree.remove(&key), previous.ok_or(Error::NoKey));
                expected.remove(&key);
            }
            _ => {
                assert_eq!(tree.get(&key).unwrap(), previous);
            }
        }
        // Inspect actual borrowed keys before any round-trip can trim buffers.
        verify(&tree, &expected);
        assert_eq!(old.page_images().unwrap(), old_images);
        if command[2] % 7 == 0 {
            if retained.len() == 4 {
                retained.remove(0);
            }
            retained.push((tree.clone(), expected.clone(), tree.page_images().unwrap()));
            let snapshot = IndexSnapshot {
                revision: 1,
                tree: tree.to_stable().unwrap(),
            };
            let next = IndexSnapshot::decode(&snapshot.encode().unwrap()).unwrap();
            verify(&next.tree, &expected);
            assert_eq!(next, snapshot);
            let delta = snapshot.delta_to(&tree.to_stable().unwrap()).unwrap();
            assert_eq!(delta.apply(&snapshot).unwrap().tree, snapshot.tree);
        }
        for (snapshot, model, images) in &retained {
            verify(snapshot, model);
            assert_eq!(snapshot.page_images().unwrap(), *images);
        }
    }
    drop(tree);
    for (snapshot, model, images) in retained {
        verify(&snapshot, &model);
        assert_eq!(snapshot.page_images().unwrap(), images);
    }
});
