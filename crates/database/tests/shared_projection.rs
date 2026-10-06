use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_index::{BPlusTree, IndexSnapshot, RecordPointer};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn base(kind: DataType) -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "items".into(),
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
    snapshot
}

fn put(snapshot: &mut Snapshot, number: i64, value: i64, replace: bool) {
    let row = vec![Value::Integer(number), Value::Integer(value)];
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

fn populated() -> Snapshot {
    let mut snapshot = base(DataType::Integer);
    for number in 0..120 {
        put(&mut snapshot, number, number, false);
    }
    snapshot.primary_index_info("items").unwrap();
    snapshot
}

fn key_at(tree: &BPlusTree, number: i64) -> &Key {
    let bound = Key::Integer(number);
    let (key, _) = tree
        .cursor(Some(&bound), None)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(key, &bound);
    key
}

#[test]
fn repeated_exports_share_eligible_keys_and_preserve_relational_bytes() {
    let snapshot = populated();
    let before = snapshot.page_fingerprint();
    let info = snapshot.primary_index_info("items").unwrap();
    let first = snapshot.export_primary_tree("items").unwrap();
    let second = snapshot.export_primary_tree("items").unwrap();
    assert_eq!(first, second);
    assert_eq!(snapshot.verify_primary_tree("items", &first).unwrap(), info);
    for number in [0, 50, 119] {
        assert!(std::ptr::eq(
            key_at(&first, number),
            key_at(&second, number)
        ));
    }
    assert_eq!(snapshot.page_fingerprint(), before);
    let independent =
        BPlusTree::from_stable_pages(first.root_id(), &first.page_images().unwrap()).unwrap();
    assert_eq!(first, independent);
    assert!(!std::ptr::eq(key_at(&first, 50), key_at(&independent, 50)));
}

#[test]
fn external_mutation_and_source_mutation_detach_without_changing_old_exports() {
    let mut snapshot = populated();
    let old = snapshot.clone();
    let exported = old.export_primary_tree("items").unwrap();
    let bytes = exported.page_images().unwrap();
    let pointer = exported.get(&Key::Integer(0)).unwrap();
    let mut external = exported.clone();
    external
        .replace(
            &Key::Integer(0),
            RecordPointer {
                page_id: u64::MAX,
                slot_id: u16::MAX,
            },
        )
        .unwrap();
    assert!(snapshot.verify_primary_tree("items", &external).is_err());
    assert_eq!(
        snapshot
            .export_primary_tree("items")
            .unwrap()
            .get(&Key::Integer(0))
            .unwrap(),
        pointer
    );
    put(&mut snapshot, 0, 999, true);
    let current = snapshot.export_primary_tree("items").unwrap();
    assert!(!std::ptr::eq(key_at(&exported, 0), key_at(&current, 0)));
    assert!(std::ptr::eq(key_at(&exported, 119), key_at(&current, 119)));
    assert_eq!(exported.page_images().unwrap(), bytes);
    assert!(old.verify_primary_tree("items", &exported).is_ok());
    assert!(snapshot.verify_primary_tree("items", &exported).is_err());
    assert_eq!(
        old.get("items", &Key::Integer(0)).unwrap().unwrap()[1],
        Value::Integer(0)
    );
    assert_eq!(
        snapshot.get("items", &Key::Integer(0)).unwrap().unwrap()[1],
        Value::Integer(999)
    );
}

#[test]
fn installed_export_survives_source_release_and_keeps_stable_holes() {
    let mut snapshot = populated();
    let original = snapshot.export_primary_tree("items").unwrap();
    snapshot
        .install_primary_tree("items", original.clone())
        .unwrap();
    for number in 0..100 {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Delete(Key::Integer(number)),
            })
            .unwrap();
    }
    let surviving = snapshot.export_primary_tree("items").unwrap();
    assert!(surviving.has_stable_ids());
    let ids: Vec<_> = surviving
        .page_images()
        .unwrap()
        .iter()
        .map(|p| u64::from_le_bytes(p[8..16].try_into().unwrap()))
        .collect();
    assert_ne!(ids, (1..=surviving.page_count() as u64).collect::<Vec<_>>());
    assert_eq!(surviving.len(), 20);
    let before = snapshot.page_fingerprint();
    let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
    replay.verify_primary_tree("items", &surviving).unwrap();
    drop(snapshot);
    drop(original);
    assert_eq!(surviving.validate().unwrap(), 20);
    assert_eq!(replay.page_fingerprint(), before);
    for number in 100..120 {
        let key = Key::Integer(number);
        let location = replay.row_location("items", &key).unwrap().unwrap();
        let pointer = surviving.get(&key).unwrap().unwrap();
        assert_eq!(
            (pointer.page_id, pointer.slot_id),
            (location.page_id, location.slot_id)
        );
    }
}

#[test]
fn maximum_short_unicode_and_long_keys_keep_exact_coverage_and_fallback() {
    let mut snapshot = base(DataType::Text);
    let mut short = Vec::new();
    let mut long = Vec::new();
    for number in 0..30 {
        let text = format!("{number:03}\0{}", "я".repeat(126));
        assert_eq!(text.len(), 256);
        short.push(Key::Text(text.clone()));
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Text(text), Value::Integer(number)]),
            })
            .unwrap();
        let text = format!("long-{number:03}{}", "я".repeat(150));
        assert!(text.len() > 256);
        long.push(Key::Text(text.clone()));
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Text(text), Value::Integer(number)]),
            })
            .unwrap();
    }
    let exported = snapshot.export_primary_tree("items").unwrap();
    let info = snapshot.verify_primary_tree("items", &exported).unwrap();
    assert_eq!(info.entries, 30);
    assert_eq!(info.excluded_long_keys, 30);
    for key in short {
        let location = snapshot.row_location("items", &key).unwrap().unwrap();
        let pointer = exported.get(&key).unwrap().unwrap();
        assert_eq!(
            (pointer.page_id, pointer.slot_id),
            (location.page_id, location.slot_id)
        );
    }
    for key in long {
        assert!(snapshot.get("items", &key).unwrap().is_some());
    }
    let restored = IndexSnapshot::decode(
        &IndexSnapshot {
            revision: 1,
            tree: exported,
        }
        .encode()
        .unwrap(),
    )
    .unwrap();
    snapshot
        .verify_primary_tree("items", &restored.tree)
        .unwrap();
}

#[test]
fn independent_thread_snapshots_export_private_writes_and_shared_untouched_keys() {
    let snapshot = populated();
    let exported = snapshot.export_primary_tree("items").unwrap();
    let original = snapshot.page_fingerprint();
    let gate = std::sync::Barrier::new(5);
    let outputs = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|worker| {
                let reader = &snapshot;
                let gate = &gate;
                let untouched = &exported;
                scope.spawn(move || {
                    let mut writer = reader.clone();
                    gate.wait();
                    for number in 0..8 {
                        put(&mut writer, number, 1000 + worker * 100 + number, true);
                    }
                    let tree = writer.export_primary_tree("items").unwrap();
                    assert!(std::ptr::eq(key_at(untouched, 119), key_at(&tree, 119)));
                    assert!(!std::ptr::eq(key_at(untouched, 0), key_at(&tree, 0)));
                    writer.verify_primary_tree("items", &tree).unwrap();
                    (writer, tree)
                })
            })
            .collect();
        gate.wait();
        workers
            .into_iter()
            .map(|w| w.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(snapshot.page_fingerprint(), original);
    for (worker, (writer, tree)) in outputs.into_iter().enumerate() {
        writer.verify_primary_tree("items", &tree).unwrap();
        assert_eq!(
            writer.get("items", &Key::Integer(0)).unwrap().unwrap()[1],
            Value::Integer(1000 + worker as i64 * 100)
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn retained_projection_histories_preserve_rows_addresses_and_import_parity(
        actions in prop::collection::vec((0u8..3, -24i64..24, any::<i32>(), any::<bool>()), 0..80)
    ) {
        let mut snapshot = base(DataType::Integer);
        let mut rows = BTreeMap::new();
        let mut histories = Vec::new();
        for (operation, number, value, accepted) in actions {
            let mut staged = snapshot.clone();
            let mut wanted = rows.clone();
            if operation == 2 && wanted.contains_key(&number) {
                staged.apply(Event { table_id: 1, kind: EventKind::Delete(Key::Integer(number)) }).unwrap();
                wanted.remove(&number);
            } else if operation != 2 {
                put(&mut staged, number, i64::from(value), wanted.contains_key(&number));
                wanted.insert(number, i64::from(value));
            }
            let projected = staged.export_primary_tree("items").unwrap();
            let physical = BPlusTree::from_stable_pages(projected.root_id(), &projected.page_images().unwrap()).unwrap();
            prop_assert_eq!(&projected, &physical);
            prop_assert_eq!(projected.len(), wanted.len());
            staged.verify_primary_tree("items", &projected).unwrap();
            if histories.len() < 6 { histories.push((staged.clone(), projected, wanted.clone())); }
            if accepted { snapshot = staged; rows = wanted; }
            prop_assert_eq!(snapshot.scan("items", 10000).unwrap(), rows.iter().map(|(k,v)| vec![Value::Integer(*k),Value::Integer(*v)]).collect::<Vec<_>>());
            for (retained, tree, expected) in &histories {
                retained.verify_primary_tree("items", tree).unwrap();
                prop_assert_eq!(retained.scan("items", 10000).unwrap(), expected.iter().map(|(k,v)| vec![Value::Integer(*k),Value::Integer(*v)]).collect::<Vec<_>>());
                for key in expected.keys() {
                    let location = retained.row_location("items", &Key::Integer(*key)).unwrap().unwrap();
                    let pointer = tree.get(&Key::Integer(*key)).unwrap().unwrap();
                    prop_assert_eq!((pointer.page_id,pointer.slot_id),(location.page_id,location.slot_id));
                }
            }
        }
    }
}
