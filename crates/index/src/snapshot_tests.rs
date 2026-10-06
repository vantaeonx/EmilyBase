use crate::page::Body;
use crate::{BPlusTree, Error, IndexPage, IndexSnapshot, Key, MAX_INDEX_PAGES, RecordPointer};
use proptest::prelude::*;
use std::sync::Arc;

fn snapshot() -> IndexSnapshot {
    let entries: Vec<_> = (0..100)
        .map(|key| {
            (
                Key::Integer(key),
                RecordPointer {
                    page_id: key as u64 + 1,
                    slot_id: 0,
                },
            )
        })
        .collect();
    IndexSnapshot {
        revision: 1,
        tree: BPlusTree::from_sorted_stable(&entries).unwrap(),
    }
}

// Former whole-image admission is an independent regression oracle. It still
// rebuilds a distinct owned tree through the original untrusted-page decoder.
fn materialized(snapshot: &IndexSnapshot) -> bool {
    if snapshot.revision == 0 || !snapshot.tree.has_stable_ids() {
        return false;
    }
    if snapshot.tree.validate().ok() != Some(snapshot.tree.len()) {
        return false;
    }
    let Ok(images) = snapshot.tree.page_images() else {
        return false;
    };
    BPlusTree::from_stable_pages(snapshot.tree.root_id(), &images).is_ok()
}

fn refused(snapshot: IndexSnapshot) {
    let before = snapshot.clone();
    assert!(!materialized(&snapshot));
    assert!(snapshot.validate().is_err());
    assert!(snapshot.fingerprint().is_err());
    assert!(snapshot.encode().is_err());
    assert_eq!(snapshot, before);
}

#[test]
fn map_identity_is_checked_even_when_local_topology_and_page_encoding_succeed() {
    let mut value = snapshot();
    let leaf = *value
        .tree
        .pages
        .iter()
        .find(|(_, p)| p.is_leaf())
        .unwrap()
        .0;
    Arc::make_mut(value.tree.pages.get_mut(&leaf).unwrap()).id = 1024;
    assert!(value.tree.validate().is_ok());
    assert!(value.tree.pages[&leaf].encode().is_ok());
    assert!(matches!(value.validate(), Err(Error::PageId)));
    refused(value);
}

#[test]
fn complete_valid_topology_cannot_escape_the_stable_arena_domain() {
    let mut value = snapshot();
    let remap = |id: u64| id + MAX_INDEX_PAGES as u64;
    let mut pages = std::collections::BTreeMap::new();
    for source in value.tree.pages.into_values() {
        let mut page = source.as_ref().clone();
        page.id = remap(page.id);
        match &mut page.body {
            Body::Leaf { next, .. } => *next = next.map(remap),
            Body::Branch { children } => children.iter_mut().for_each(|id| *id = remap(*id)),
        }
        assert!(page.encode().is_ok());
        pages.insert(page.id, Arc::new(page));
    }
    value.tree.pages = pages;
    value.tree.root = remap(value.tree.root);
    assert_eq!(value.tree.validate().unwrap(), 100);
    refused(value);
}

#[test]
fn streamed_admission_preserves_empty_revision_and_stable_flag_refusal() {
    let mut value = snapshot();
    value.revision = 0;
    refused(value);
    let mut value = snapshot();
    value.tree.stable_ids = false;
    refused(value);
    let mut value = snapshot();
    value.tree.pages.clear();
    refused(value);
    let mut value = snapshot();
    value.tree.root = 0;
    refused(value);
    let mut value = snapshot();
    value.tree.root = u64::MAX;
    refused(value);
    let mut value = snapshot();
    value.tree.len = usize::MAX;
    refused(value);
}

#[test]
fn valid_local_pages_still_require_exact_separators_reachability_and_leaf_successors() {
    let mut value = snapshot();
    let root = value.tree.root;
    Arc::make_mut(value.tree.pages.get_mut(&root).unwrap()).keys[0] = Key::Integer(15);
    assert!(value.tree.pages[&root].encode().is_ok());
    refused(value);
    let mut value = snapshot();
    value.tree.pages.insert(
        1024,
        Arc::new(IndexPage::leaf(1024, Vec::new(), None).unwrap()),
    );
    refused(value);
    let mut value = snapshot();
    let leaf = *value
        .tree
        .pages
        .iter()
        .find(|(_, p)| p.next_leaf().is_some())
        .unwrap()
        .0;
    if let Body::Leaf { next, .. } =
        &mut Arc::make_mut(value.tree.pages.get_mut(&leaf).unwrap()).body
    {
        *next = None;
    }
    assert!(value.tree.pages[&leaf].encode().is_ok());
    refused(value);
}

#[test]
fn malformed_local_keys_pointers_and_branch_arity_cannot_pass_direct_validation() {
    let mut value = snapshot();
    let leaf = *value
        .tree
        .pages
        .iter()
        .find(|(_, p)| p.is_leaf())
        .unwrap()
        .0;
    Arc::make_mut(value.tree.pages.get_mut(&leaf).unwrap()).keys[0] = Key::Text("x".repeat(257));
    refused(value);
    let mut value = snapshot();
    if let Body::Leaf { values, .. } =
        &mut Arc::make_mut(value.tree.pages.get_mut(&leaf).unwrap()).body
    {
        values[0].page_id = 0;
    }
    refused(value);
    let mut value = snapshot();
    let root = value.tree.root;
    if let Body::Branch { children } =
        &mut Arc::make_mut(value.tree.pages.get_mut(&root).unwrap()).body
    {
        children.pop();
    }
    refused(value);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_valid_and_corrupted_arenas_match_materialized_import_admission(
        keys in prop::collection::btree_set(-200i64..200, 15..100), defect in 0u8..8
    ) {
        let entries: Vec<_> = keys.into_iter().map(|key| (Key::Integer(key), RecordPointer { page_id: key.unsigned_abs()+1, slot_id: key as u16 })).collect();
        let mut value = IndexSnapshot { revision: 1, tree: BPlusTree::from_sorted_stable(&entries).unwrap() };
        match defect {
            0 => (),
            1 => value.revision = 0,
            2 => value.tree.len += 1,
            3 => value.tree.root = 1024,
            4 => { value.tree.pages.insert(1024, Arc::new(IndexPage::leaf(1024,Vec::new(),None).unwrap())); },
            5 => value.tree.stable_ids = false,
            6 => { Arc::make_mut(value.tree.pages.values_mut().next().unwrap()).id = 1024; },
            _ => { Arc::make_mut(value.tree.pages.values_mut().find(|p| p.is_leaf()).unwrap()).keys[0] = Key::Text("x".repeat(257)); },
        }
        let expected = materialized(&value);
        prop_assert_eq!(value.validate().is_ok(),expected);
        prop_assert_eq!(value.fingerprint().is_ok(),expected);
        prop_assert_eq!(value.encode().is_ok(),expected);
    }
}
