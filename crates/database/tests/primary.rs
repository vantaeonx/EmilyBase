use emilybase_catalog::{Column, DataType, Key, Row, Schema, Value};
use emilybase_database::{Event, EventKind, MAX_ROWS, Snapshot};
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;

fn table(snapshot: &mut Snapshot, name: &str, kind: DataType) -> u64 {
    let table_id = snapshot.next_table_id();
    snapshot
        .apply(Event {
            table_id,
            kind: EventKind::Create(Schema {
                name: name.into(),
                columns: vec![
                    Column {
                        name: "id".into(),
                        data_type: kind,
                        nullable: false,
                    },
                    Column {
                        name: "n".into(),
                        data_type: DataType::Integer,
                        nullable: false,
                    },
                ],
                primary_key: 0,
            }),
        })
        .unwrap();
    table_id
}
fn put(snapshot: &mut Snapshot, table_id: u64, row: Row, replace: bool) {
    snapshot
        .apply(Event {
            table_id,
            kind: if replace {
                EventKind::Replace(row)
            } else {
                EventKind::Insert(row)
            },
        })
        .unwrap();
}
fn row(key: &Key, number: i64) -> Row {
    vec![key.to_value(), Value::Integer(number)]
}
fn bytes(snapshot: &Snapshot) -> Vec<[u8; 4096]> {
    snapshot.pages().map(|page| page.encode()).collect()
}

#[test]
fn full_integer_and_maximum_index_text_capacity_preserve_all_ten_thousand_table_rows() {
    for kind in [DataType::Integer, DataType::Text] {
        let mut snapshot = Snapshot::empty().unwrap();
        let table_id = table(&mut snapshot, "t", kind);
        let keys = (0..MAX_ROWS)
            .map(|id| {
                if kind == DataType::Integer {
                    Key::Integer(id as i64)
                } else {
                    Key::Text(format!("{id:0256}"))
                }
            })
            .collect::<Vec<_>>();
        for (id, key) in keys.iter().enumerate() {
            put(&mut snapshot, table_id, row(key, id as i64), false);
        }
        let before = bytes(&snapshot);
        let info = snapshot.primary_index_info("t").unwrap();
        assert_eq!(info.entries, MAX_ROWS);
        assert_eq!(info.excluded_long_keys, 0);
        assert_eq!(info.pages, 768);
        assert!(info.root_id > 1);
        assert_eq!(bytes(&snapshot), before);
        for (id, key) in keys.iter().enumerate() {
            assert_eq!(snapshot.get("t", key).unwrap(), Some(&row(key, id as i64)));
        }
        let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
        assert_eq!(replay.primary_index_info("t").unwrap(), info);
        for id in [0, 13, 14, 511, 4096, MAX_ROWS - 1] {
            assert_eq!(
                replay.get("t", &keys[id]).unwrap(),
                Some(&row(&keys[id], id as i64))
            );
        }
        let extra = if kind == DataType::Integer {
            Key::Integer(MAX_ROWS as i64)
        } else {
            Key::Text("z".repeat(256))
        };
        assert!(
            snapshot
                .apply(Event {
                    table_id,
                    kind: EventKind::Insert(row(&extra, 1))
                })
                .is_err()
        );
        assert_eq!(snapshot.primary_index_info("t").unwrap(), info);
        assert_eq!(bytes(&snapshot), before);
    }
}

#[test]
fn mixed_utf8_keys_use_exact_256_byte_boundary_and_preserve_maximum_table_keys() {
    let mut snapshot = Snapshot::empty().unwrap();
    let table_id = table(&mut snapshot, "t", DataType::Text);
    let keys = [
        "".into(),
        "a".into(),
        format!("{}a", "界".repeat(85)),
        format!("{}ab", "界".repeat(85)),
        "界".repeat(1024),
    ];
    assert_eq!(keys[2].len(), 256);
    assert_eq!(keys[3].len(), 257);
    assert_eq!(keys[4].len(), 3072);
    for (id, text) in keys.iter().enumerate() {
        put(
            &mut snapshot,
            table_id,
            row(&Key::Text(text.clone()), id as i64),
            false,
        );
    }
    let info = snapshot.primary_index_info("t").unwrap();
    assert_eq!(
        (info.entries, info.excluded_long_keys, info.pages),
        (3, 2, 1)
    );
    for (id, text) in keys.iter().enumerate() {
        let key = Key::Text(text.clone());
        assert_eq!(
            snapshot.get("t", &key).unwrap(),
            Some(&row(&key, id as i64))
        );
    }
    for text in ["missing".into(), "x".repeat(257), "x".repeat(3072)] {
        assert!(snapshot.get("t", &Key::Text(text)).unwrap().is_none());
    }
    assert!(snapshot.get("t", &Key::Text("x".repeat(3073))).is_err());
    assert!(snapshot.get("t", &Key::Integer(1)).is_err());
    let long = Key::Text(keys[4].clone());
    put(&mut snapshot, table_id, row(&long, 99), true);
    assert_eq!(snapshot.get("t", &long).unwrap(), Some(&row(&long, 99)));
    snapshot
        .apply(Event {
            table_id,
            kind: EventKind::Delete(Key::Text(keys[2].clone())),
        })
        .unwrap();
    assert_eq!(snapshot.primary_index_info("t").unwrap().entries, 2);
    let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
    assert_eq!(
        replay.primary_index_info("t").unwrap(),
        snapshot.primary_index_info("t").unwrap()
    );
    assert_eq!(replay.get("t", &long).unwrap(), Some(&row(&long, 99)));
}

#[test]
fn immutable_snapshot_cache_survives_other_branches_and_drop_recreate_retires_only_current_table() {
    let mut snapshot = Snapshot::empty().unwrap();
    let id = table(&mut snapshot, "t", DataType::Integer);
    let sibling = table(&mut snapshot, "s", DataType::Integer);
    let key = Key::Integer(7);
    put(&mut snapshot, id, row(&key, 1), false);
    put(&mut snapshot, sibling, row(&key, 2), false);
    snapshot.primary_index_info("t").unwrap();
    snapshot.primary_index_info("s").unwrap();
    let historical = snapshot.clone();
    put(&mut snapshot, id, row(&key, 3), true);
    assert_eq!(historical.get("t", &key).unwrap(), Some(&row(&key, 1)));
    assert_eq!(snapshot.get("t", &key).unwrap(), Some(&row(&key, 3)));
    snapshot
        .apply(Event {
            table_id: id,
            kind: EventKind::Drop,
        })
        .unwrap();
    assert!(snapshot.primary_index_info("t").is_err());
    let recreated = table(&mut snapshot, "t", DataType::Integer);
    assert_ne!(id, recreated);
    assert_eq!(snapshot.primary_index_info("t").unwrap().entries, 0);
    assert!(snapshot.get("t", &key).unwrap().is_none());
    put(&mut snapshot, recreated, row(&key, 4), false);
    assert_eq!(snapshot.get("t", &key).unwrap(), Some(&row(&key, 4)));
    assert_eq!(snapshot.get("s", &key).unwrap(), Some(&row(&key, 2)));
    assert_eq!(historical.get("t", &key).unwrap(), Some(&row(&key, 1)));
}

#[test]
fn simultaneous_pure_readers_share_lazy_build_and_leave_physical_pages_unchanged() {
    let mut snapshot = Snapshot::empty().unwrap();
    let id = table(&mut snapshot, "t", DataType::Integer);
    for key in 0..200 {
        put(&mut snapshot, id, row(&Key::Integer(key), key * 7), false);
    }
    let before = bytes(&snapshot);
    let snapshot = Arc::new(snapshot);
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let threads = (0..8)
        .map(|_| {
            let snapshot = Arc::clone(&snapshot);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                for key in -1..201 {
                    let expected = (0..200)
                        .contains(&key)
                        .then(|| row(&Key::Integer(key), key * 7));
                    assert_eq!(
                        snapshot.get("t", &Key::Integer(key)).unwrap().cloned(),
                        expected
                    );
                }
                snapshot.primary_index_info("t").unwrap()
            })
        })
        .collect::<Vec<_>>();
    let info = snapshot.primary_index_info("t").unwrap();
    for thread in threads {
        assert_eq!(thread.join().unwrap(), info);
    }
    assert_eq!(bytes(&snapshot), before);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn independent_primary_model_covers_mutation_failed_writes_and_replayed_caches(
        operations in proptest::collection::vec((0u8..4,0i64..24,any::<i64>()),0..80)
    ) {
        let mut snapshot=Snapshot::empty().unwrap();
        let id=table(&mut snapshot,"t",DataType::Integer);
        let mut model=BTreeMap::new();
        for (action,key,value) in operations {
            let key_value=Key::Integer(key);
            let historical=snapshot.clone();
            let before=bytes(&snapshot);
            match action {
                0|1 => { put(&mut snapshot,id,row(&key_value,value),model.contains_key(&key));model.insert(key,value); }
                2 if model.contains_key(&key) => { snapshot.apply(Event{table_id:id,kind:EventKind::Delete(key_value)}).unwrap();model.remove(&key); }
                _ => {
                    let bad=Event{table_id:id,kind:if model.contains_key(&key) { EventKind::Insert(row(&key_value,value)) }
                        else { EventKind::Replace(row(&key_value,value)) }};
                    prop_assert!(snapshot.apply(bad).is_err());prop_assert_eq!(bytes(&snapshot),before);
                }
            }
            let replay=Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
            prop_assert_eq!(snapshot.primary_index_info("t").unwrap().entries,model.len());
            let live_info=snapshot.primary_index_info("t").unwrap();
            let replay_info=replay.primary_index_info("t").unwrap();
            prop_assert_eq!((live_info.entries,live_info.excluded_long_keys),(replay_info.entries,replay_info.excluded_long_keys));
            for probe in -1..25 {
                let expected=model.get(&probe).map(|value|row(&Key::Integer(probe),*value));
                prop_assert_eq!(snapshot.get("t",&Key::Integer(probe)).unwrap().cloned(),expected.clone());
                prop_assert_eq!(replay.get("t",&Key::Integer(probe)).unwrap().cloned(),expected);
            }
            // Initialization of another snapshot's cache does not mutate this image.
            historical.primary_index_info("t").unwrap();
        }
    }
}
