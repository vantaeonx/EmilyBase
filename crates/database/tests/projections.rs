use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_index::{BPlusTree, IndexSnapshot, RecordPointer};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn initialized() -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "t".into(),
                columns: vec![
                    Column {
                        name: "id".into(),
                        data_type: DataType::Integer,
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
    snapshot
}
fn put(snapshot: &mut Snapshot, key: i64, value: i64, replace: bool) {
    let row = vec![Value::Integer(key), Value::Integer(value)];
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

#[test]
fn incomplete_extra_and_obsolete_trees_are_rejected_without_changing_the_current_cell() {
    let mut snapshot = initialized();
    for key in 0..32 {
        put(&mut snapshot, key, key * 7, false);
    }
    let tree = snapshot.export_primary_tree("t").unwrap();
    let mut incomplete = tree.clone();
    incomplete.remove(&Key::Integer(9)).unwrap();
    let mut extra = tree.clone();
    extra
        .insert(
            Key::Integer(99),
            RecordPointer {
                page_id: 1,
                slot_id: 0,
            },
        )
        .unwrap();
    let mut wrong = tree.clone();
    wrong
        .replace(
            &Key::Integer(9),
            RecordPointer {
                page_id: u64::MAX,
                slot_id: u16::MAX,
            },
        )
        .unwrap();
    let before = snapshot.page_fingerprint();
    let info = snapshot.primary_index_info("t").unwrap();
    for bad in [incomplete, extra, wrong, BPlusTree::new_stable()] {
        assert!(snapshot.install_primary_tree("t", bad).is_err());
        assert_eq!(snapshot.page_fingerprint(), before);
        assert_eq!(snapshot.primary_index_info("t").unwrap(), info);
        assert_eq!(snapshot.export_primary_tree("t").unwrap(), tree);
    }
    assert!(
        snapshot
            .install_primary_tree("missing", tree.clone())
            .is_err()
    );
    let historical = snapshot.clone();
    put(&mut snapshot, 9, 99, true);
    assert!(snapshot.install_primary_tree("t", tree.clone()).is_err());
    assert!(historical.verify_primary_tree("t", &tree).is_ok());
    assert_eq!(
        snapshot.get("t", &Key::Integer(9)).unwrap(),
        Some(&vec![Value::Integer(9), Value::Integer(99)])
    );
}

#[test]
fn installed_stable_tree_maintains_addresses_across_split_merge_and_replay() {
    let mut snapshot = initialized();
    for key in 0..64 {
        put(&mut snapshot, key, key, false);
    }
    let tree = snapshot.export_primary_tree("t").unwrap();
    let old = snapshot.clone();
    let fingerprint = snapshot.page_fingerprint();
    snapshot.install_primary_tree("t", tree.clone()).unwrap();
    assert_eq!(snapshot.page_fingerprint(), fingerprint);
    for key in 64..128 {
        put(&mut snapshot, key, key, false);
    }
    for key in 0..120 {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Delete(Key::Integer(key)),
            })
            .unwrap();
    }
    for key in 120..128 {
        put(&mut snapshot, key, -key, true);
    }
    let current = snapshot.export_primary_tree("t").unwrap();
    let info = snapshot.verify_primary_tree("t", &current).unwrap();
    assert_eq!(info.entries, 8);
    assert_eq!(info.pages, 1);
    assert!(snapshot.verify_primary_tree("t", &tree).is_err());
    assert!(old.verify_primary_tree("t", &tree).is_ok());
    let mut replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
    assert_eq!(replay.page_fingerprint(), snapshot.page_fingerprint());
    replay.install_primary_tree("t", current).unwrap();
    assert_eq!(
        replay.scan_integer_range("t", None, None, 100).unwrap(),
        snapshot.scan("t", 100).unwrap()
    );
    assert_eq!(
        replay.get("t", &Key::Integer(127)).unwrap(),
        Some(&vec![Value::Integer(127), Value::Integer(-127)])
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn exported_loaded_and_replayed_trees_match_an_independent_mutation_model(
        operations in proptest::collection::vec((0u8..3, -16i64..16, any::<i64>()), 0..64)
    ) {
        let mut snapshot = initialized();
        let mut model = BTreeMap::new();
        for (action, key, value) in operations {
            if action == 2 && model.contains_key(&key) {
                snapshot.apply(Event { table_id: 1, kind: EventKind::Delete(Key::Integer(key)) }).unwrap();
                model.remove(&key);
            } else if action != 2 {
                put(&mut snapshot, key, value, model.contains_key(&key));
                model.insert(key, value);
            }
            let tree = snapshot.export_primary_tree("t").unwrap();
            let encoded = IndexSnapshot { revision: 1, tree }.encode().unwrap();
            let decoded = IndexSnapshot::decode(&encoded).unwrap();
            let before = snapshot.page_fingerprint();
            snapshot.install_primary_tree("t", decoded.tree.clone()).unwrap();
            prop_assert_eq!(snapshot.page_fingerprint(), before);
            let mut replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
            replay.install_primary_tree("t", decoded.tree).unwrap();
            let expected = model.iter().map(|(key, value)| vec![Value::Integer(*key), Value::Integer(*value)]).collect::<Vec<_>>();
            prop_assert_eq!(snapshot.scan_integer_range("t", None, None, 100).unwrap(), expected.clone());
            prop_assert_eq!(replay.scan("t", 100).unwrap(), expected);
            for probe in -17..18 {
                prop_assert_eq!(snapshot.get("t", &Key::Integer(probe)).unwrap(), replay.get("t", &Key::Integer(probe)).unwrap());
            }
        }
    }
}
