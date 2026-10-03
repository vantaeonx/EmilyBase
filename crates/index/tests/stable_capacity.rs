use std::collections::BTreeSet;

use emilybase_index::*;

fn pointer(key: usize) -> RecordPointer {
    RecordPointer {
        page_id: key as u64 + 1,
        slot_id: key as u16,
    }
}
fn ids(tree: &BPlusTree) -> BTreeSet<u64> {
    tree.page_images()
        .unwrap()
        .iter()
        .map(|image| u64::from_le_bytes(image[8..16].try_into().unwrap()))
        .collect()
}

#[test]
fn exhausted_sparse_arena_reuses_retired_ids_and_retains_atomic_failure() {
    let mut tree = BPlusTree::new_stable();
    let mut capacity = None;
    for key in 0..=MAX_INDEX_ENTRIES {
        let previous = tree.clone();
        match tree.insert(Key::Integer(key as i64), pointer(key)) {
            Ok(()) => (),
            Err(Error::Limit) => {
                assert_eq!(tree, previous);
                capacity = Some(key);
                break;
            }
            other => panic!("unexpected bounded insertion outcome: {other:?}"),
        }
    }
    let capacity = capacity.expect("arena exhaustion must be exercised");
    assert_eq!(tree.page_count(), MAX_INDEX_PAGES);
    let original_ids = ids(&tree);
    let base = IndexSnapshot {
        revision: 7,
        tree: tree.clone(),
    };
    for key in 0..256 {
        assert_eq!(
            tree.remove(&Key::Integer(key)).unwrap(),
            pointer(key as usize)
        );
    }
    let survivors = ids(&tree);
    assert!(survivors.len() < original_ids.len());
    assert!(survivors.is_subset(&original_ids));
    let retired: BTreeSet<_> = original_ids.difference(&survivors).copied().collect();
    let delta = base.delta_to(&tree).unwrap();
    assert_eq!(
        delta.retired.iter().copied().collect::<BTreeSet<_>>(),
        retired
    );
    let deleted = delta.apply(&base).unwrap();
    assert_eq!(deleted.tree, tree);
    tree = IndexSnapshot::decode(&deleted.encode().unwrap())
        .unwrap()
        .tree;
    for key in capacity..capacity + 256 {
        tree.insert(Key::Integer(key as i64), pointer(key)).unwrap();
    }
    let final_ids = ids(&tree);
    assert!(final_ids.is_subset(&original_ids));
    assert!(survivors.is_subset(&final_ids));
    assert!(!final_ids.is_disjoint(&retired));
    assert_eq!(tree.len(), capacity);
    for key in 0..256 {
        assert_eq!(tree.get(&Key::Integer(key)).unwrap(), None);
    }
    for key in 256..capacity + 256 {
        assert_eq!(
            tree.get(&Key::Integer(key as i64)).unwrap(),
            Some(pointer(key))
        );
    }
    let final_snapshot = deleted.delta_to(&tree).unwrap().apply(&deleted).unwrap();
    assert_eq!(final_snapshot.tree, tree);
    assert_eq!(
        IndexSnapshot::decode(&final_snapshot.encode().unwrap()).unwrap(),
        final_snapshot
    );
}

#[test]
fn maximum_unicode_key_snapshots_preserve_mixed_type_order_and_opaque_targets() {
    let mut tree = BPlusTree::new_stable();
    let mut expected = Vec::new();
    for number in -40..40 {
        let entry = (
            Key::Integer(number),
            RecordPointer {
                page_id: u64::MAX,
                slot_id: u16::MAX,
            },
        );
        tree.insert(entry.0.clone(), entry.1).unwrap();
        expected.push(entry);
    }
    for number in 0..180 {
        let key = Key::Text(format!("{number:04}{}", "я".repeat(126)));
        assert!(matches!(&key,Key::Text(text) if text.len()==MAX_KEY_BYTES));
        let value = pointer(number);
        tree.insert(key.clone(), value).unwrap();
        expected.push((key, value));
    }
    let base = IndexSnapshot { revision: 1, tree };
    assert_eq!(
        base.tree.range(None, None, MAX_INDEX_ENTRIES).unwrap(),
        expected
    );
    let mut changed = base.tree.clone();
    for (key, _) in &expected[90..120] {
        changed.remove(key).unwrap();
    }
    let next = base.delta_to(&changed).unwrap().apply(&base).unwrap();
    let restored = IndexSnapshot::decode(&next.encode().unwrap()).unwrap();
    expected.drain(90..120);
    assert_eq!(
        restored.tree.range(None, None, MAX_INDEX_ENTRIES).unwrap(),
        expected
    );
    let expected_range = expected
        .iter()
        .filter(|(key, _)| matches!(key,Key::Text(text) if text.starts_with("000")))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        restored
            .tree
            .range(
                Some(&Key::Text("000".into())),
                Some(&Key::Text("001".into())),
                MAX_INDEX_ENTRIES
            )
            .unwrap(),
        expected_range
    );
}

#[test]
fn structurally_valid_delta_page_with_wrong_separator_is_rejected_atomically() {
    let base = IndexSnapshot {
        revision: 1,
        tree: BPlusTree::from_sorted_stable(
            &(0..30)
                .map(|key| (Key::Integer(key), pointer(key as usize)))
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    };
    let mut changed = base.tree.clone();
    changed.insert(Key::Integer(100), pointer(100)).unwrap();
    let mut delta = base.delta_to(&changed).unwrap();
    let root_id = base.tree.root_id();
    let valid_wrong_root = IndexPage::branch(
        root_id,
        vec![Key::Integer(9), Key::Integer(20)],
        vec![1, 2, 3],
    )
    .unwrap()
    .encode()
    .unwrap();
    delta
        .upserts
        .retain(|image| u64::from_le_bytes(image[8..16].try_into().unwrap()) != root_id);
    delta.upserts.push(valid_wrong_root);
    delta
        .upserts
        .sort_by_key(|image| u64::from_le_bytes(image[8..16].try_into().unwrap()));
    let before = base.encode().unwrap();
    assert!(delta.apply(&base).is_err());
    assert_eq!(base.encode().unwrap(), before);
}
