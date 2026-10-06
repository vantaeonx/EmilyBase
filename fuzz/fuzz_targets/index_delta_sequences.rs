#![no_main]
#![forbid(unsafe_code)]
use emilybase_index::{BPlusTree, IndexSnapshot, Key, RecordPointer};
use libfuzzer_sys::fuzz_target;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn verify(snapshot: &IndexSnapshot, expected: &BTreeMap<Key, RecordPointer>) {
    snapshot.validate().unwrap();
    assert_eq!(snapshot.tree.len(), expected.len());
    let entries: Vec<_> = expected
        .iter()
        .map(|(key, value)| (key.clone(), *value))
        .collect();
    assert_eq!(snapshot.tree.range(None, None, 10000).unwrap(), entries);
    for (key, value) in expected {
        assert_eq!(snapshot.tree.get(key).unwrap(), Some(*value));
    }
    let encoded = snapshot.encode().unwrap();
    let hash: [u8; 32] = Sha256::digest(&encoded).into();
    assert_eq!(snapshot.fingerprint().unwrap(), hash);
    assert_eq!(IndexSnapshot::decode(&encoded).unwrap(), *snapshot);
}

fuzz_target!(|input: &[u8]| {
    if input.is_empty() || input.len() > 512 {
        return;
    }
    let mut live = IndexSnapshot {
        revision: 1,
        tree: BPlusTree::new_stable(),
    };
    let mut expected = BTreeMap::new();
    let mut retained = Vec::new();
    for command in input[1..].as_chunks::<4>().0.iter().take(48) {
        let operation = command[0] % 4;
        let number = command[1];
        let key = if command[0] & 16 != 0 {
            Key::Text(match number % 4 {
                0 => String::new(),
                1 => format!("{number:03}{}\0", "λ".repeat(126)),
                2 => format!("k{number}\0"),
                _ => "λ".repeat(128),
            })
        } else {
            Key::Integer(match number {
                0 => i64::MIN,
                255 => i64::MAX,
                _ => i64::from(number as i8),
            })
        };
        let pointer = RecordPointer {
            page_id: if command[2] == 0 {
                u64::MAX
            } else {
                u64::from(command[2])
            },
            slot_id: u16::from_le_bytes([command[2], command[3]]),
        };
        if command[2] & 7 == 0 {
            if retained.len() == 4 {
                retained.remove(usize::from(command[3]) % 4);
            }
            retained.push((live.clone(), expected.clone(), live.encode().unwrap()));
        }
        for (snapshot, rows, encoded) in &retained {
            verify(snapshot, rows);
            assert_eq!(snapshot.encode().unwrap(), *encoded);
        }
        let before = live.encode().unwrap();
        let old = live.clone();
        let mut target = live.tree.clone();
        let valid = match operation {
            0 => !expected.contains_key(&key),
            1 | 2 => expected.contains_key(&key),
            _ => true,
        };
        let result = match operation {
            0 => target.insert(key.clone(), pointer),
            1 => target.replace(&key, pointer).map(|_| ()),
            2 => target.remove(&key).map(|_| ()),
            _ => Ok(()),
        };
        assert_eq!(result.is_ok(), valid);
        if !valid {
            assert_eq!(live.encode().unwrap(), before);
            continue;
        }
        let delta = live.delta_to(&target).unwrap();
        let selected = delta.apply(&live).unwrap();
        let mut candidate = expected.clone();
        match operation {
            0 | 1 => {
                candidate.insert(key, pointer);
            }
            2 => {
                candidate.remove(&key);
            }
            _ => (),
        }
        verify(&selected, &candidate);
        assert_eq!(selected.tree, target);
        let mut wrong = delta.clone();
        wrong.base_fingerprint[usize::from(command[3]) % 32] ^= 1;
        assert!(wrong.apply(&live).is_err());
        let mut wrong = delta.clone();
        wrong.revision += 1;
        assert!(wrong.apply(&live).is_err());
        let mut wrong = delta.clone();
        wrong.entries = usize::MAX;
        assert!(wrong.apply(&live).is_err());
        assert_eq!(live.encode().unwrap(), before);
        assert_eq!(old.encode().unwrap(), before);
        if command[3] & 1 != 0 {
            live = selected;
            expected = candidate;
        }
        verify(&live, &expected);
    }
    drop(live);
    drop(expected);
    // Last historical owners remain usable after all mutable working state
    // disappears; sanitizer leak checks also observe release on target exit.
    for (snapshot, rows, encoded) in retained {
        verify(&snapshot, &rows);
        assert_eq!(snapshot.encode().unwrap(), encoded);
    }
});
