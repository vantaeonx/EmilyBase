use crate::{Error, Event, EventKind, Snapshot};
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Barrier};

fn snapshot() -> Snapshot {
    let mut value = Snapshot::empty().unwrap();
    for (id, name) in [(1, "changed"), (2, "untouched")] {
        value
            .apply(Event {
                table_id: id,
                kind: EventKind::Create(Schema {
                    name: name.into(),
                    columns: vec![
                        Column {
                            name: "id".into(),
                            data_type: DataType::Integer,
                            nullable: false,
                        },
                        Column {
                            name: "body".into(),
                            data_type: DataType::Text,
                            nullable: false,
                        },
                    ],
                    primary_key: 0,
                }),
            })
            .unwrap();
        value
            .apply(Event {
                table_id: id,
                kind: EventKind::Insert(vec![Value::Integer(1), Value::Text("x".repeat(768))]),
            })
            .unwrap();
    }
    value
}

#[test]
fn clone_shares_rows_and_first_mutation_detaches_only_the_changed_table() {
    let base = snapshot();
    let mut branch = base.clone();
    let key = Key::Integer(1);
    for name in ["changed", "untouched"] {
        assert!(std::ptr::eq(
            base.get(name, &key).unwrap().unwrap(),
            branch.get(name, &key).unwrap().unwrap()
        ));
    }
    branch
        .apply(Event {
            table_id: 1,
            kind: EventKind::Replace(vec![Value::Integer(1), Value::Text("new".into())]),
        })
        .unwrap();
    assert!(!std::ptr::eq(
        base.get("changed", &key).unwrap().unwrap(),
        branch.get("changed", &key).unwrap().unwrap()
    ));
    assert!(std::ptr::eq(
        base.get("untouched", &key).unwrap().unwrap(),
        branch.get("untouched", &key).unwrap().unwrap()
    ));
    assert_eq!(
        base.get("changed", &key).unwrap().unwrap()[1],
        Value::Text("x".repeat(768))
    );
    assert_eq!(
        branch.get("changed", &key).unwrap().unwrap()[1],
        Value::Text("new".into())
    );
}

#[test]
fn refused_events_preserve_shared_rows_locations_pages_and_history_digest() {
    let base = snapshot();
    let key = Key::Integer(1);
    let events = [
        Event {
            table_id: 1,
            kind: EventKind::Insert(vec![Value::Integer(1), Value::Text("duplicate".into())]),
        },
        Event {
            table_id: 1,
            kind: EventKind::Replace(vec![Value::Integer(9), Value::Text("missing".into())]),
        },
        Event {
            table_id: 1,
            kind: EventKind::Delete(Key::Integer(9)),
        },
        Event {
            table_id: 1,
            kind: EventKind::Replace(vec![
                Value::Text("wrong type".into()),
                Value::Text("value".into()),
            ]),
        },
        Event {
            table_id: 9,
            kind: EventKind::Drop,
        },
    ];
    for event in events {
        let mut branch = base.clone();
        let digest = base.page_fingerprint();
        assert!(branch.apply(event).is_err());
        assert_eq!(branch.page_fingerprint(), digest);
        assert_eq!(branch.event_count(), base.event_count());
        assert_eq!(
            branch.row_location("changed", &key).unwrap(),
            base.row_location("changed", &key).unwrap()
        );
        for name in ["changed", "untouched"] {
            assert!(std::ptr::eq(
                base.get(name, &key).unwrap().unwrap(),
                branch.get(name, &key).unwrap().unwrap()
            ));
        }
        assert!(
            base.pages()
                .zip(branch.pages())
                .all(|(a, b)| std::ptr::eq(a, b))
        );
    }
}

#[test]
fn old_physical_locations_and_borrowed_rows_survive_branch_replace_delete_and_drop() {
    let base = snapshot();
    let key = Key::Integer(1);
    let old = base.row_location("changed", &key).unwrap().unwrap();
    let mut replaced = base.clone();
    replaced
        .apply(Event {
            table_id: 1,
            kind: EventKind::Replace(vec![Value::Integer(1), Value::Text("new".into())]),
        })
        .unwrap();
    assert!(matches!(
        replaced.resolve_row_location("changed", &key, old),
        Err(Error::StaleLocation)
    ));
    let mut deleted = base.clone();
    deleted
        .apply(Event {
            table_id: 1,
            kind: EventKind::Delete(key.clone()),
        })
        .unwrap();
    assert!(deleted.get("changed", &key).unwrap().is_none());
    assert!(deleted.row_location("changed", &key).unwrap().is_none());
    let mut dropped = base.clone();
    dropped
        .apply(Event {
            table_id: 1,
            kind: EventKind::Drop,
        })
        .unwrap();
    assert!(matches!(dropped.get("changed", &key), Err(Error::NoTable)));
    for branch in [&replaced, &deleted, &dropped] {
        assert!(std::ptr::eq(
            base.get("untouched", &key).unwrap().unwrap(),
            branch.get("untouched", &key).unwrap().unwrap()
        ));
        assert_eq!(
            branch.row_location("untouched", &key).unwrap(),
            base.row_location("untouched", &key).unwrap()
        );
    }
    assert_eq!(
        base.resolve_row_location("changed", &key, old).unwrap()[1],
        Value::Text("x".repeat(768))
    );
    let mut rows = base.primary_rows("changed", None, None).unwrap();
    assert_eq!(
        rows.next().unwrap().unwrap()[1],
        Value::Text("x".repeat(768))
    );
}

#[test]
fn retired_table_name_reuse_does_not_reuse_a_historical_table_or_location_map() {
    let base = snapshot();
    let mut branch = base.clone();
    let schema = base.schema("changed").unwrap().clone();
    branch
        .apply(Event {
            table_id: 1,
            kind: EventKind::Drop,
        })
        .unwrap();
    branch
        .apply(Event {
            table_id: 3,
            kind: EventKind::Create(schema),
        })
        .unwrap();
    branch
        .apply(Event {
            table_id: 3,
            kind: EventKind::Insert(vec![Value::Integer(1), Value::Text("new table".into())]),
        })
        .unwrap();
    assert_eq!(branch.table_id("changed").unwrap(), 3);
    assert_eq!(base.table_id("changed").unwrap(), 1);
    let key = Key::Integer(1);
    assert_eq!(
        branch
            .row_location("changed", &key)
            .unwrap()
            .unwrap()
            .table_id,
        3
    );
    assert_eq!(
        base.row_location("changed", &key)
            .unwrap()
            .unwrap()
            .table_id,
        1
    );
    assert_eq!(branch.row_count(), 2);
    assert_eq!(base.row_count(), 2);
    assert!(std::ptr::eq(
        base.get("untouched", &key).unwrap().unwrap(),
        branch.get("untouched", &key).unwrap().unwrap()
    ));
}

#[test]
fn independent_writer_threads_detach_from_a_shared_immutable_base() {
    let base = Arc::new(snapshot());
    let barrier = Arc::new(Barrier::new(5));
    let writers: Vec<_> = (0..4)
        .map(|writer| {
            let base = Arc::clone(&base);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let mut branch = base.as_ref().clone();
                barrier.wait();
                for step in 0..32 {
                    branch
                        .apply(Event {
                            table_id: 1,
                            kind: EventKind::Replace(vec![
                                Value::Integer(1),
                                Value::Text(format!("{writer}/{step}")),
                            ]),
                        })
                        .unwrap();
                    let old = base
                        .row_location("changed", &Key::Integer(1))
                        .unwrap()
                        .unwrap();
                    assert_eq!(
                        base.resolve_row_location("changed", &Key::Integer(1), old)
                            .unwrap()[1],
                        Value::Text("x".repeat(768))
                    );
                    assert!(std::ptr::eq(
                        base.get("untouched", &Key::Integer(1)).unwrap().unwrap(),
                        branch.get("untouched", &Key::Integer(1)).unwrap().unwrap()
                    ));
                }
                branch
            })
        })
        .collect();
    barrier.wait();
    let values: Vec<_> = writers
        .into_iter()
        .map(|writer| writer.join().unwrap())
        .collect();
    for (writer, branch) in values.iter().enumerate() {
        assert_eq!(
            branch.get("changed", &Key::Integer(1)).unwrap().unwrap()[1],
            Value::Text(format!("{writer}/31"))
        );
        let replay = Snapshot::from_pages(branch.pages().cloned().collect()).unwrap();
        assert_eq!(
            replay.scan("changed", 10).unwrap(),
            branch.scan("changed", 10).unwrap()
        );
    }
}

#[test]
fn full_global_row_capacity_shares_untouched_wide_rows_across_many_views() {
    let mut base = snapshot();
    for number in 2..=crate::MAX_ROWS as i64 - 1 {
        base.apply(Event {
            table_id: 2,
            kind: EventKind::Insert(vec![
                Value::Integer(number),
                Value::Text("wide".repeat(192)),
            ]),
        })
        .unwrap();
    }
    assert_eq!(base.row_count(), crate::MAX_ROWS);
    let views: Vec<_> = (0..8).map(|_| base.clone()).collect();
    let mut branch = base.clone();
    let before = base.page_fingerprint();
    branch
        .apply(Event {
            table_id: 1,
            kind: EventKind::Replace(vec![
                Value::Integer(1),
                Value::Text("one changed row".into()),
            ]),
        })
        .unwrap();
    for key in 1..=crate::MAX_ROWS as i64 - 1 {
        let key = Key::Integer(key);
        let row = base.get("untouched", &key).unwrap().unwrap();
        assert!(std::ptr::eq(
            row,
            branch.get("untouched", &key).unwrap().unwrap()
        ));
        for view in &views {
            assert!(std::ptr::eq(
                row,
                view.get("untouched", &key).unwrap().unwrap()
            ));
        }
        assert_eq!(
            base.row_location("untouched", &key).unwrap(),
            branch.row_location("untouched", &key).unwrap()
        );
    }
    let count = branch.event_count();
    assert!(matches!(
        branch.apply(Event {
            table_id: 2,
            kind: EventKind::Insert(vec![
                Value::Integer(crate::MAX_ROWS as i64),
                Value::Text("over limit".into())
            ])
        }),
        Err(Error::Limit("live rows"))
    ));
    assert_eq!(branch.event_count(), count);
    assert_eq!(base.page_fingerprint(), before);
    assert_eq!(branch.row_count(), crate::MAX_ROWS);
}

#[test]
fn all_supported_value_types_remain_owned_by_the_correct_immutable_generation() {
    let mut value = Snapshot::empty().unwrap();
    value
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "typed".into(),
                primary_key: 0,
                columns: [
                    DataType::Integer,
                    DataType::Boolean,
                    DataType::Float,
                    DataType::Text,
                    DataType::Bytes,
                    DataType::Text,
                ]
                .into_iter()
                .enumerate()
                .map(|(i, data_type)| Column {
                    name: format!("c{i}"),
                    data_type,
                    nullable: i == 5,
                })
                .collect(),
            }),
        })
        .unwrap();
    let row = vec![
        Value::Integer(i64::MIN),
        Value::Boolean(true),
        Value::Float(1.25),
        Value::Text("я\0text".into()),
        Value::Bytes(vec![0, 255, 1]),
        Value::Null,
    ];
    value
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(row.clone()),
        })
        .unwrap();
    let old = value.clone();
    let mut changed = row.clone();
    changed[1] = Value::Boolean(false);
    changed[2] = Value::Float(-2.5);
    changed[3] = Value::Text("new".into());
    changed[4] = Value::Bytes(vec![4, 5]);
    changed[5] = Value::Text("now nonnull".into());
    value
        .apply(Event {
            table_id: 1,
            kind: EventKind::Replace(changed.clone()),
        })
        .unwrap();
    assert_eq!(
        old.get("typed", &Key::Integer(i64::MIN)).unwrap(),
        Some(&row)
    );
    assert_eq!(
        value.get("typed", &Key::Integer(i64::MIN)).unwrap(),
        Some(&changed)
    );
    let reopened = Snapshot::from_pages(value.pages().cloned().collect()).unwrap();
    assert_eq!(
        reopened.get("typed", &Key::Integer(i64::MIN)).unwrap(),
        Some(&changed)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn cloned_branches_match_independent_rows_and_physical_replay(
        commands in prop::collection::vec((0u8..4,0i64..8,0u16..1000),1..50), long in any::<bool>()
    ) {
        let mut current = Snapshot::empty().unwrap();
        let mut schema = snapshot().schema("changed").unwrap().clone();
        schema.columns[0].data_type = if long { DataType::Text } else { DataType::Integer };
        for (id,name) in [(1,"changed"),(2,"untouched")] {
            schema.name = name.into();
            current.apply(Event { table_id:id,kind:EventKind::Create(schema.clone()) }).unwrap();
        }
        let key = |number| if long { Key::Text(format!("{number:08}{}","я".repeat(1532))) } else { Key::Integer(number) };
        current.apply(Event { table_id:2,kind:EventKind::Insert(vec![key(0).to_value(),Value::Text("untouched".into())]) }).unwrap();
        let mut reference = BTreeMap::<Key,Vec<Value>>::new();
        for (operation,number,value) in commands {
            let historical = current.clone();
            let historical_rows = reference.clone();
            let historical_digest = historical.page_fingerprint();
            let mut staged = current.clone();
            let k = key(number);
            let row = vec![k.to_value(),Value::Text(format!("value-{value}"))];
            let expected = match operation {
                0 => !reference.contains_key(&k),
                1 | 2 => reference.contains_key(&k),
                _ => true,
            };
            let kind = match operation {
                0 => EventKind::Insert(row.clone()),
                1 => EventKind::Replace(row.clone()),
                2 => EventKind::Delete(k.clone()),
                _ => EventKind::Insert(vec![k.to_value(),Value::Text("rollback".into())]),
            };
            let result = staged.apply(Event { table_id:1,kind });
            if operation != 3 {
                prop_assert_eq!(result.is_ok(),expected);
                if expected {
                    if operation==2 { reference.remove(&k); } else { reference.insert(k,row); }
                    current=staged;
                }
            }
            prop_assert_eq!(historical.page_fingerprint(),historical_digest);
            prop_assert_eq!(historical.scan("changed",8).unwrap(),historical_rows.values().cloned().collect::<Vec<_>>());
            prop_assert_eq!(current.scan("changed",8).unwrap(),reference.values().cloned().collect::<Vec<_>>());
            prop_assert!(std::ptr::eq(current.get("untouched",&key(0)).unwrap().unwrap(),historical.get("untouched",&key(0)).unwrap().unwrap()));
            let replay=Snapshot::from_pages(current.pages().cloned().collect()).unwrap();
            prop_assert_eq!(replay.scan("changed",8).unwrap(),reference.values().cloned().collect::<Vec<_>>());
            for (k,row) in &reference {
                let location=current.row_location("changed",k).unwrap().unwrap();
                prop_assert_eq!(current.resolve_row_location("changed",k,location).unwrap(),row);
                prop_assert_eq!(replay.row_location("changed",k).unwrap(),Some(location));
            }
        }
    }
}
