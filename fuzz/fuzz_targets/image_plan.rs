#![no_main]
#![forbid(unsafe_code)]
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_commit_model::{ImagePlan, Model};
use emilybase_database::{Event, EventKind};
use libfuzzer_sys::fuzz_target;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fuzz_target!(|input: &[u8]| {
    if input.len() > 16384 {
        return;
    }
    // Raw complete frames, truncated prefixes and unknown headers are admitted
    // independently of generation. Accepted bytes must re-encode canonically.
    if let Ok(decoded) = ImagePlan::decode(input) {
        assert_eq!(decoded.encode().unwrap(), input);
    }
    if input.is_empty() {
        return;
    }
    let mut base = Model::new([7; 16]).unwrap();
    let mut staged = base.begin().unwrap();
    let text = input[0] & 1 != 0;
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "items".into(),
                primary_key: 1,
                columns: vec![
                    Column {
                        name: "value".into(),
                        data_type: DataType::Text,
                        nullable: false,
                    },
                    Column {
                        name: "id".into(),
                        data_type: if text {
                            DataType::Text
                        } else {
                            DataType::Integer
                        },
                        nullable: false,
                    },
                ],
            }),
        })
        .unwrap();
    staged.rebuild_index("items").unwrap();
    base.publish(staged.prepare().unwrap()).unwrap();
    let mut expected = BTreeMap::<Key, String>::new();
    for command in input[1..].as_chunks::<4>().0.iter().take(12) {
        let key = if text {
            Key::Text(match command[1] % 4 {
                0 => "λ".repeat(1536),
                1 => "λ".repeat(128),
                _ => format!("k{}\0", command[1]),
            })
        } else {
            Key::Integer(i64::from(command[1] as i8))
        };
        let operation = command[0] % 3;
        let value = format!("synthetic-{}", command[2]);
        let valid = if operation == 0 {
            !expected.contains_key(&key)
        } else {
            expected.contains_key(&key)
        };
        let mut staged = base.begin().unwrap();
        let kind = match operation {
            0 => EventKind::Insert(vec![Value::Text(value.clone()), key.to_value()]),
            1 => EventKind::Replace(vec![Value::Text(value.clone()), key.to_value()]),
            _ => EventKind::Delete(key.clone()),
        };
        assert_eq!(staged.apply(Event { table_id: 1, kind }).is_ok(), valid);
        if !valid {
            continue;
        }
        staged.rebuild_index("items").unwrap();
        let prepared = staged.prepare().unwrap();
        let original = prepared.image_plan().unwrap();
        let encoded = original.encode().unwrap();
        let decoded = ImagePlan::decode(&encoded).unwrap();
        let before = base.fingerprint();
        let replayed = decoded.replay(&base).unwrap();
        assert_eq!(replayed.fingerprint(), original.next_fingerprint());
        let mut candidate = expected.clone();
        if operation == 2 {
            candidate.remove(&key);
        } else {
            candidate.insert(key, value);
        }
        for (key, value) in &candidate {
            assert_eq!(
                &replayed.view().get("items", key).unwrap().unwrap()[0],
                &Value::Text(value.clone())
            );
        }
        assert_eq!(replayed.view().row_count(), candidate.len());
        let offset = (usize::from(command[2]) * 256 + usize::from(command[3])) % encoded.len();
        let mut damaged = encoded.clone();
        damaged[offset] ^= 1;
        assert!(ImagePlan::decode(&damaged).is_err());
        // Public SHA is not authorization. Repairing it may permit structural
        // admission, but only full exact-base replay can accept any new state.
        let end = damaged.len() - 32;
        let digest = Sha256::digest(&damaged[..end]);
        damaged[end..].copy_from_slice(&digest);
        if let Ok(decoded) = ImagePlan::decode(&damaged)
            && let Ok(replayed) = decoded.replay(&base)
        {
            assert_eq!(replayed.fingerprint(), original.next_fingerprint());
        }
        assert_eq!(base.fingerprint(), before);
        if command[3] & 1 != 0 {
            base.publish(prepared).unwrap();
            expected = candidate;
        }
    }
});
