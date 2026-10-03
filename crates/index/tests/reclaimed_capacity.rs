use emilybase_index::*;

fn pointer(id: u64) -> RecordPointer {
    RecordPointer {
        page_id: id + 1,
        slot_id: id as u16,
    }
}

#[test]
fn reclaimed_pages_allow_new_splits_after_actual_arena_exhaustion() {
    let mut tree = BPlusTree::new();
    let mut rejected = None;
    for key in 0..=MAX_INDEX_ENTRIES {
        let before = tree.clone();
        match tree.insert(Key::Integer(key as i64), pointer(key as u64)) {
            Ok(()) => (),
            Err(Error::Limit) => {
                assert_eq!(tree, before);
                rejected = Some(key);
                break;
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }
    let rejected = rejected.expect("the actual limit must be reached");
    let count = tree.page_count();
    for key in 0..256 {
        assert_eq!(
            tree.remove(&Key::Integer(key)).unwrap(),
            pointer(key as u64)
        );
    }
    assert!(tree.page_count() < count);
    let restored = BPlusTree::from_pages(tree.root_id(), &tree.page_images().unwrap()).unwrap();
    assert_eq!(restored, tree);
    for key in rejected..rejected + 256 {
        tree.insert(Key::Integer(key as i64), pointer(key as u64))
            .unwrap();
    }
    assert_eq!(tree.len(), rejected);
    assert_eq!(tree.validate().unwrap(), rejected);
    for key in 0..256 {
        assert_eq!(tree.get(&Key::Integer(key)).unwrap(), None);
    }
    for key in 256..rejected + 256 {
        assert_eq!(
            tree.get(&Key::Integer(key as i64)).unwrap(),
            Some(pointer(key as u64))
        );
    }
    assert_eq!(
        BPlusTree::from_pages(tree.root_id(), &tree.page_images().unwrap()).unwrap(),
        tree
    );
}

#[test]
fn full_entry_capacity_allows_replacement_then_reclaims_a_deleted_key() {
    let entries: Vec<_> = (0..MAX_INDEX_ENTRIES)
        .map(|i| (Key::Integer(i as i64), pointer(i as u64)))
        .collect();
    let mut tree = BPlusTree::from_sorted(&entries).unwrap();
    let full = tree.clone();
    let new_key = Key::Integer(MAX_INDEX_ENTRIES as i64);
    assert_eq!(
        tree.insert(new_key.clone(), pointer(20000)),
        Err(Error::Limit)
    );
    assert_eq!(tree, full);
    assert_eq!(
        tree.replace(&Key::Integer(700), pointer(30000)).unwrap(),
        pointer(700)
    );
    tree.remove(&Key::Integer(701)).unwrap();
    tree.insert(new_key.clone(), pointer(20000)).unwrap();
    assert_eq!(tree.len(), MAX_INDEX_ENTRIES);
    assert_eq!(tree.get(&Key::Integer(700)).unwrap(), Some(pointer(30000)));
    assert_eq!(tree.get(&Key::Integer(701)).unwrap(), None);
    assert_eq!(tree.get(&new_key).unwrap(), Some(pointer(20000)));
    assert_eq!(
        BPlusTree::from_pages(tree.root_id(), &tree.page_images().unwrap()).unwrap(),
        tree
    );
}
