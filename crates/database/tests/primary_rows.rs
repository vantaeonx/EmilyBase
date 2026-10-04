use emilybase_catalog::{Column, DataType, Key, Row, Schema, Value};
use emilybase_database::{Event, EventKind, MAX_ROWS, Snapshot};
use proptest::prelude::*;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

fn initialized(kind: DataType) -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "t".into(),
                columns: vec![
                    Column {
                        name: "n".into(),
                        data_type: DataType::Integer,
                        nullable: true,
                    },
                    Column {
                        name: "id".into(),
                        data_type: kind,
                        nullable: false,
                    },
                    Column {
                        name: "payload".into(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                ],
                primary_key: 1,
            }),
        })
        .unwrap();
    snapshot
}
fn put(snapshot: &mut Snapshot, key: &Key, n: i64, replace: bool) {
    let row = vec![
        if n % 3 == 0 {
            Value::Null
        } else {
            Value::Integer(n)
        },
        key.to_value(),
        Value::Null,
    ];
    snapshot
        .apply(Event {
            table_id: 1,
            kind: if replace {
                EventKind::Replace(row)
            } else {
                EventKind::Insert(row)
            },
        })
        .unwrap();
}
fn rows(snapshot: &Snapshot, lower: Option<&Key>, upper: Option<&Key>, reverse: bool) -> Vec<Row> {
    let cursor = snapshot.primary_rows("t", lower, upper).unwrap();
    if reverse {
        cursor.rev().map(|row| row.unwrap().clone()).collect()
    } else {
        cursor.map(|row| row.unwrap().clone()).collect()
    }
}

#[test]
fn integer_intervals_and_arbitrary_ends_match_live_order_and_borrow_actual_rows() {
    let mut snapshot = initialized(DataType::Integer);
    for key in [i64::MIN, -10, -1, 0, 1, 14, 15, 200, i64::MAX] {
        put(&mut snapshot, &Key::Integer(key), key, false);
    }
    let before = snapshot.page_fingerprint();
    let keys = [
        None,
        Some(Key::Integer(i64::MIN)),
        Some(Key::Integer(-1)),
        Some(Key::Integer(14)),
        Some(Key::Integer(i64::MAX)),
    ];
    for lower in &keys {
        for upper in &keys {
            let expected = snapshot
                .scan("t", MAX_ROWS)
                .unwrap()
                .into_iter()
                .filter(|row| {
                    let key = snapshot.schema("t").unwrap().key(row).unwrap();
                    lower.as_ref().is_none_or(|bound| &key >= bound)
                        && upper.as_ref().is_none_or(|bound| &key < bound)
                })
                .collect::<Vec<_>>();
            assert_eq!(
                rows(&snapshot, lower.as_ref(), upper.as_ref(), false),
                expected
            );
            assert_eq!(
                rows(&snapshot, lower.as_ref(), upper.as_ref(), true),
                expected.iter().rev().cloned().collect::<Vec<_>>()
            );
            let mut want = VecDeque::from(expected);
            let mut cursor = snapshot
                .primary_rows("t", lower.as_ref(), upper.as_ref())
                .unwrap();
            for step in 0..want.len() + 3 {
                let (actual, expected) = if step % 2 == 0 {
                    (cursor.next(), want.pop_front())
                } else {
                    (cursor.next_back(), want.pop_back())
                };
                assert_eq!(actual.map(|row| row.unwrap().clone()), expected);
            }
            assert!(cursor.next().is_none());
            assert!(cursor.next_back().is_none());
        }
    }
    for borrowed in snapshot.primary_rows("t", None, None).unwrap() {
        let row = borrowed.unwrap();
        let key = snapshot.schema("t").unwrap().key(row).unwrap();
        assert!(std::ptr::eq(row, snapshot.get("t", &key).unwrap().unwrap()));
    }
    assert_eq!(snapshot.page_fingerprint(), before);
}

#[test]
fn long_text_bounds_and_keys_merge_before_the_caller_limit_in_both_directions() {
    let keys = [
        String::new(),
        "\0".into(),
        "a".into(),
        "a\0".into(),
        "b".into(),
        "界".into(),
        "😀".into(),
        format!("a{}", "x".repeat(255)),
        format!("a{}", "x".repeat(256)),
        format!("a{}", "x".repeat(3071)),
        "界".repeat(1024),
    ];
    let mut snapshot = initialized(DataType::Text);
    for (n, key) in keys.into_iter().enumerate() {
        put(&mut snapshot, &Key::Text(key), n as i64, false);
    }
    let bounds = [
        None,
        Some(Key::Text("".into())),
        Some(Key::Text("a".into())),
        Some(Key::Text("b".into())),
        Some(Key::Text("a".repeat(257))),
        Some(Key::Text("界".repeat(1024))),
    ];
    for lower in &bounds {
        for upper in &bounds {
            let mut expected = snapshot
                .scan("t", MAX_ROWS)
                .unwrap()
                .into_iter()
                .filter(|row| {
                    let key = snapshot.schema("t").unwrap().key(row).unwrap();
                    lower.as_ref().is_none_or(|bound| &key >= bound)
                        && upper.as_ref().is_none_or(|bound| &key < bound)
                })
                .collect::<Vec<_>>();
            for reverse in [false, true] {
                if reverse {
                    expected.reverse();
                }
                for limit in [0, 1, 2, 4, MAX_ROWS] {
                    let cursor = snapshot
                        .primary_rows("t", lower.as_ref(), upper.as_ref())
                        .unwrap();
                    let actual = if reverse {
                        cursor
                            .rev()
                            .take(limit)
                            .map(|r| r.unwrap().clone())
                            .collect::<Vec<_>>()
                    } else {
                        cursor.take(limit).map(|r| r.unwrap().clone()).collect()
                    };
                    assert_eq!(
                        actual,
                        expected.iter().take(limit).cloned().collect::<Vec<_>>()
                    );
                }
            }
        }
    }
    assert_eq!(
        snapshot.primary_index_info("t").unwrap().excluded_long_keys,
        3
    );
}

#[test]
fn bound_validation_precedes_empty_input_and_partial_consumption() {
    let int = initialized(DataType::Integer);
    let text = initialized(DataType::Text);
    let wrong = Key::Text("sensitive-bound".into());
    assert!(int.primary_rows("t", Some(&wrong), Some(&wrong)).is_err());
    let too_long = Key::Text("sensitive-bound".repeat(300));
    assert!(
        text.primary_rows("t", Some(&too_long), Some(&too_long))
            .is_err()
    );
    assert!(
        text.primary_rows("t", Some(&Key::Integer(1)), None)
            .is_err()
    );
    assert!(text.primary_rows("missing", None, None).is_err());
    assert!(text.primary_rows("t", None, None).unwrap().next().is_none());
    let lower = Key::Text("z".into());
    let upper = Key::Text("a".into());
    assert!(
        text.primary_rows("t", Some(&lower), Some(&upper))
            .unwrap()
            .next_back()
            .is_none()
    );
}

#[test]
fn cursors_outlive_temporary_names_and_bounds_but_borrow_only_the_snapshot() {
    let mut snapshot = initialized(DataType::Text);
    let long = format!("a{}", "x".repeat(3071));
    for key in ["a", &long, "b", "c"] {
        put(&mut snapshot, &Key::Text(key.into()), 1, false);
    }
    let mut short = {
        let name = String::from("t");
        let lower = Key::Text("a".into());
        let upper = Key::Text("c".into());
        snapshot
            .primary_rows(&name, Some(&lower), Some(&upper))
            .unwrap()
    };
    assert_eq!(short.next().unwrap().unwrap()[1], Value::Text("a".into()));
    assert_eq!(
        short.next_back().unwrap().unwrap()[1],
        Value::Text("b".into())
    );
    assert_eq!(short.next().unwrap().unwrap()[1], Value::Text(long.clone()));
    assert!(short.next_back().is_none());
    let mut long_bound = {
        let lower = Key::Text(long.clone());
        snapshot.primary_rows("t", Some(&lower), None).unwrap()
    };
    assert_eq!(long_bound.next().unwrap().unwrap()[1], Value::Text(long));
    assert_eq!(
        long_bound.next_back().unwrap().unwrap()[1],
        Value::Text("c".into())
    );
}

#[test]
fn historical_clones_replay_and_verified_installed_trees_preserve_borrowed_rows() {
    let mut current = initialized(DataType::Text);
    for key in ["a".into(), "b".into(), format!("a{}", "x".repeat(3071))] {
        put(&mut current, &Key::Text(key), 1, false);
    }
    let old = current.clone();
    let digest = old.page_fingerprint();
    let mut cursor = old.primary_rows("t", None, None).unwrap();
    assert_eq!(cursor.next().unwrap().unwrap()[1], Value::Text("a".into()));
    let image = current.export_primary_tree("t").unwrap();
    current.install_primary_tree("t", image).unwrap();
    put(&mut current, &Key::Text("b".into()), 8, true);
    current
        .apply(Event {
            table_id: 1,
            kind: EventKind::Delete(Key::Text("a".into())),
        })
        .unwrap();
    assert_eq!(cursor.next_back().unwrap().unwrap()[0], Value::Integer(1));
    assert_eq!(cursor.count(), 1);
    assert_eq!(old.page_fingerprint(), digest);
    let replay = Snapshot::from_pages(current.pages().cloned().collect()).unwrap();
    assert_eq!(
        rows(&current, None, None, true),
        rows(&replay, None, None, true)
    );
}

#[test]
fn full_capacity_wide_rows_support_short_borrowed_reads_and_shared_cold_readers() {
    let mut snapshot = initialized(DataType::Integer);
    for key in 0..MAX_ROWS {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![
                    Value::Null,
                    Value::Integer(key as i64),
                    Value::Text("x".repeat(3072)),
                ]),
            })
            .unwrap();
    }
    let snapshot = Arc::new(snapshot);
    let before = snapshot.page_fingerprint();
    let readers = (0..8)
        .map(|i| {
            let snapshot = Arc::clone(&snapshot);
            std::thread::spawn(move || {
                let lower = Key::Integer(i * 100);
                let upper = Key::Integer(i * 100 + 20);
                let first = snapshot
                    .primary_rows("t", Some(&lower), Some(&upper))
                    .unwrap()
                    .next_back()
                    .unwrap()
                    .unwrap();
                assert_eq!(first[1], Value::Integer(i * 100 + 19));
                let expected = snapshot
                    .get("t", &Key::Integer(i * 100 + 19))
                    .unwrap()
                    .unwrap();
                assert!(std::ptr::eq(first, expected));
            })
        })
        .collect::<Vec<_>>();
    for reader in readers {
        reader.join().unwrap();
    }
    assert_eq!(
        snapshot.primary_rows("t", None, None).unwrap().count(),
        MAX_ROWS
    );
    assert_eq!(snapshot.page_fingerprint(), before);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn generated_text_mutations_and_mixed_ends_match_independent_rows(
        operations in prop::collection::vec((0u8..3,0u8..25,any::<i16>()),1..65),
        lower in prop::option::of(0u8..25),upper in prop::option::of(0u8..25),ends in prop::collection::vec(any::<bool>(),0..40),
    ) {
        let mut snapshot=initialized(DataType::Text);let mut model=BTreeMap::<Key,Row>::new();
        let key=|n:u8|Key::Text(if n.is_multiple_of(3) {format!("{n:02}{}","z".repeat(3070))}else{format!("{n:02}\0界")});
        for (kind,n,v) in operations {
            let key=key(n);let row=vec![Value::Integer(i64::from(v)),key.to_value(),Value::Null];
            match kind {
                0 if !model.contains_key(&key)=>{snapshot.apply(Event{table_id:1,kind:EventKind::Insert(row.clone())}).unwrap();model.insert(key,row);}
                1 if model.contains_key(&key)=>{snapshot.apply(Event{table_id:1,kind:EventKind::Replace(row.clone())}).unwrap();model.insert(key,row);}
                2 if model.contains_key(&key)=>{snapshot.apply(Event{table_id:1,kind:EventKind::Delete(key.clone())}).unwrap();model.remove(&key);}
                _=>{},
            }
        }
        let lower=lower.map(key);let upper=upper.map(key);
        let mut want=model.iter().filter(|(k,_)|lower.as_ref().is_none_or(|b|*k>=b)&&upper.as_ref().is_none_or(|b|*k<b)).map(|(_,v)|v.clone()).collect::<VecDeque<_>>();
        let mut cursor=snapshot.primary_rows("t",lower.as_ref(),upper.as_ref()).unwrap();
        for backwards in ends {
            let (actual,expected)=if backwards {(cursor.next_back(),want.pop_back())}else{(cursor.next(),want.pop_front())};
            prop_assert_eq!(actual.map(|r|r.unwrap().clone()),expected);
        }
        prop_assert_eq!(cursor.map(|r|r.unwrap().clone()).collect::<Vec<_>>(),want.into_iter().collect::<Vec<_>>());
        let replay=Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
        prop_assert_eq!(rows(&snapshot,lower.as_ref(),upper.as_ref(),true),rows(&replay,lower.as_ref(),upper.as_ref(),true));
    }
}
