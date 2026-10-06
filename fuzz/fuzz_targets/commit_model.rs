#![no_main]
#![forbid(unsafe_code)]

use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_commit_format::{IndexKeyType, PageAddress, Predecessor, RootBinding};
use emilybase_commit_model::{Error, Model, Staged};
use emilybase_database::{Event, EventKind};
use emilybase_index::IndexSnapshot;
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;

fn selection(base: &Model, staged: &Staged, text: bool) -> (RootBinding, IndexSnapshot) {
    let view = staged.view().unwrap();
    let tree = view.export_primary_tree("items").unwrap();
    let info = view.verify_primary_tree("items", &tree).unwrap();
    let previous = base.selection(1);
    let revision = previous.map_or(1, |selection| selection.binding().revision() + 1);
    let predecessor = previous.map(|selection| {
        Predecessor::new(
            selection.binding().revision(),
            selection.binding().transaction(),
            selection.index().fingerprint().unwrap(),
        )
        .unwrap()
    });
    let binding = RootBinding::new(
        PageAddress::primary(base.database_id(), 1, tree.root_id()).unwrap(),
        if text {
            IndexKeyType::Text
        } else {
            IndexKeyType::Integer
        },
        revision,
        staged.transaction(),
        info.entries as u64,
        info.excluded_long_keys as u64,
        info.pages as u32,
        predecessor,
    )
    .unwrap();
    (binding, IndexSnapshot { revision, tree })
}

fuzz_target!(|bytes: &[u8]| {
    if bytes.is_empty() || bytes.len() > 256 {
        return;
    }
    let text = bytes[0] & 1 != 0;
    let mut live = Model::new([7; 16]).unwrap();
    let mut initial = live.begin().unwrap();
    initial
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "items".into(),
                primary_key: 0,
                columns: vec![
                    Column {
                        name: "id".into(),
                        data_type: if text {
                            DataType::Text
                        } else {
                            DataType::Integer
                        },
                        nullable: false,
                    },
                    Column {
                        name: "value".into(),
                        data_type: DataType::Text,
                        nullable: false,
                    },
                ],
            }),
        })
        .unwrap();
    let (binding, index) = selection(&live, &initial, text);
    initial.index(binding, index).unwrap();
    live.publish(initial.prepare().unwrap()).unwrap();
    let mut expected = BTreeMap::<Key, String>::new();
    for command in bytes[1..].as_chunks::<4>().0.iter().take(32) {
        let operation = command[0] % 7;
        let key = if text {
            Key::Text(if command[1] & 1 == 0 {
                format!("k{}", command[1])
            } else {
                "λ".repeat(usize::from(command[1])) + "\0"
            })
        } else {
            Key::Integer(match command[1] {
                0 => i64::MIN,
                255 => i64::MAX,
                other => i64::from(other as i8),
            })
        };
        let value = format!("synthetic-{}", command[2]);
        let kind = match operation {
            0 | 3 => EventKind::Insert(vec![key.to_value(), Value::Text(value.clone())]),
            2 => EventKind::Delete(key.clone()),
            _ => EventKind::Replace(vec![key.to_value(), Value::Text(value.clone())]),
        };
        let succeeds = if matches!(operation, 0 | 3) {
            !expected.contains_key(&key)
        } else {
            expected.contains_key(&key)
        };
        let before = live.clone();
        let fingerprint = live.fingerprint();
        let mut staged = live.begin().unwrap();
        let applied = staged.apply(Event { table_id: 1, kind });
        assert_eq!(applied.is_ok(), succeeds);
        if !succeeds {
            assert!(matches!(staged.prepare(), Err(Error::Aborted)));
        } else if operation == 3 {
            drop(staged);
        } else {
            let (mut binding, index) = selection(&live, &staged, text);
            if matches!(operation, 4..=6) {
                let base = binding.predecessor().unwrap();
                let predecessor = if operation == 4 {
                    let mut wrong = base.fingerprint();
                    wrong[usize::from(command[3]) % 32] ^= 1;
                    Some(Predecessor::new(base.revision(), base.transaction(), wrong).unwrap())
                } else {
                    Some(base)
                };
                let address = if operation == 5 {
                    PageAddress::primary([9; 16], 1, binding.address().page()).unwrap()
                } else {
                    binding.address()
                };
                binding = RootBinding::new(
                    address,
                    binding.key_type(),
                    binding.revision(),
                    if operation == 6 {
                        binding.transaction() + 1
                    } else {
                        binding.transaction()
                    },
                    binding.covered(),
                    binding.excluded(),
                    binding.pages(),
                    predecessor,
                )
                .unwrap();
                staged.index(binding, index).unwrap();
                assert!(staged.prepare().is_err());
            } else {
                staged.index(binding, index).unwrap();
                let prepared = staged.prepare().unwrap();
                assert_eq!(live.fingerprint(), fingerprint);
                let plan = prepared.image_plan().unwrap();
                let replayed = plan.replay(&live).unwrap();
                assert_eq!(replayed.fingerprint(), plan.next_fingerprint());
                live.publish(prepared).unwrap();
                assert_eq!(replayed.fingerprint(), live.fingerprint());
                assert_eq!(
                    replayed.view().scan("items", 10000).unwrap(),
                    live.view().scan("items", 10000).unwrap()
                );
                if operation == 2 {
                    expected.remove(&key);
                } else {
                    expected.insert(key, value);
                }
            }
        }
        if !succeeds || operation >= 3 {
            assert_eq!(live.fingerprint(), fingerprint);
        }
        assert_eq!(before.fingerprint(), fingerprint);
        assert_eq!(live.view().row_count(), expected.len());
        let mut covered = 0;
        for (key, value) in &expected {
            assert_eq!(
                live.view().get("items", key).unwrap().unwrap()[1],
                Value::Text(value.clone())
            );
            if !matches!(key,Key::Text(text) if text.len()>256) {
                covered += 1;
            }
        }
        let binding = live.selection(1).unwrap().binding();
        assert_eq!(binding.covered(), covered);
        assert_eq!(binding.excluded(), expected.len() as u64 - covered);
    }
});
