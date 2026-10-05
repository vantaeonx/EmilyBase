use crate::{Event, EventKind, Snapshot};
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn populated() -> Snapshot {
    let mut value = Snapshot::empty().unwrap();
    value
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "items".into(),
                primary_key: 0,
                columns: vec![
                    Column {
                        name: "id".into(),
                        data_type: DataType::Text,
                        nullable: false,
                    },
                    Column {
                        name: "body".into(),
                        data_type: DataType::Text,
                        nullable: false,
                    },
                ],
            }),
        })
        .unwrap();
    for number in 0..2 {
        value
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![key(number).to_value(), Value::Text("v".repeat(768))]),
            })
            .unwrap();
    }
    value
}

fn key(number: u8) -> Key {
    Key::Text(format!("{number:02}{}", "я".repeat(1535)))
}

#[test]
fn first_write_keeps_other_rows_in_the_same_table_shared() {
    let original = populated();
    let mut branch = original.clone();
    branch
        .apply(Event {
            table_id: 1,
            kind: EventKind::Replace(vec![key(0).to_value(), Value::Text("new".into())]),
        })
        .unwrap();
    assert!(std::ptr::eq(
        original.get("items", &key(1)).unwrap().unwrap(),
        branch.get("items", &key(1)).unwrap().unwrap()
    ));
    assert_eq!(
        original.get("items", &key(0)).unwrap().unwrap()[1],
        Value::Text("v".repeat(768))
    );
}

#[test]
fn first_write_preserves_long_key_allocations_and_owned_scan_results_do_not_alias() {
    let original = populated();
    let mut branch = original.clone();
    let long = key(1);
    let before = original
        .state
        .table("items")
        .unwrap()
        .rows
        .get_key_value(&long)
        .unwrap()
        .0;
    branch
        .apply(Event {
            table_id: 1,
            kind: EventKind::Replace(vec![key(0).to_value(), Value::Text("new".into())]),
        })
        .unwrap();
    let after = branch
        .state
        .table("items")
        .unwrap()
        .rows
        .get_key_value(&long)
        .unwrap()
        .0;
    assert!(std::sync::Arc::ptr_eq(before, after));
    let mut rows = branch.scan("items", 2).unwrap();
    rows[1][0] = Value::Text("mutated returned key".into());
    rows[1][1] = Value::Text("mutated returned value".into());
    assert_eq!(
        branch.get("items", &long).unwrap().unwrap()[1],
        Value::Text("v".repeat(768))
    );
    assert_eq!(
        original.get("items", &long).unwrap().unwrap()[1],
        Value::Text("v".repeat(768))
    );
}

#[test]
fn delete_and_reinsert_keep_unrelated_row_bodies_and_retire_only_old_locations() {
    let original = populated();
    let mut branch = original.clone();
    let unchanged = original.get("items", &key(1)).unwrap().unwrap();
    let old = original.row_location("items", &key(0)).unwrap().unwrap();
    branch
        .apply(Event {
            table_id: 1,
            kind: EventKind::Delete(key(0)),
        })
        .unwrap();
    assert!(std::ptr::eq(
        unchanged,
        branch.get("items", &key(1)).unwrap().unwrap()
    ));
    assert!(branch.get("items", &key(0)).unwrap().is_none());
    branch
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(vec![key(0).to_value(), Value::Text("reinserted".into())]),
        })
        .unwrap();
    assert!(std::ptr::eq(
        unchanged,
        branch.get("items", &key(1)).unwrap().unwrap()
    ));
    assert_ne!(branch.row_location("items", &key(0)).unwrap(), Some(old));
    assert_eq!(
        original
            .resolve_row_location("items", &key(0), old)
            .unwrap()[1],
        Value::Text("v".repeat(768))
    );
    let replay = Snapshot::from_pages(branch.pages().cloned().collect()).unwrap();
    assert_eq!(
        replay.scan("items", 2).unwrap(),
        branch.scan("items", 2).unwrap()
    );
    let forward = branch
        .primary_rows("items", None, None)
        .unwrap()
        .map(|r| r.unwrap().clone())
        .collect::<Vec<_>>();
    let backward = branch
        .primary_rows("items", None, None)
        .unwrap()
        .rev()
        .map(|r| r.unwrap().clone())
        .collect::<Vec<_>>();
    assert_eq!(forward.into_iter().rev().collect::<Vec<_>>(), backward);
}

#[test]
fn unchanged_rows_remain_shared_after_many_generations_and_other_views_are_dropped() {
    let mut current = populated();
    let origin = current.clone();
    let unchanged = origin.get("items", &key(1)).unwrap().unwrap();
    let mut held = Vec::new();
    for generation in 0..64 {
        held.push(current.clone());
        let mut next = current.clone();
        next.apply(Event {
            table_id: 1,
            kind: EventKind::Replace(vec![
                key(0).to_value(),
                Value::Text(format!("generation{generation}")),
            ]),
        })
        .unwrap();
        assert!(std::ptr::eq(
            unchanged,
            next.get("items", &key(1)).unwrap().unwrap()
        ));
        current = next;
    }
    for (generation, old) in held.iter().enumerate().skip(1) {
        assert_eq!(
            old.get("items", &key(0)).unwrap().unwrap()[1],
            Value::Text(format!("generation{}", generation - 1))
        );
    }
    drop(held);
    assert_eq!(
        current.get("items", &key(0)).unwrap().unwrap()[1],
        Value::Text("generation63".into())
    );
    assert!(std::ptr::eq(
        unchanged,
        current.get("items", &key(1)).unwrap().unwrap()
    ));
}

#[test]
fn full_row_capacity_preserves_shared_bodies_keys_and_order_at_all_key_boundaries() {
    for bytes in [0, 256, 3072] {
        let make_key = |number: u16| {
            if bytes == 0 {
                Key::Integer(i64::from(number))
            } else {
                Key::Text(format!("key-{number:08}{}", "я".repeat((bytes - 12) / 2)))
            }
        };
        let mut live = Snapshot::empty().unwrap();
        live.apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "items".into(),
                primary_key: 0,
                columns: vec![
                    Column {
                        name: "id".into(),
                        data_type: if bytes == 0 {
                            DataType::Integer
                        } else {
                            DataType::Text
                        },
                        nullable: false,
                    },
                    Column {
                        name: "body".into(),
                        data_type: DataType::Text,
                        nullable: false,
                    },
                ],
            }),
        })
        .unwrap();
        for number in 0..10000u16 {
            live.apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![
                    make_key(number).to_value(),
                    Value::Text("v".repeat(768)),
                ]),
            })
            .unwrap();
        }
        let old = live.clone();
        let before = old.page_fingerprint();
        live.apply(Event {
            table_id: 1,
            kind: EventKind::Replace(vec![make_key(0).to_value(), Value::Text("new".into())]),
        })
        .unwrap();
        assert_eq!(live.row_count(), 10000);
        for number in 1..10000u16 {
            let k = make_key(number);
            assert!(std::ptr::eq(
                old.get("items", &k).unwrap().unwrap(),
                live.get("items", &k).unwrap().unwrap()
            ));
            let a = old
                .state
                .table("items")
                .unwrap()
                .rows
                .get_key_value(&k)
                .unwrap()
                .0;
            let b = live
                .state
                .table("items")
                .unwrap()
                .rows
                .get_key_value(&k)
                .unwrap()
                .0;
            assert!(std::sync::Arc::ptr_eq(a, b));
            assert_eq!(
                old.row_location("items", &k).unwrap(),
                live.row_location("items", &k).unwrap()
            );
        }
        let lower = make_key(9996);
        let upper = make_key(9999);
        let interval = live
            .primary_rows("items", Some(&lower), Some(&upper))
            .unwrap()
            .map(|r| r.unwrap()[0].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            interval,
            (9996..9999)
                .map(|n| make_key(n).to_value())
                .collect::<Vec<_>>()
        );
        let backwards = live
            .primary_rows("items", Some(&lower), Some(&upper))
            .unwrap()
            .rev()
            .map(|r| r.unwrap()[0].clone())
            .collect::<Vec<_>>();
        assert_eq!(backwards, interval.into_iter().rev().collect::<Vec<_>>());
        assert_eq!(old.page_fingerprint(), before);
        let replay = Snapshot::from_pages(live.pages().cloned().collect()).unwrap();
        assert_eq!(
            replay.get("items", &make_key(0)).unwrap().unwrap()[1],
            Value::Text("new".into())
        );
        assert_eq!(replay.row_count(), 10000);
    }
}

#[test]
fn shared_keys_and_bodies_are_released_after_the_last_live_generation() {
    let old = populated();
    let mut live = old.clone();
    let table = old.state.table("items").unwrap();
    let (original_key, original_body) = table.rows.get_key_value(&key(1)).unwrap();
    let weak_key = std::sync::Arc::downgrade(original_key);
    let weak_body = std::sync::Arc::downgrade(original_body);
    live.apply(Event {
        table_id: 1,
        kind: EventKind::Replace(vec![key(0).to_value(), Value::Text("new".into())]),
    })
    .unwrap();
    drop(old);
    assert!(weak_key.upgrade().is_some());
    assert!(weak_body.upgrade().is_some());
    live.apply(Event {
        table_id: 1,
        kind: EventKind::Delete(key(1)),
    })
    .unwrap();
    assert!(weak_key.upgrade().is_none());
    assert!(weak_body.upgrade().is_none());
}

#[test]
fn raw_file_crud_and_owned_scans_preserve_private_bodies_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("raw.emily");
    let source = populated();
    let mut raw = crate::Database::create(&path).unwrap();
    raw.create_table(source.schema("items").unwrap().clone())
        .unwrap();
    for row in source.scan("items", 2).unwrap() {
        raw.insert("items", row).unwrap();
    }
    let mut scanned = raw.scan("items", 2).unwrap();
    scanned[1][1] = Value::Text("changed client-owned result".into());
    raw.update(
        "items",
        &key(0),
        vec![key(0).to_value(), Value::Text("raw update".into())],
    )
    .unwrap();
    assert_eq!(
        raw.get("items", &key(1)).unwrap().unwrap()[1],
        Value::Text("v".repeat(768))
    );
    drop(raw);
    let raw = crate::Database::open(&path).unwrap();
    assert_eq!(
        raw.get("items", &key(0)).unwrap().unwrap()[1],
        Value::Text("raw update".into())
    );
    assert_eq!(
        raw.get("items", &key(1)).unwrap().unwrap()[1],
        Value::Text("v".repeat(768))
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn only_accepted_replacements_detach_their_row_body(
        commands in prop::collection::vec((0u8..4,0u8..8,any::<bool>()),1..50)
    ) {
        let mut current=populated();
        let mut expected=BTreeMap::from([(0u8,"v".repeat(768)),(1u8,"v".repeat(768))]);
        for (operation,number,commit) in commands {
            let old=current.clone();let mut staged=current.clone();
            let previous=expected.clone();
            let value=format!("value{operation}/{number}");
            let event=match operation {
                0=>EventKind::Insert(vec![key(number).to_value(),Value::Text(value.clone())]),
                1=>EventKind::Replace(vec![key(number).to_value(),Value::Text(value.clone())]),
                2=>EventKind::Delete(key(number)),
                _=>EventKind::Replace(vec![key(number).to_value(),Value::Null]),
            };
            let allowed=match operation {0=>!expected.contains_key(&number),1|2=>expected.contains_key(&number),_=>false};
            prop_assert_eq!(staged.apply(Event {table_id:1,kind:event}).is_ok(),allowed);
            if allowed && commit {
                if operation==2 {expected.remove(&number);} else {expected.insert(number,value);}
                current=staged;
            }
            for (&n,body) in &previous {
                prop_assert_eq!(&old.get("items",&key(n)).unwrap().unwrap()[1],&Value::Text(body.clone()));
                if !(allowed && commit && n==number) {
                    prop_assert!(std::ptr::eq(old.get("items",&key(n)).unwrap().unwrap(),current.get("items",&key(n)).unwrap().unwrap()));
                }
            }
            let replay=Snapshot::from_pages(current.pages().cloned().collect()).unwrap();
            for (&n,body) in &expected {
                prop_assert_eq!(&current.get("items",&key(n)).unwrap().unwrap()[1],&Value::Text(body.clone()));
                prop_assert_eq!(replay.row_location("items",&key(n)).unwrap(),current.row_location("items",&key(n)).unwrap());
            }
            prop_assert_eq!(current.row_count(),expected.len());
        }
    }
}
