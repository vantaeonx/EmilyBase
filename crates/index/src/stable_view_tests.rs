use crate::page::Body;
use crate::{BPlusTree, Error, IndexPage, IndexSnapshot, Key, RecordPointer};
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;

fn pointer(number: i64) -> RecordPointer {
    RecordPointer {
        page_id: number.unsigned_abs() + 1,
        slot_id: number as u16,
    }
}

fn dense(count: i64) -> BPlusTree {
    BPlusTree::from_sorted(
        &(0..count)
            .map(|n| (Key::Integer(n), pointer(n)))
            .collect::<Vec<_>>(),
    )
    .unwrap()
}

fn old_import(value: &BPlusTree) -> BPlusTree {
    BPlusTree::from_stable_pages(value.root_id(), &value.page_images().unwrap()).unwrap()
}

fn assert_unchanged(before: &BPlusTree, after: &BPlusTree) {
    assert_eq!(before, after);
    for (id, page) in &before.pages {
        assert!(Arc::ptr_eq(page, &after.pages[id]));
    }
}

#[test]
fn admitted_view_preserves_dense_images_and_shares_page_bodies() {
    let source = dense(200);
    let retained = source.clone();
    let images = source.page_images().unwrap();
    let stable = source.to_stable().unwrap();
    assert!(stable.has_stable_ids());
    assert!(!source.has_stable_ids());
    assert_eq!(stable, old_import(&source));
    assert_eq!(stable.page_images().unwrap(), images);
    assert_eq!(stable.root_id(), source.root_id());
    for (id, page) in &source.pages {
        assert!(Arc::ptr_eq(page, &stable.pages[id]));
    }
    assert_unchanged(&retained, &source);
    let again = stable.to_stable().unwrap();
    assert_unchanged(&stable, &again);
    let encoded = IndexSnapshot {
        revision: 1,
        tree: stable,
    }
    .encode()
    .unwrap();
    assert_eq!(IndexSnapshot::decode(&encoded).unwrap().tree, again);
}

#[test]
fn sparse_views_keep_holes_root_and_the_last_valid_arena_id() {
    let mut source = dense(200).to_stable().unwrap();
    for number in 0..180 {
        source.remove(&Key::Integer(number)).unwrap();
    }
    assert!(
        source
            .pages
            .keys()
            .copied()
            .ne(1..=source.page_count() as u64)
    );
    let stable = source.to_stable().unwrap();
    assert_unchanged(&source, &stable);
    assert_eq!(stable, old_import(&source));
    let mut boundary = BPlusTree::new_stable();
    boundary.pages.clear();
    boundary.pages.insert(
        1024,
        Arc::new(IndexPage::leaf(1024, Vec::new(), None).unwrap()),
    );
    boundary.root = 1024;
    let last = boundary.to_stable().unwrap();
    assert_unchanged(&boundary, &last);
    assert_eq!(last.root_id(), 1024);
    assert!(
        IndexSnapshot {
            revision: 1,
            tree: last
        }
        .encode()
        .is_ok()
    );
}

#[test]
fn returned_view_survives_source_release_and_detaches_its_own_mutations() {
    let mut source = dense(60);
    let mut stable = source.to_stable().unwrap();
    let leaf = source.find_leaf(Some(&Key::Integer(0))).unwrap();
    let weak = Arc::downgrade(&source.pages[&leaf]);
    source.replace(&Key::Integer(0), pointer(800)).unwrap();
    assert_eq!(stable.get(&Key::Integer(0)).unwrap(), Some(pointer(0)));
    drop(source);
    assert!(weak.upgrade().is_some());
    stable.replace(&Key::Integer(0), pointer(900)).unwrap();
    assert!(weak.upgrade().is_none());
    assert_eq!(stable.validate().unwrap(), 60);
}

#[test]
fn dense_and_stable_deletion_policies_do_not_leak_across_the_view() {
    let mut source = dense(120);
    let stable = source.to_stable().unwrap();
    let frozen = stable.page_images().unwrap();
    for number in 0..100 {
        source.remove(&Key::Integer(number)).unwrap();
    }
    assert!(!source.has_stable_ids());
    assert_eq!(
        source.pages.keys().copied().collect::<Vec<_>>(),
        (1..=source.page_count() as u64).collect::<Vec<_>>()
    );
    assert_eq!(stable.page_images().unwrap(), frozen);
    assert_eq!(stable.len(), 120);
    assert_eq!(source.to_stable().unwrap(), old_import(&source));
}

fn refuses_without_changes(source: BPlusTree) {
    let retained = source.clone();
    assert!(source.to_stable().is_err());
    assert_unchanged(&retained, &source);
}

#[test]
fn conversion_requires_exact_map_identity_domain_count_and_complete_topology() {
    let source = dense(60);
    let mut bad = source.clone();
    bad.len += 1;
    refuses_without_changes(bad);
    let mut bad = source.clone();
    bad.root = 0;
    refuses_without_changes(bad);
    let mut bad = source.clone();
    let id = bad.root;
    Arc::make_mut(bad.pages.get_mut(&id).unwrap()).id = 1024;
    assert!(bad.validate().is_ok());
    refuses_without_changes(bad);
    let mut bad = source.clone();
    let leaf = bad.find_leaf(Some(&Key::Integer(0))).unwrap();
    if let Body::Leaf { next, .. } = &mut Arc::make_mut(bad.pages.get_mut(&leaf).unwrap()).body {
        *next = Some(leaf);
    }
    refuses_without_changes(bad);
    let mut bad = source.clone();
    let root = bad.root;
    Arc::make_mut(bad.pages.get_mut(&root).unwrap()).keys[0] = Key::Integer(-1);
    refuses_without_changes(bad);
    let mut bad = source.clone();
    bad.pages.insert(
        1024,
        Arc::new(IndexPage::leaf(1024, Vec::new(), None).unwrap()),
    );
    refuses_without_changes(bad);
    let mut bad = source;
    let leaf = bad.find_leaf(Some(&Key::Integer(0))).unwrap();
    if let Body::Leaf { values, .. } = &mut Arc::make_mut(bad.pages.get_mut(&leaf).unwrap()).body {
        values[0].page_id = 0;
    }
    refuses_without_changes(bad);
}

#[test]
fn physical_arena_bounds_refuse_without_repairing_or_normalizing_ids() {
    let mut zero = BPlusTree::new();
    zero.pages.clear();
    assert_eq!(zero.to_stable(), Err(Error::Limit));
    let mut too_many = BPlusTree::new();
    for id in 2..=1025 {
        too_many
            .pages
            .insert(id, Arc::new(IndexPage::leaf(id, Vec::new(), None).unwrap()));
    }
    assert_eq!(too_many.to_stable(), Err(Error::Limit));
    let mut out_of_range = BPlusTree::new();
    out_of_range.pages.clear();
    out_of_range.pages.insert(
        1025,
        Arc::new(IndexPage::leaf(1025, Vec::new(), None).unwrap()),
    );
    out_of_range.root = 1025;
    assert!(out_of_range.validate().is_ok());
    assert_eq!(out_of_range.to_stable(), Err(Error::PageId));
    let original = BPlusTree::new();
    let empty = original.to_stable().unwrap();
    assert_eq!(empty, BPlusTree::new_stable());
    assert!(Arc::ptr_eq(&original.pages[&1], &empty.pages[&1]));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn checked_views_match_independent_rows_and_original_import_after_mutations(
        operations in prop::collection::vec((0u8..3, -40i64..40), 0..100), stable in any::<bool>()
    ) {
        let mut source = if stable { BPlusTree::new_stable() } else { BPlusTree::new() };
        let mut rows = BTreeMap::new();
        let mut retained = Vec::new();
        for (action, number) in operations {
            let key = Key::Integer(number);
            match action {
                0 if !rows.contains_key(&key) => {
                    source.insert(key.clone(), pointer(number)).unwrap();
                    rows.insert(key, pointer(number));
                }
                1 if rows.contains_key(&key) => {
                    source.replace(&key, pointer(number + 500)).unwrap();
                    rows.insert(key, pointer(number + 500));
                }
                2 if rows.contains_key(&key) => {
                    source.remove(&key).unwrap();
                    rows.remove(&key);
                }
                _ => (),
            }
            let old = source.clone();
            let admitted = source.to_stable().unwrap();
            assert_unchanged(&old, &source);
            prop_assert_eq!(&admitted, &old_import(&source));
            for (id, page) in &source.pages {
                prop_assert!(Arc::ptr_eq(page, &admitted.pages[id]));
            }
            prop_assert_eq!(admitted.range(None, None, 10000).unwrap(), rows.iter().map(|(k,v)| (k.clone(),*v)).collect::<Vec<_>>());
            if retained.len() < 4 { retained.push((admitted.clone(), admitted.page_images().unwrap())); }
            for (view, bytes) in &retained { prop_assert_eq!(view.page_images().unwrap(), bytes.clone()); }
        }
    }
}
