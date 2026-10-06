use emilybase_catalog::{DataType, Key, Value};
use emilybase_commit_format::Domain;
use emilybase_commit_model::{Error, MAX_EVENTS, MAX_SELECTED_INDEX_PAGES, Model, PlanCounts};
use emilybase_database::{Event, EventKind};
use proptest::prelude::*;
use std::collections::BTreeSet;
mod support;
use support::{indexes, model, schema};

fn insert(stage: &mut emilybase_commit_model::Staged, table: u64, key: i64, value: &str) {
    stage
        .apply(Event {
            table_id: table,
            kind: EventKind::Insert(vec![Value::Integer(key), Value::Text(value.into())]),
        })
        .unwrap();
}

#[test]
fn physical_plan_replays_both_tables_with_equal_page_numbers_in_distinct_namespaces() {
    let mut base = model(&[("left", DataType::Integer), ("right", DataType::Integer)]);
    let old = base.clone();
    let mut stage = base.begin().unwrap();
    insert(&mut stage, 1, 1, "left");
    insert(&mut stage, 2, 1, "right");
    indexes(&base, &mut stage, &["left", "right"]);
    let prepared = stage.prepare().unwrap();
    let plan = prepared.image_plan().unwrap();
    assert_eq!(plan.database_id(), base.database_id());
    assert_eq!(plan.base_transaction(), base.transaction());
    assert_eq!(plan.base_fingerprint(), base.fingerprint());
    assert_eq!(plan.transaction(), prepared.transaction());
    let addresses: BTreeSet<_> = plan
        .history()
        .iter()
        .map(|write| write.address())
        .chain(
            plan.roots()
                .iter()
                .flat_map(|root| root.upserts().iter().map(|write| write.address())),
        )
        .collect();
    assert_eq!(addresses.len(), 3);
    assert_eq!(
        plan.history()[0].address().domain(),
        Domain::RelationalHistory
    );
    for root in plan.roots() {
        assert_eq!(root.upserts()[0].address().domain(), Domain::PrimaryIndex);
        assert_eq!(
            root.upserts()[0].address().table(),
            root.binding().address().table()
        );
        assert_eq!(root.upserts()[0].address().page(), 1);
        assert_eq!(root.upserts()[0].image().len(), 4096);
        assert!(root.retired().is_empty());
    }
    let replayed = plan.replay(&base).unwrap();
    base.publish(prepared).unwrap();
    assert_eq!(replayed.fingerprint(), base.fingerprint());
    assert_eq!(replayed.fingerprint(), plan.next_fingerprint());
    for name in ["left", "right"] {
        assert_eq!(
            replayed.view().scan(name, 10).unwrap(),
            base.view().scan(name, 10).unwrap()
        );
        assert_eq!(
            replayed
                .view()
                .row_location(name, &Key::Integer(1))
                .unwrap(),
            base.view().row_location(name, &Key::Integer(1)).unwrap()
        );
    }
    assert_eq!(old.view().row_count(), 0);
    assert!(matches!(plan.replay(&base), Err(Error::Conflict)));
}

#[test]
fn unchanged_roots_are_omitted_and_index_only_plans_need_no_page_bodies() {
    let mut base = model(&[("left", DataType::Integer), ("right", DataType::Integer)]);
    let mut stage = base.begin().unwrap();
    insert(&mut stage, 1, 1, "left");
    indexes(&base, &mut stage, &["left"]);
    let prepared = stage.prepare().unwrap();
    let plan = prepared.image_plan().unwrap();
    assert_eq!(plan.roots().len(), 1);
    let replayed = plan.replay(&base).unwrap();
    assert!(std::ptr::eq(
        base.selection(2).unwrap(),
        replayed.selection(2).unwrap()
    ));
    base.publish(prepared).unwrap();
    let row = base.view().get("left", &Key::Integer(1)).unwrap().unwrap();
    let mut stage = base.begin().unwrap();
    stage.rebuild_index("left").unwrap();
    let prepared = stage.prepare().unwrap();
    let plan = prepared.image_plan().unwrap();
    assert!(plan.history().is_empty());
    assert!(plan.roots()[0].upserts().is_empty());
    assert_eq!(plan.counts().unwrap().image_body_bytes(), 0);
    let replayed = plan.replay(&base).unwrap();
    assert!(std::ptr::eq(
        row,
        replayed
            .view()
            .get("left", &Key::Integer(1))
            .unwrap()
            .unwrap()
    ));
    assert_eq!(
        replayed.selection(1).unwrap().binding().revision(),
        base.selection(1).unwrap().binding().revision() + 1
    );
}

#[test]
fn table_retirement_and_same_name_recreation_do_not_reuse_old_scope() {
    let mut base = model(&[("items", DataType::Integer)]);
    let old = base.clone();
    let mut stage = base.begin().unwrap();
    stage
        .apply(Event {
            table_id: 1,
            kind: EventKind::Drop,
        })
        .unwrap();
    stage
        .apply(Event {
            table_id: 2,
            kind: EventKind::Create(schema("items", DataType::Integer)),
        })
        .unwrap();
    insert(&mut stage, 2, 1, "recreated");
    indexes(&base, &mut stage, &["items"]);
    let prepared = stage.prepare().unwrap();
    let plan = prepared.image_plan().unwrap();
    assert_eq!(plan.counts().unwrap().retired_tables(), 1);
    assert_eq!(
        plan.retired_tables()[0].binding(),
        base.selection(1).unwrap().binding()
    );
    assert_eq!(
        plan.retired_tables()[0].index_fingerprint(),
        base.selection(1).unwrap().index_fingerprint()
    );
    assert_eq!(plan.roots()[0].binding().address().table(), 2);
    let replayed = plan.replay(&base).unwrap();
    assert!(replayed.selection(1).is_none());
    assert_eq!(replayed.view().table_id("items").unwrap(), 2);
    assert_eq!(old.view().table_id("items").unwrap(), 1);
    base.publish(prepared).unwrap();
    assert_eq!(base.fingerprint(), replayed.fingerprint());
}

#[test]
fn long_excluded_text_keys_and_current_short_pointers_replay_together() {
    let base = model(&[("items", DataType::Text)]);
    let mut stage = base.begin().unwrap();
    for key in ["λ".repeat(128), "λ".repeat(1536)] {
        stage
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Text(key), Value::Text("value".into())]),
            })
            .unwrap();
    }
    indexes(&base, &mut stage, &["items"]);
    let plan = stage.prepare().unwrap().image_plan().unwrap();
    let replayed = plan.replay(&base).unwrap();
    assert_eq!(
        (
            replayed.selection(1).unwrap().binding().covered(),
            replayed.selection(1).unwrap().binding().excluded()
        ),
        (1, 1)
    );
    assert_eq!(replayed.view().row_count(), 2);
    assert!(
        replayed
            .view()
            .get("items", &Key::Text("λ".repeat(1536)))
            .unwrap()
            .is_some()
    );
}

#[test]
fn combined_image_counts_use_separate_bounds_and_checked_bytes() {
    let max = PlanCounts::from_counts(
        MAX_EVENTS,
        MAX_SELECTED_INDEX_PAGES,
        MAX_SELECTED_INDEX_PAGES,
        128,
        128,
    )
    .unwrap();
    assert_eq!(max.history_pages(), 256);
    assert_eq!(max.primary_pages(), 2048);
    assert_eq!(max.retired_pages(), 2048);
    assert_eq!(max.changed_roots(), 128);
    assert_eq!(max.image_body_bytes(), 9437184);
    for fields in [
        (257, 1, 0, 1, 0),
        (0, 2049, 0, 1, 0),
        (0, 0, 2049, 1, 0),
        (0, 0, 0, 129, 0),
        (0, 0, 0, 0, 129),
        (usize::MAX, 0, 0, 0, 0),
        (0, usize::MAX, 0, 1, 0),
        (0, 0, usize::MAX, 1, 0),
        (0, 0, 0, usize::MAX, 0),
        (0, 0, 0, 0, usize::MAX),
        (0, 1, 0, 0, 0),
    ] {
        assert!(matches!(
            PlanCounts::from_counts(fields.0, fields.1, fields.2, fields.3, fields.4),
            Err(Error::Limit)
        ));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn physical_replay_matches_independent_accepted_and_discarded_row_updates(
        actions in prop::collection::vec((0i64..12, 0u8..3, any::<bool>()), 1..36)
    ) {
        let mut base = model(&[("items", DataType::Integer)]);
        let mut expected = std::collections::BTreeMap::new();
        for (key, operation, publish) in actions {
            let old = base.clone();
            let mut stage = base.begin().unwrap();
            let kind = match operation {
                0 => EventKind::Insert(vec![Value::Integer(key), Value::Text(key.to_string())]),
                1 => EventKind::Replace(vec![Value::Integer(key), Value::Text((key+1).to_string())]),
                _ => EventKind::Delete(Key::Integer(key)),
            };
            if stage.apply(Event { table_id: 1, kind }).is_err() { continue; }
            stage.rebuild_index("items").unwrap();
            let prepared = stage.prepare().unwrap();
            let plan = prepared.image_plan().unwrap();
            let replayed = plan.replay(&base).unwrap();
            let mut candidate = expected.clone();
            match operation { 0 => { candidate.insert(key,key.to_string()); }, 1 => { candidate.insert(key,(key+1).to_string()); }, _ => { candidate.remove(&key); } }
            for (key,value) in &candidate {
                prop_assert_eq!(&replayed.view().get("items",&Key::Integer(*key)).unwrap().unwrap()[1], &Value::Text(value.clone()));
            }
            prop_assert_eq!(replayed.view().row_count(), candidate.len());
            prop_assert_eq!(base.fingerprint(), old.fingerprint());
            if publish { base.publish(prepared).unwrap(); expected=candidate; prop_assert_eq!(base.fingerprint(), replayed.fingerprint()); }
            for (key,value) in &expected {
                prop_assert_eq!(&base.view().get("items",&Key::Integer(*key)).unwrap().unwrap()[1], &Value::Text(value.clone()));
            }
        }
    }
}

#[test]
fn full_10000_row_arena_readdressing_replays_768_images_beyond_table_page_bound() {
    use emilybase_index::{BPlusTree, IndexPage};
    let mut base = Model::new([7; 16]).unwrap();
    let mut stage = base.begin().unwrap();
    stage
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(schema("items", DataType::Integer)),
        })
        .unwrap();
    stage.rebuild_index("items").unwrap();
    base.publish(stage.prepare().unwrap()).unwrap();
    for start in (0..10000).step_by(MAX_EVENTS) {
        let mut stage = base.begin().unwrap();
        for key in start..(start + MAX_EVENTS).min(10000) {
            insert(&mut stage, 1, key as i64, "synthetic");
        }
        stage.rebuild_index("items").unwrap();
        base.publish(stage.prepare().unwrap()).unwrap();
    }
    // Incrementally maintained caches need not have the dense rebuild shape.
    let entries: Vec<_> = (0..10000)
        .map(|key| {
            let key = Key::Integer(key);
            let pointer = base
                .selection(1)
                .unwrap()
                .index()
                .tree
                .get(&key)
                .unwrap()
                .unwrap();
            (key, pointer)
        })
        .collect();
    let dense = BPlusTree::from_sorted_stable(&entries).unwrap();
    let mut canonical = base.begin().unwrap();
    let (binding, index) = support::candidate(&base, &canonical, "items", dense);
    canonical.index(binding, index).unwrap();
    base.publish(canonical.prepare().unwrap()).unwrap();
    let before = base.fingerprint();
    let original = base.selection(1).unwrap().index();
    let pages = original.tree.page_images().unwrap();
    assert_eq!(pages.len(), 768);
    let remap = |id: u64| 769 - id;
    let mut images = Vec::with_capacity(pages.len());
    for (index, image) in pages.iter().enumerate() {
        let page = IndexPage::decode(image, index as u64 + 1).unwrap();
        let transformed = if let Some(pointers) = page.pointers() {
            IndexPage::leaf(
                remap(page.id()),
                page.keys()
                    .iter()
                    .cloned()
                    .zip(pointers.iter().copied())
                    .collect(),
                page.next_leaf().map(remap),
            )
            .unwrap()
        } else {
            IndexPage::branch(
                remap(page.id()),
                page.keys().to_vec(),
                page.children()
                    .unwrap()
                    .iter()
                    .copied()
                    .map(remap)
                    .collect(),
            )
            .unwrap()
        };
        images.push(transformed.encode().unwrap());
    }
    images.reverse();
    let tree = BPlusTree::from_stable_pages(remap(original.tree.root_id()), &images).unwrap();
    let mut stage = base.begin().unwrap();
    let (binding, index) = support::candidate(&base, &stage, "items", tree);
    stage.index(binding, index).unwrap();
    let prepared = stage.prepare().unwrap();
    let plan = prepared.image_plan().unwrap();
    assert!(plan.history().is_empty());
    assert_eq!(plan.counts().unwrap().primary_pages(), 768);
    assert_eq!(plan.counts().unwrap().image_body_bytes(), 3145728);
    assert!(plan.counts().unwrap().primary_pages() > MAX_EVENTS);
    let replayed = plan.replay(&base).unwrap();
    assert_eq!(base.fingerprint(), before);
    base.publish(prepared).unwrap();
    assert_eq!(replayed.fingerprint(), base.fingerprint());
    for key in 0..10000 {
        let key = Key::Integer(key);
        let location = replayed
            .view()
            .row_location("items", &key)
            .unwrap()
            .unwrap();
        let pointer = replayed
            .selection(1)
            .unwrap()
            .index()
            .tree
            .get(&key)
            .unwrap()
            .unwrap();
        assert_eq!(
            (pointer.page_id, pointer.slot_id),
            (location.page_id, location.slot_id)
        );
    }
}

#[test]
fn equal_transaction_divergent_fork_is_not_an_authorized_plan_base() {
    let original = model(&[("items", DataType::Integer)]);
    let mut left = original.clone();
    let mut right = original;
    for (live, value) in [(&mut left, "left"), (&mut right, "right")] {
        let mut stage = live.begin().unwrap();
        insert(&mut stage, 1, 1, value);
        stage.rebuild_index("items").unwrap();
        live.publish(stage.prepare().unwrap()).unwrap();
    }
    assert_eq!(left.transaction(), right.transaction());
    let mut stage = left.begin().unwrap();
    insert(&mut stage, 1, 2, "next");
    stage.rebuild_index("items").unwrap();
    let plan = stage.prepare().unwrap().image_plan().unwrap();
    let before = right.fingerprint();
    assert!(matches!(plan.replay(&right), Err(Error::Conflict)));
    assert_eq!(right.fingerprint(), before);
}

#[test]
fn maximum_256_new_history_pages_replay_as_a_contiguous_append() {
    let mut base = model(&[("items", DataType::Integer)]);
    let value = "x".repeat(3072);
    let mut initial = base.begin().unwrap();
    for key in 0..256 {
        insert(&mut initial, 1, key, &value);
    }
    initial.rebuild_index("items").unwrap();
    base.publish(initial.prepare().unwrap()).unwrap();
    let count = base.view().page_count();
    let before = base.fingerprint();
    let mut stage = base.begin().unwrap();
    for key in 256..512 {
        insert(&mut stage, 1, key, &value);
    }
    stage.rebuild_index("items").unwrap();
    let plan = stage.prepare().unwrap().image_plan().unwrap();
    assert_eq!(plan.counts().unwrap().history_pages(), 256);
    for (index, write) in plan.history().iter().enumerate() {
        assert_eq!(write.address().page() as usize, count + index + 1);
    }
    let replayed = plan.replay(&base).unwrap();
    assert_eq!(replayed.view().row_count(), 512);
    assert_eq!(base.fingerprint(), before);
    assert_eq!(base.view().row_count(), 256);
    for key in 0..512 {
        assert_eq!(
            replayed
                .view()
                .get("items", &Key::Integer(key))
                .unwrap()
                .unwrap()[1],
            Value::Text(value.clone())
        );
    }
}
