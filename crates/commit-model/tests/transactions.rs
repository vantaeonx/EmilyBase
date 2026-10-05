use emilybase_catalog::{DataType, Key, Value};
use emilybase_commit_format::{IndexKeyType, PageAddress, Predecessor, RootBinding};
use emilybase_commit_model::{Error, MAX_EVENTS, Model};
use emilybase_database::{Event, EventKind};
use emilybase_index::{BPlusTree, IndexSnapshot, RecordPointer};
mod support;
use support::{candidate, indexes, model, schema};

fn put(staged: &mut emilybase_commit_model::Staged, name: &str, key: i64, value: &str) {
    let table_id = staged.view().unwrap().table_id(name).unwrap();
    staged
        .apply(Event {
            table_id,
            kind: EventKind::Insert(vec![Value::Integer(key), Value::Text(value.into())]),
        })
        .unwrap();
}

#[test]
fn prepare_and_publish_expose_complete_rows_and_indexes_and_preserve_old_views() {
    let mut live = model(&[("left", DataType::Integer), ("right", DataType::Integer)]);
    let old = live.clone();
    let fingerprint = live.fingerprint();
    let mut staged = live.begin().unwrap();
    put(&mut staged, "left", 1, "left");
    put(&mut staged, "right", 1, "right");
    indexes(&live, &mut staged, &["left", "right"]);
    let prepared = staged.prepare().unwrap();
    assert_eq!(live.fingerprint(), fingerprint);
    assert_eq!(old.view().row_count(), 0);
    assert_eq!(prepared.view().row_count(), 2);
    assert_eq!(prepared.selection(1).unwrap().binding().covered(), 1);
    assert_eq!(prepared.selection(2).unwrap().binding().covered(), 1);
    assert_eq!(prepared.transaction(), 3);
    live.publish(prepared).unwrap();
    assert_eq!(live.view().row_count(), 2);
    assert_eq!(old.view().row_count(), 0);
    assert_ne!(live.fingerprint(), fingerprint);
    for (table, name, value) in [(1, "left", "left"), (2, "right", "right")] {
        let location = live
            .view()
            .row_location(name, &Key::Integer(1))
            .unwrap()
            .unwrap();
        let pointer = live
            .selection(table)
            .unwrap()
            .index()
            .tree
            .get(&Key::Integer(1))
            .unwrap()
            .unwrap();
        assert_eq!(
            (pointer.page_id, pointer.slot_id),
            (location.page_id, location.slot_id)
        );
        assert_eq!(
            live.view().get(name, &Key::Integer(1)).unwrap().unwrap()[1],
            Value::Text(value.into())
        );
    }
}

#[test]
fn rollback_missing_candidates_duplicate_keys_and_stage_errors_keep_the_base_exact() {
    let live = model(&[("items", DataType::Integer)]);
    let before = live.fingerprint();
    let mut dropped = live.begin().unwrap();
    put(&mut dropped, "items", 1, "discarded");
    drop(dropped);
    let mut missing = live.begin().unwrap();
    put(&mut missing, "items", 1, "missing tree");
    assert!(missing.prepare().is_err());
    let mut failed = live.begin().unwrap();
    put(&mut failed, "items", 1, "valid first change");
    let table_id = failed.view().unwrap().table_id("items").unwrap();
    assert!(
        failed
            .apply(Event {
                table_id,
                kind: EventKind::Insert(vec![Value::Integer(1), Value::Text("duplicate".into())])
            })
            .is_err()
    );
    assert!(matches!(failed.view(), Err(Error::Aborted)));
    assert!(matches!(failed.prepare(), Err(Error::Aborted)));
    assert_eq!(live.fingerprint(), before);
    assert_eq!(live.view().row_count(), 0);
    assert!(matches!(live.begin().unwrap().prepare(), Err(Error::Empty)));
    assert!(Model::new([0; 16]).is_err());
}

#[test]
fn stale_prepared_writers_and_equal_transaction_divergent_forks_are_refused() {
    let mut left = model(&[("items", DataType::Integer)]);
    let mut right = left.clone();
    let mut a = left.begin().unwrap();
    put(&mut a, "items", 1, "left");
    indexes(&left, &mut a, &["items"]);
    let mut b = right.begin().unwrap();
    put(&mut b, "items", 1, "right");
    indexes(&right, &mut b, &["items"]);
    let a = a.prepare().unwrap();
    let b = b.prepare().unwrap();
    let mut stale = left.begin().unwrap();
    put(&mut stale, "items", 2, "stale");
    indexes(&left, &mut stale, &["items"]);
    let stale = stale.prepare().unwrap();
    left.publish(a).unwrap();
    right.publish(b).unwrap();
    let before = left.fingerprint();
    assert!(matches!(left.publish(stale), Err(Error::Conflict)));
    assert_eq!(left.fingerprint(), before);
    assert_eq!(left.transaction(), right.transaction());
    let mut from_right = right.begin().unwrap();
    put(&mut from_right, "items", 2, "foreign fork");
    indexes(&right, &mut from_right, &["items"]);
    assert!(matches!(
        left.publish(from_right.prepare().unwrap()),
        Err(Error::Conflict)
    ));
    assert_eq!(left.fingerprint(), before);
    assert_eq!(left.view().row_count(), 1);
}

#[test]
fn unchanged_table_roots_keep_their_old_transaction_and_exact_images() {
    let mut live = model(&[("left", DataType::Integer), ("right", DataType::Integer)]);
    let right = live.selection(2).unwrap().clone();
    for key in 1..4 {
        let mut staged = live.begin().unwrap();
        put(&mut staged, "left", key, "changed");
        indexes(&live, &mut staged, &["left"]);
        live.publish(staged.prepare().unwrap()).unwrap();
        assert_eq!(live.selection(2).unwrap(), &right);
        assert_eq!(live.selection(2).unwrap().binding().transaction(), 2);
    }
    let mut staged = live.begin().unwrap();
    put(&mut staged, "right", 1, "later");
    indexes(&live, &mut staged, &["right"]);
    let prepared = staged.prepare().unwrap();
    assert_eq!(prepared.selection(2).unwrap().binding().revision(), 2);
    assert_eq!(prepared.selection(2).unwrap().binding().transaction(), 6);
    live.publish(prepared).unwrap();
}

#[test]
fn dropped_table_retirement_and_same_name_recreation_never_reuse_scope() {
    let mut live = model(&[("items", DataType::Integer)]);
    let old = live.clone();
    let mut staged = live.begin().unwrap();
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Drop,
        })
        .unwrap();
    let prepared = staged.prepare().unwrap();
    assert_eq!(prepared.retired_tables(), &[1]);
    assert!(prepared.selection(1).is_none());
    live.publish(prepared).unwrap();
    let mut staged = live.begin().unwrap();
    staged
        .apply(Event {
            table_id: staged.view().unwrap().next_table_id(),
            kind: EventKind::Create(schema("items", DataType::Integer)),
        })
        .unwrap();
    indexes(&live, &mut staged, &["items"]);
    let prepared = staged.prepare().unwrap();
    assert!(prepared.selection(1).is_none());
    assert_eq!(prepared.selection(2).unwrap().binding().revision(), 1);
    live.publish(prepared).unwrap();
    assert_eq!(live.view().table_id("items").unwrap(), 2);
    assert_eq!(old.view().table_id("items").unwrap(), 1);
}

#[test]
fn obsolete_pointer_and_other_table_tree_are_not_authorized_by_valid_headers() {
    let mut live = model(&[("left", DataType::Integer), ("right", DataType::Integer)]);
    let mut initial = live.begin().unwrap();
    put(&mut initial, "left", 1, "left");
    put(&mut initial, "right", 1, "right");
    indexes(&live, &mut initial, &["left", "right"]);
    live.publish(initial.prepare().unwrap()).unwrap();
    let before = live.fingerprint();
    for foreign in [false, true] {
        let mut staged = live.begin().unwrap();
        staged
            .apply(Event {
                table_id: 1,
                kind: EventKind::Replace(vec![
                    Value::Integer(1),
                    Value::Text("new row image".into()),
                ]),
            })
            .unwrap();
        let previous = live.selection(1).unwrap();
        let source = live.selection(if foreign { 2 } else { 1 }).unwrap();
        let index = IndexSnapshot {
            revision: previous.index().revision + 1,
            tree: source.index().tree.clone(),
        };
        let binding = RootBinding::new(
            PageAddress::primary(live.database_id(), 1, index.tree.root_id()).unwrap(),
            IndexKeyType::Integer,
            index.revision,
            staged.transaction(),
            1,
            0,
            index.tree.page_count() as u32,
            Some(
                Predecessor::new(
                    previous.binding().revision(),
                    previous.binding().transaction(),
                    previous.index().fingerprint().unwrap(),
                )
                .unwrap(),
            ),
        )
        .unwrap();
        staged.index(binding, index).unwrap();
        assert!(staged.prepare().is_err());
        assert_eq!(live.fingerprint(), before);
    }
}

#[test]
fn incomplete_extra_wrong_owner_and_wrong_key_type_candidates_fail_as_a_whole() {
    let live = model(&[("items", DataType::Integer)]);
    let before = live.fingerprint();
    for fault in 0..5 {
        let mut staged = live.begin().unwrap();
        put(&mut staged, "items", 1, "new");
        let tree = staged.view().unwrap().export_primary_tree("items").unwrap();
        let (valid, index) = candidate(&live, &staged, "items", tree);
        let address = PageAddress::primary(
            if fault == 0 {
                [8; 16]
            } else {
                live.database_id()
            },
            if fault == 1 { 2 } else { 1 },
            valid.address().page(),
        )
        .unwrap();
        let binding = RootBinding::new(
            address,
            if fault == 2 {
                IndexKeyType::Text
            } else {
                valid.key_type()
            },
            valid.revision(),
            if fault == 3 {
                valid.transaction() + 1
            } else {
                valid.transaction()
            },
            valid.covered(),
            if fault == 4 { 1 } else { 0 },
            valid.pages(),
            valid.predecessor(),
        );
        if let Ok(binding) = binding {
            staged.index(binding, index).unwrap();
            assert!(staged.prepare().is_err());
        } else {
            assert_eq!(fault, 4);
        }
        assert_eq!(live.fingerprint(), before);
    }
    let mut staged = live.begin().unwrap();
    put(&mut staged, "items", 1, "new");
    indexes(&live, &mut staged, &["items"]);
    let extra_tree = BPlusTree::new_stable();
    let extra = RootBinding::new(
        PageAddress::primary(live.database_id(), 99, 1).unwrap(),
        IndexKeyType::Integer,
        1,
        staged.transaction(),
        0,
        0,
        1,
        None,
    )
    .unwrap();
    staged
        .index(
            extra,
            IndexSnapshot {
                revision: 1,
                tree: extra_tree,
            },
        )
        .unwrap();
    assert!(staged.prepare().is_err());
    assert_eq!(live.fingerprint(), before);
}

#[test]
fn wrong_exact_base_duplicate_candidate_and_unstable_images_abort_staging() {
    let live = model(&[("items", DataType::Integer)]);
    let before = live.fingerprint();
    let mut staged = live.begin().unwrap();
    put(&mut staged, "items", 1, "new");
    let tree = staged.view().unwrap().export_primary_tree("items").unwrap();
    let (valid, index) = candidate(&live, &staged, "items", tree);
    let previous = valid.predecessor().unwrap();
    let wrong = RootBinding::new(
        valid.address(),
        valid.key_type(),
        valid.revision(),
        valid.transaction(),
        valid.covered(),
        valid.excluded(),
        valid.pages(),
        Some(Predecessor::new(previous.revision(), previous.transaction(), [9; 32]).unwrap()),
    )
    .unwrap();
    staged.index(wrong, index).unwrap();
    assert!(staged.prepare().is_err());
    let mut staged = live.begin().unwrap();
    indexes(&live, &mut staged, &["items"]);
    let tree = staged.view().unwrap().export_primary_tree("items").unwrap();
    let (binding, index) = candidate(&live, &staged, "items", tree);
    assert!(staged.index(binding, index).is_err());
    assert!(matches!(staged.prepare(), Err(Error::Aborted)));
    let mut staged = live.begin().unwrap();
    let (binding, _) = candidate(&live, &staged, "items", BPlusTree::new_stable());
    assert!(
        staged
            .index(
                binding,
                IndexSnapshot {
                    revision: binding.revision(),
                    tree: BPlusTree::new()
                }
            )
            .is_err()
    );
    assert!(matches!(staged.prepare(), Err(Error::Aborted)));
    assert_eq!(live.fingerprint(), before);
}

#[test]
fn event_budget_aborts_the_entire_stage_including_earlier_valid_rows() {
    let live = model(&[("items", DataType::Integer)]);
    let mut staged = live.begin().unwrap();
    for key in 0..MAX_EVENTS {
        put(&mut staged, "items", key as i64, "accepted stage");
    }
    assert!(matches!(
        staged.apply(Event {
            table_id: 1,
            kind: EventKind::Insert(vec![
                Value::Integer(MAX_EVENTS as i64),
                Value::Text("overflow".into())
            ])
        }),
        Err(Error::Limit)
    ));
    assert!(matches!(staged.prepare(), Err(Error::Aborted)));
    assert_eq!(live.view().row_count(), 0);
    let mut staged = live.begin().unwrap();
    assert!(
        staged
            .apply(Event {
                table_id: 0,
                kind: EventKind::Root
            })
            .is_err()
    );
    assert!(matches!(staged.prepare(), Err(Error::Aborted)));
}

#[test]
fn long_text_counts_and_current_short_pointers_are_verified_together() {
    let mut live = model(&[("text", DataType::Text)]);
    let long = "λ".repeat(1536);
    let mut staged = live.begin().unwrap();
    for key in [
        "".to_owned(),
        "a\0b".to_owned(),
        "λ".repeat(128),
        long.clone(),
    ] {
        staged
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Text(key), Value::Text("value".into())]),
            })
            .unwrap();
    }
    indexes(&live, &mut staged, &["text"]);
    live.publish(staged.prepare().unwrap()).unwrap();
    let binding = live.selection(1).unwrap().binding();
    assert_eq!((binding.covered(), binding.excluded()), (3, 1));
    assert!(
        live.view()
            .get("text", &Key::Text(long.clone()))
            .unwrap()
            .is_some()
    );
    let old = live.clone();
    let mut staged = live.begin().unwrap();
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Delete(Key::Text(long.clone())),
        })
        .unwrap();
    indexes(&live, &mut staged, &["text"]);
    live.publish(staged.prepare().unwrap()).unwrap();
    assert_eq!(live.selection(1).unwrap().binding().excluded(), 0);
    assert!(old.view().get("text", &Key::Text(long)).unwrap().is_some());
}

#[test]
fn selected_topology_can_split_merge_and_reuse_arena_ids_without_mutating_old_views() {
    let mut live = model(&[("items", DataType::Integer)]);
    let mut staged = live.begin().unwrap();
    for key in 0..180 {
        put(&mut staged, "items", key, "initial");
    }
    indexes(&live, &mut staged, &["items"]);
    live.publish(staged.prepare().unwrap()).unwrap();
    let old = live.clone();
    let before = old.selection(1).unwrap().index().fingerprint().unwrap();
    let mut staged = live.begin().unwrap();
    let mut tree = live.selection(1).unwrap().index().tree.clone();
    for key in 0..150 {
        staged
            .apply(Event {
                table_id: 1,
                kind: EventKind::Delete(Key::Integer(key)),
            })
            .unwrap();
        tree.remove(&Key::Integer(key)).unwrap();
    }
    for key in 200..260 {
        put(&mut staged, "items", key, "reused");
        let location = staged
            .view()
            .unwrap()
            .row_location("items", &Key::Integer(key))
            .unwrap()
            .unwrap();
        tree.insert(
            Key::Integer(key),
            RecordPointer {
                page_id: location.page_id,
                slot_id: location.slot_id,
            },
        )
        .unwrap();
    }
    let (binding, index) = candidate(&live, &staged, "items", tree);
    staged.index(binding, index).unwrap();
    live.publish(staged.prepare().unwrap()).unwrap();
    assert_eq!(live.view().row_count(), 90);
    assert_eq!(old.view().row_count(), 180);
    assert_eq!(
        old.selection(1).unwrap().index().fingerprint().unwrap(),
        before
    );
    assert!(
        live.view()
            .get("items", &Key::Integer(1))
            .unwrap()
            .is_none()
    );
    assert!(old.view().get("items", &Key::Integer(1)).unwrap().is_some());
}
