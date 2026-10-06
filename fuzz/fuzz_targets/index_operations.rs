#![no_main]
#![forbid(unsafe_code)]
use emilybase_index::{BPlusTree, Error, IndexSnapshot, Key, MAX_INDEX_ENTRIES, RecordPointer};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;

fuzz_target!(|bytes: &[u8]| {
    if bytes.is_empty() || bytes.len() > 2048 {
        return;
    }
    let mut model: BTreeMap<_, _> = (0..usize::from(bytes[0]) * 2)
        .map(|i| {
            (
                Key::Integer(i as i64),
                RecordPointer {
                    page_id: i as u64 + 1,
                    slot_id: i as u16,
                },
            )
        })
        .collect();
    let initial = model
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect::<Vec<_>>();
    let mut tree = BPlusTree::from_sorted(&initial).unwrap();
    let mut retained = Vec::new();
    for (step, op) in bytes[1..].as_chunks::<6>().0.iter().enumerate() {
        let number = i16::from_le_bytes([op[1], op[2]]) as i64;
        let key = if op[0] & 4 == 0 {
            Key::Integer(number)
        } else {
            Key::Text(format!("clé-{number}"))
        };
        let value = RecordPointer {
            page_id: u64::from(u16::from_le_bytes([op[3], op[4]])) + 1,
            slot_id: u16::from(op[5]),
        };
        let old = model.get(&key).copied();
        match op[0] & 3 {
            0 => {
                if old.is_some() {
                    assert_eq!(tree.insert(key.clone(), value), Err(Error::Duplicate));
                } else {
                    tree.insert(key.clone(), value).unwrap();
                    model.insert(key.clone(), value);
                }
            }
            1 => {
                assert_eq!(tree.replace(&key, value), old.ok_or(Error::NoKey));
                if old.is_some() {
                    model.insert(key.clone(), value);
                }
            }
            2 => {
                assert_eq!(tree.remove(&key), old.ok_or(Error::NoKey));
                model.remove(&key);
            }
            _ => {
                assert_eq!(tree.get(&key).unwrap(), old);
            }
        }
        assert_eq!(tree.validate().unwrap(), model.len());
        assert_eq!(
            tree.range(None, None, MAX_INDEX_ENTRIES).unwrap(),
            model
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect::<Vec<_>>()
        );
        if step % 13 == 0 {
            let admitted = tree.to_stable().unwrap();
            let legacy =
                BPlusTree::from_stable_pages(tree.root_id(), &tree.page_images().unwrap()).unwrap();
            assert_eq!(admitted, legacy);
            let snapshot = IndexSnapshot {
                revision: step as u64 + 1,
                tree: admitted,
            };
            let encoded = snapshot.encode().unwrap();
            assert_eq!(IndexSnapshot::decode(&encoded).unwrap(), snapshot);
            if retained.len() == 4 {
                retained.remove(0);
            }
            retained.push((snapshot, encoded));
        }
        if step % 17 == 0 {
            for (snapshot, encoded) in &retained {
                snapshot.validate().unwrap();
                assert_eq!(snapshot.encode().unwrap(), *encoded);
            }
            tree = BPlusTree::from_pages(tree.root_id(), &tree.page_images().unwrap()).unwrap();
        }
    }
});
