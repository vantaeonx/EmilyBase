use crate::page::Body;
use crate::{BPlusTree, Error, IndexPage, Key, MAX_INDEX_PAGES, MAX_TREE_HEIGHT, RecordPointer};

fn tree() -> BPlusTree {
    BPlusTree::from_sorted(
        &(0..100)
            .map(|i| {
                (
                    Key::Integer(i),
                    RecordPointer {
                        page_id: i as u64 + 1,
                        slot_id: 0,
                    },
                )
            })
            .collect::<Vec<_>>(),
    )
    .unwrap()
}
fn assert_fused_error(tree: &BPlusTree, backwards: bool) {
    let mut cursor = tree.cursor(None, None).unwrap();
    let mut error = false;
    for _ in 0..MAX_INDEX_PAGES + 2 {
        match if backwards {
            cursor.next_back()
        } else {
            cursor.next()
        } {
            Some(Err(_)) => {
                error = true;
                break;
            }
            Some(Ok(_)) => {}
            None => break,
        }
    }
    assert!(error);
    assert!(cursor.next().is_none());
    assert!(cursor.next_back().is_none());
    assert_eq!(cursor.size_hint(), (0, Some(0)));
}

#[test]
fn borrowed_keys_are_the_actual_leaf_keys() {
    let tree = tree();
    let mut cursor = tree.cursor(None, None).unwrap();
    for backwards in [false, true, false, true] {
        let (key, _) = (if backwards {
            cursor.next_back()
        } else {
            cursor.next()
        })
        .unwrap()
        .unwrap();
        assert!(
            tree.pages
                .values()
                .filter(|p| p.is_leaf())
                .flat_map(|p| p.keys.iter())
                .any(|actual| std::ptr::eq(actual, key))
        );
    }
}

#[test]
fn wrong_leaf_links_missing_interior_pages_and_malformed_values_fail_and_fuse() {
    let original = tree();
    let leaves = original
        .pages
        .values()
        .filter(|page| page.is_leaf())
        .map(|page| page.id)
        .collect::<Vec<_>>();
    let middle = leaves[leaves.len() / 2];
    let mut missing = original.clone();
    missing.pages.remove(&middle);
    let mut wrong = original.clone();
    if let Body::Leaf { next, .. } = &mut wrong.pages.get_mut(&middle).unwrap().body {
        *next = None;
    }
    let mut values = original.clone();
    if let Body::Leaf { values, .. } = &mut values.pages.get_mut(&middle).unwrap().body {
        values.pop();
    }
    for bad in [missing, wrong, values] {
        for backwards in [false, true] {
            assert_fused_error(&bad, backwards);
        }
    }
}

#[test]
fn terminal_successor_and_inaccurate_counts_cannot_loop_or_invent_entries() {
    let mut wrong_count = tree();
    wrong_count.len = 1;
    for backwards in [false, true] {
        assert_fused_error(&wrong_count, backwards);
    }
    let mut terminal = tree();
    let last = terminal
        .pages
        .values_mut()
        .filter(|p| p.is_leaf())
        .last()
        .unwrap();
    if let Body::Leaf { next, .. } = &mut last.body {
        *next = Some(999);
    }
    assert_fused_error(&terminal, false);
}

#[test]
fn cyclic_missing_roots_and_height_limits_reject_without_panicking() {
    let mut missing = tree();
    missing.root = u64::MAX;
    assert!(missing.cursor(None, None).is_err());
    let mut cycle = tree();
    let root = cycle.root;
    if let Body::Branch { children } = &mut cycle.pages.get_mut(&root).unwrap().body {
        children[0] = root;
    }
    assert!(cycle.cursor(None, None).is_err());
    let mut deep = BPlusTree::new();
    deep.len = 1;
    let leaf = IndexPage::leaf(
        (MAX_TREE_HEIGHT + 1) as u64,
        vec![(
            Key::Integer(0),
            RecordPointer {
                page_id: 1,
                slot_id: 0,
            },
        )],
        None,
    )
    .unwrap();
    deep.pages.clear();
    deep.pages.insert(leaf.id, leaf);
    for id in (1..=MAX_TREE_HEIGHT as u64).rev() {
        deep.pages
            .insert(id, IndexPage::branch(id, Vec::new(), vec![id + 1]).unwrap());
    }
    assert!(matches!(deep.cursor(None, None), Err(Error::Limit)));
}

#[test]
fn over_capacity_and_invalid_bounds_validate_before_returning_an_empty_cursor() {
    let mut large = tree();
    large.len = crate::MAX_INDEX_ENTRIES + 1;
    assert!(matches!(large.cursor(None, None), Err(Error::Limit)));
    let empty = BPlusTree::new();
    let bad = Key::Text("z".repeat(257));
    assert!(matches!(
        empty.cursor(Some(&bad), Some(&bad)),
        Err(Error::KeySize)
    ));
}
