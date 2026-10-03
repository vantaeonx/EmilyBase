use emilybase_index::*;

fn pointer(key: i64) -> RecordPointer {
    RecordPointer {
        page_id: key as u64 + 1001,
        slot_id: key as u16,
    }
}
fn entries(count: usize) -> Vec<(Key, RecordPointer)> {
    (0..count as i64)
        .map(|key| (Key::Integer(key), pointer(key)))
        .collect()
}
fn reopen(tree: &BPlusTree) -> BPlusTree {
    let restored = BPlusTree::from_pages(tree.root_id(), &tree.page_images().unwrap()).unwrap();
    assert_eq!(&restored, tree);
    restored
}
fn root(tree: &BPlusTree) -> IndexPage {
    IndexPage::decode(
        &tree.page_images().unwrap()[tree.root_id() as usize - 1],
        tree.root_id(),
    )
    .unwrap()
}

#[test]
fn bulk_balances_partial_last_nodes_at_each_height_boundary() {
    for count in [
        0,
        1,
        7,
        14,
        15,
        28,
        29,
        98,
        196,
        210,
        211,
        212,
        225,
        294,
        1000,
        MAX_INDEX_ENTRIES,
    ] {
        let source = entries(count);
        let tree = BPlusTree::from_sorted(&source).unwrap();
        assert_eq!(tree.len(), count);
        assert_eq!(tree.validate().unwrap(), count);
        assert_eq!(tree.range(None, None, MAX_INDEX_ENTRIES).unwrap(), source);
        for (key, pointer) in &source {
            assert_eq!(tree.get(key).unwrap(), Some(*pointer));
        }
        reopen(&tree);
        for image in tree.page_images().unwrap() {
            let id = u64::from_le_bytes(image[8..16].try_into().unwrap());
            let page = IndexPage::decode(&image, id).unwrap();
            if id != tree.root_id() {
                assert!(page.keys().len() >= MIN_KEYS);
            }
        }
        if count == MAX_INDEX_ENTRIES {
            assert_eq!(tree.page_count(), 768);
        }
    }
}

#[test]
fn bulk_rejects_duplicate_unsorted_oversized_and_invalid_targets_without_mutating_source() {
    let cases = vec![
        (
            vec![(Key::Integer(1), pointer(1)), (Key::Integer(1), pointer(2))],
            Error::Duplicate,
        ),
        (
            vec![(Key::Integer(2), pointer(2)), (Key::Integer(1), pointer(1))],
            Error::Layout("bulk input order"),
        ),
        (
            vec![(Key::Text("я".repeat(129)), pointer(0))],
            Error::KeySize,
        ),
        (
            vec![(
                Key::Integer(1),
                RecordPointer {
                    page_id: 0,
                    slot_id: 0,
                },
            )],
            Error::PageId,
        ),
        (entries(MAX_INDEX_ENTRIES + 1), Error::Limit),
    ];
    for (source, error) in cases {
        let original = source.clone();
        assert_eq!(BPlusTree::from_sorted(&source), Err(error));
        assert_eq!(source, original);
    }
}

#[test]
fn maximum_utf8_keys_bulk_build_canonically_and_remain_mutable() {
    let source: Vec<_> = (0..MAX_INDEX_ENTRIES)
        .map(|i| {
            (
                Key::Text(format!("{i:05}{}x", "я".repeat(125))),
                pointer(i as i64),
            )
        })
        .collect();
    let mut tree = BPlusTree::from_sorted(&source).unwrap();
    assert_eq!(tree, BPlusTree::from_sorted(&source).unwrap());
    assert_eq!(tree.range(None, None, MAX_INDEX_ENTRIES).unwrap(), source);
    assert_eq!(tree.remove(&source[5000].0).unwrap(), source[5000].1);
    let changed = RecordPointer {
        page_id: u64::MAX,
        slot_id: u16::MAX,
    };
    assert_eq!(
        tree.replace(&source[5010].0, changed).unwrap(),
        source[5010].1
    );
    tree.insert(source[5000].0.clone(), source[5000].1).unwrap();
    assert_eq!(tree.len(), MAX_INDEX_ENTRIES);
    assert_eq!(reopen(&tree).get(&source[5010].0).unwrap(), Some(changed));
}

#[test]
fn replacement_changes_one_image_and_preserves_root_keys_and_target_identity() {
    let mut tree = BPlusTree::from_sorted(&entries(300)).unwrap();
    let before = tree.page_images().unwrap();
    let old_root = tree.root_id();
    let value = RecordPointer {
        page_id: u64::MAX,
        slot_id: u16::MAX,
    };
    assert_eq!(
        tree.replace(&Key::Integer(117), value).unwrap(),
        pointer(117)
    );
    let after = tree.page_images().unwrap();
    assert_eq!(before.iter().zip(&after).filter(|(a, b)| a != b).count(), 1);
    assert_eq!(tree.root_id(), old_root);
    assert_eq!(tree.len(), 300);
    assert_eq!(tree.get(&Key::Integer(117)).unwrap(), Some(value));
    assert_eq!(tree.replace(&Key::Integer(117), value).unwrap(), value);
    assert_eq!(tree.page_images().unwrap(), after);
    reopen(&tree);
}

#[test]
fn rejected_maintenance_preserves_exact_pages_root_and_counts() {
    let mut tree = BPlusTree::from_sorted(&entries(300)).unwrap();
    let before = tree.clone();
    assert_eq!(tree.remove(&Key::Integer(301)), Err(Error::NoKey));
    assert_eq!(
        tree.replace(&Key::Integer(301), pointer(2)),
        Err(Error::NoKey)
    );
    assert_eq!(
        tree.replace(
            &Key::Integer(2),
            RecordPointer {
                page_id: 0,
                slot_id: 0
            }
        ),
        Err(Error::PageId)
    );
    assert_eq!(
        tree.remove(&Key::Text("x".repeat(257))),
        Err(Error::KeySize)
    );
    assert_eq!(
        tree.replace(&Key::Text("x".repeat(257)), pointer(0)),
        Err(Error::KeySize)
    );
    assert_eq!(tree, before);
    assert_eq!(tree.page_images().unwrap(), before.page_images().unwrap());
    let mut empty = BPlusTree::new();
    assert_eq!(empty.remove(&Key::Text("".into())), Err(Error::NoKey));
    assert_eq!(empty, BPlusTree::new());
}

fn two_leaves(left_count: i64, right_count: i64) -> BPlusTree {
    let leaf = |id, begin, count, next| {
        IndexPage::leaf(
            id,
            (begin..begin + count)
                .map(|key| (Key::Integer(key), pointer(key)))
                .collect(),
            next,
        )
        .unwrap()
        .encode()
        .unwrap()
    };
    let branch = IndexPage::branch(3, vec![Key::Integer(left_count)], vec![1, 2])
        .unwrap()
        .encode()
        .unwrap();
    BPlusTree::from_pages(
        3,
        &[
            leaf(1, 0, left_count, Some(2)),
            leaf(2, left_count, right_count, None),
            branch,
        ],
    )
    .unwrap()
}

#[test]
fn leaf_rotations_update_the_exact_separator_in_both_directions() {
    for (left, right, deleted, separator) in [(8, 7, 8, 7), (7, 8, 0, 8)] {
        let mut tree = two_leaves(left, right);
        assert_eq!(
            tree.remove(&Key::Integer(deleted)).unwrap(),
            pointer(deleted)
        );
        assert_eq!(tree.page_count(), 3);
        assert_eq!(root(&tree).keys(), &[Key::Integer(separator)]);
        let expected: Vec<_> = (0..left + right)
            .filter(|key| *key != deleted)
            .map(|key| (Key::Integer(key), pointer(key)))
            .collect();
        assert_eq!(tree.range(None, None, 100).unwrap(), expected);
        reopen(&tree);
    }
}

#[test]
fn leaf_merge_collapses_root_and_keeps_external_row_locations() {
    let mut tree = two_leaves(7, 7);
    tree.remove(&Key::Integer(0)).unwrap();
    assert_eq!(tree.page_count(), 1);
    assert_eq!(tree.root_id(), 1);
    assert!(root(&tree).is_leaf());
    assert_eq!(tree.range(None, None, 100).unwrap(), entries(14)[1..]);
    reopen(&tree);
}

fn height_three(left_children: usize, right_children: usize) -> BPlusTree {
    let leaves = left_children + right_children;
    let mut images = Vec::new();
    for leaf in 0..leaves {
        let id = leaf as u64 + 1;
        let values = (leaf * 7..leaf * 7 + 7)
            .map(|key| (Key::Integer(key as i64), pointer(key as i64)))
            .collect();
        images.push(
            IndexPage::leaf(id, values, (leaf + 1 < leaves).then_some(id + 1))
                .unwrap()
                .encode()
                .unwrap(),
        );
    }
    let left = leaves as u64 + 1;
    let right = left + 1;
    images.push(
        IndexPage::branch(
            left,
            (1..left_children)
                .map(|i| Key::Integer((i * 7) as i64))
                .collect(),
            (1..=left_children as u64).collect(),
        )
        .unwrap()
        .encode()
        .unwrap(),
    );
    images.push(
        IndexPage::branch(
            right,
            (left_children + 1..leaves)
                .map(|i| Key::Integer((i * 7) as i64))
                .collect(),
            (left_children as u64 + 1..=leaves as u64).collect(),
        )
        .unwrap()
        .encode()
        .unwrap(),
    );
    images.push(
        IndexPage::branch(
            right + 1,
            vec![Key::Integer((left_children * 7) as i64)],
            vec![left, right],
        )
        .unwrap()
        .encode()
        .unwrap(),
    );
    BPlusTree::from_pages(right + 1, &images).unwrap()
}

#[test]
fn internal_rotations_preserve_balanced_depth_and_change_root_separator() {
    for (left, right, deleted, separator) in [(9, 8, 63, 56), (8, 9, 0, 63)] {
        let mut tree = height_three(left, right);
        let original = tree.range(None, None, 1000).unwrap();
        tree.remove(&Key::Integer(deleted)).unwrap();
        assert_eq!(root(&tree).keys(), &[Key::Integer(separator)]);
        assert_eq!(tree.page_count(), left + right + 2);
        assert_eq!(
            tree.range(None, None, 1000).unwrap(),
            original
                .into_iter()
                .filter(|(key, _)| *key != Key::Integer(deleted))
                .collect::<Vec<_>>()
        );
        reopen(&tree);
    }
}

#[test]
fn internal_merge_collapses_a_tree_level_without_losing_leaf_links() {
    let mut tree = height_three(8, 8);
    tree.remove(&Key::Integer(0)).unwrap();
    assert_eq!(tree.page_count(), 16);
    let root = root(&tree);
    assert_eq!(root.keys().len(), MAX_KEYS);
    assert_eq!(root.children().unwrap().len(), MAX_KEYS + 1);
    for child in root.children().unwrap() {
        assert!(
            IndexPage::decode(&tree.page_images().unwrap()[*child as usize - 1], *child)
                .unwrap()
                .is_leaf()
        );
    }
    assert_eq!(tree.range(None, None, 1000).unwrap(), entries(112)[1..]);
    reopen(&tree);
}

#[test]
fn full_deletion_reclaims_every_node_for_ascending_descending_and_interleaved_orders() {
    for order in 0..3 {
        let mut tree = BPlusTree::new();
        for (key, value) in entries(1200).into_iter().rev() {
            tree.insert(key, value).unwrap();
        }
        let mut keys: Vec<_> = (0..1200i64).collect();
        if order == 1 {
            keys.reverse();
        }
        if order == 2 {
            keys.sort_by_key(|key| (*key % 7, *key));
        }
        for (i, key) in keys.into_iter().enumerate() {
            assert_eq!(tree.remove(&Key::Integer(key)).unwrap(), pointer(key));
            assert_eq!(tree.len(), 1199 - i);
            assert_eq!(tree.get(&Key::Integer(key)).unwrap(), None);
            if i % 71 == 0 {
                tree = reopen(&tree);
            }
        }
        assert_eq!(tree, BPlusTree::new());
        tree.insert(Key::Text("fresh".into()), pointer(1)).unwrap();
        assert_eq!(
            reopen(&tree).get(&Key::Text("fresh".into())).unwrap(),
            Some(pointer(1))
        );
    }
}
