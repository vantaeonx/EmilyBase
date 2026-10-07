use crate::page::Body;
use crate::{BPlusTree, IndexPage, Key, RecordPointer};

fn key(value: &str) -> Key {
    let mut s = String::with_capacity(128 * 1024);
    s.push_str(value);
    Key::Text(s)
}
fn pointer() -> RecordPointer {
    RecordPointer {
        page_id: 1,
        slot_id: 0,
    }
}
fn shape(page: &IndexPage) {
    assert_eq!(page.keys.capacity(), page.keys.len());
    for key in &page.keys {
        if let Key::Text(text) = key {
            assert_eq!(text.capacity(), text.len());
        }
    }
    match &page.body {
        Body::Leaf { values, .. } => assert_eq!(values.capacity(), values.len()),
        Body::Branch { children } => assert_eq!(children.capacity(), children.len()),
    }
}
#[test]
fn public_leaf_constructor_does_not_retain_padded_text_or_vectors() {
    let mut entries = Vec::with_capacity(1024);
    entries.push((key("я\0"), pointer()));
    let page = IndexPage::leaf(1, entries, None).unwrap();
    shape(&page);
}
#[test]
fn public_branch_constructor_does_not_retain_padded_keys_or_children() {
    let mut keys = Vec::with_capacity(1024);
    keys.push(key("я\0"));
    let mut children = Vec::with_capacity(1024);
    children.extend([1, 2]);
    let page = IndexPage::branch(3, keys, children).unwrap();
    shape(&page);
}
#[test]
fn owned_tree_insertion_publishes_only_exact_payload_shapes() {
    let mut tree = BPlusTree::new_stable();
    tree.insert(key("я\0"), pointer()).unwrap();
    for page in tree.pages.values() {
        shape(page);
    }
    let (borrowed, actual) = tree.cursor(None, None).unwrap().next().unwrap().unwrap();
    assert_eq!(actual, pointer());
    if let Key::Text(text) = borrowed {
        assert_eq!(text.capacity(), text.len());
    } else {
        panic!("missing text")
    }
}

use crate::{Error, IndexSnapshot, MAX_INDEX_ENTRIES, MAX_KEY_BYTES, MAX_KEYS};
use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

fn padded(value: &str, capacity: usize) -> Key {
    let mut text = String::with_capacity(capacity);
    text.push_str(value);
    Key::Text(text)
}
fn location(number: u64) -> RecordPointer {
    RecordPointer {
        page_id: number + 1,
        slot_id: number as u16,
    }
}
fn arena(tree: &BPlusTree) {
    tree.validate().unwrap();
    for page in tree.pages.values() {
        shape(page);
    }
}
fn owners(before: &BPlusTree, after: &BPlusTree) {
    for (id, old) in &before.pages {
        if let Some(new) = after.pages.get(id) {
            assert_eq!(Arc::ptr_eq(old, new), old == new, "page {id}");
        }
    }
}
fn rows(tree: &BPlusTree, expected: &BTreeMap<Key, RecordPointer>) {
    arena(tree);
    assert_eq!(tree.len(), expected.len());
    let borrowed: Vec<_> = tree
        .cursor(None, None)
        .unwrap()
        .map(|entry| {
            let (key, pointer) = entry.unwrap();
            if let Key::Text(text) = key {
                assert_eq!(text.capacity(), text.len());
            }
            (key.clone(), pointer)
        })
        .collect();
    assert_eq!(
        borrowed,
        expected
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect::<Vec<_>>()
    );
}

#[test]
fn compaction_preserves_wire_bytes_links_and_exact_buffer_addresses() {
    let mut keys = Vec::with_capacity(1024);
    keys.extend([Key::Integer(i64::MIN), padded("", 1024), key("я\0")]);
    let mut values = Vec::with_capacity(1024);
    values.extend([location(0), location(1), location(2)]);
    let raw = IndexPage {
        id: 7,
        keys,
        body: Body::Leaf {
            values,
            next: Some(11),
        },
    };
    let wire = raw.encode().unwrap();
    let page = raw.compact_owned();
    shape(&page);
    assert_eq!(page.encode().unwrap(), wire);
    assert_eq!(page.next_leaf(), Some(11));
    let keys_address = page.keys.as_ptr();
    let pointers_address = page.pointers().unwrap().as_ptr();
    let texts: Vec<_> = page
        .keys
        .iter()
        .filter_map(|key| match key {
            Key::Text(value) => Some(value.as_ptr()),
            _ => None,
        })
        .collect();
    let page = page.compact_owned();
    assert_eq!(page.keys.as_ptr(), keys_address);
    assert_eq!(page.pointers().unwrap().as_ptr(), pointers_address);
    assert_eq!(
        page.keys
            .iter()
            .filter_map(|key| match key {
                Key::Text(value) => Some(value.as_ptr()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        texts
    );
    shape(&page);
}

#[test]
fn empty_and_maximum_utf8_leaf_payloads_have_exact_shapes() {
    let empty = IndexPage::leaf(1, Vec::with_capacity(4096), None).unwrap();
    shape(&empty);
    assert_eq!(empty.keys.capacity(), 0);
    assert_eq!(empty.pointers().unwrap(), &[]);
    let text = "🌌".repeat(MAX_KEY_BYTES / 4);
    let page = IndexPage::leaf(1, vec![(key(&text), location(u64::MAX - 1))], None).unwrap();
    shape(&page);
    let decoded = IndexPage::decode(&page.encode().unwrap(), 1).unwrap();
    shape(&decoded);
    assert_eq!(page, decoded);
    assert_eq!(decoded.keys(), &[Key::Text(text)]);
}

#[test]
fn branch_decode_does_not_retain_geometric_child_growth() {
    for count in 0..=MAX_KEYS {
        let keys: Vec<_> = (0..count)
            .map(|n| padded(&format!("k{n:03}\0"), 1024))
            .collect();
        let children: Vec<_> = (1..=count as u64 + 1).collect();
        let page = IndexPage::branch(99, keys, children).unwrap();
        shape(&page);
        let encoded = page.encode().unwrap();
        let restored = IndexPage::decode(&encoded, 99).unwrap();
        shape(&restored);
        assert_eq!(restored, page);
        assert_eq!(restored.encode().unwrap(), encoded);
    }
}

#[test]
fn invalid_owned_pages_still_return_the_original_typed_errors() {
    assert_eq!(IndexPage::leaf(0, vec![], None), Err(Error::PageId));
    assert_eq!(
        IndexPage::leaf(1, vec![], Some(1)),
        Err(Error::Layout("leaf successor"))
    );
    assert_eq!(
        IndexPage::leaf(
            1,
            vec![(
                key("x"),
                RecordPointer {
                    page_id: 0,
                    slot_id: 0
                }
            )],
            None
        ),
        Err(Error::Layout("leaf pointers"))
    );
    assert_eq!(
        IndexPage::leaf(1, vec![(key("x"), pointer()), (key("x"), pointer())], None),
        Err(Error::Layout("unsorted or duplicate keys"))
    );
    assert_eq!(
        IndexPage::leaf(1, vec![(key(&"x".repeat(257)), pointer())], None),
        Err(Error::KeySize)
    );
    assert_eq!(
        IndexPage::branch(3, vec![key("x")], vec![1]),
        Err(Error::Layout("branch children"))
    );
    assert_eq!(
        IndexPage::branch(3, vec![key("x")], vec![1, 1]),
        Err(Error::Layout("branch children"))
    );
    assert_eq!(
        IndexPage::branch(3, vec![key("x")], vec![1, 3]),
        Err(Error::Layout("branch children"))
    );
    assert_eq!(
        IndexPage::branch(3, vec![key("z"), key("a")], vec![1, 2, 4]),
        Err(Error::Layout("unsorted or duplicate keys"))
    );
    assert_eq!(
        IndexPage::branch(99, (0..15).map(Key::Integer).collect(), (1..=16).collect()),
        Err(Error::Limit)
    );
}

#[test]
fn insert_replace_borrow_merge_and_root_collapse_compact_both_id_policies() {
    for stable in [false, true] {
        let mut tree = if stable {
            BPlusTree::new_stable()
        } else {
            BPlusTree::new()
        };
        let mut expected = BTreeMap::new();
        for number in (0..180).rev() {
            let text = format!("k{number:03}я\0");
            let prior = tree.clone();
            let key = padded(&text, 4096);
            expected.insert(key.clone(), location(number));
            tree.insert(key, location(number)).unwrap();
            owners(&prior, &tree);
            rows(&tree, &expected);
        }
        let retained = tree.clone();
        let encoded = retained.page_images().unwrap();
        let mut merges = 0;
        let mut collapse = false;
        for number in (0..90).flat_map(|n| [n, 179 - n]) {
            let key = padded(&format!("k{number:03}я\0"), 4096);
            let prior = tree.clone();
            assert_eq!(
                tree.replace(&key, location(number + 1000)).unwrap(),
                location(number)
            );
            expected.insert(key.clone(), location(number + 1000));
            owners(&prior, &tree);
            rows(&tree, &expected);
            let prior = tree.clone();
            assert_eq!(tree.remove(&key).unwrap(), expected.remove(&key).unwrap());
            owners(&prior, &tree);
            merges += usize::from(prior.page_count() > tree.page_count());
            collapse |= prior.root_id() != tree.root_id();
            rows(&tree, &expected);
            arena(&retained);
            assert_eq!(retained.page_images().unwrap(), encoded);
        }
        assert!(merges > 10);
        assert!(collapse);
        assert_eq!(tree.page_count(), 1);
        assert_eq!(tree.pages[&tree.root].keys.capacity(), 0);
        assert_eq!(retained.len(), 180);
    }
}

#[test]
fn stable_hole_reuse_compacts_new_owners_and_preserves_retired_owners() {
    let entries: Vec<_> = (0..120)
        .map(|n| (padded(&format!("k{n:03}"), 4096), location(n)))
        .collect();
    let original = BPlusTree::from_sorted_stable(&entries).unwrap();
    let frozen = original.page_images().unwrap();
    let mut tree = original.clone();
    for n in 0..90 {
        tree.remove(&padded(&format!("k{n:03}"), 4096)).unwrap();
        arena(&tree);
    }
    let retired: BTreeSet<_> = original
        .pages
        .keys()
        .filter(|id| !tree.pages.contains_key(id))
        .copied()
        .collect();
    assert!(!retired.is_empty());
    for n in 200..400 {
        let old = tree.clone();
        tree.insert(padded(&format!("k{n:03}"), 4096), location(n))
            .unwrap();
        arena(&tree);
        owners(&old, &tree);
    }
    let reused: Vec<_> = retired
        .iter()
        .filter(|id| tree.pages.contains_key(id))
        .collect();
    assert!(!reused.is_empty());
    for id in reused {
        assert!(!Arc::ptr_eq(&original.pages[id], &tree.pages[id]));
        assert_ne!(original.pages[id], tree.pages[id]);
    }
    assert_eq!(original.page_images().unwrap(), frozen);
    arena(&original);
}

#[test]
fn unchanged_publication_keeps_exact_shared_page_and_text_addresses() {
    let mut tree = BPlusTree::new_stable();
    tree.insert(key("я\0"), pointer()).unwrap();
    let old = tree.clone();
    let source = Arc::clone(&tree.pages[&1]);
    let address = source.keys.as_ptr();
    let candidate = IndexPage {
        id: 1,
        keys: vec![key("я\0")],
        body: Body::Leaf {
            values: vec![pointer()],
            next: None,
        },
    };
    tree.publish_page(candidate);
    assert!(Arc::ptr_eq(&source, &tree.pages[&1]));
    assert_eq!(tree.pages[&1].keys.as_ptr(), address);
    assert_eq!(tree.replace(&key("я\0"), pointer()).unwrap(), pointer());
    assert_eq!(tree.insert(key("я\0"), location(2)), Err(Error::Duplicate));
    assert_eq!(
        tree.insert(
            key("z"),
            RecordPointer {
                page_id: 0,
                slot_id: 0
            }
        ),
        Err(Error::PageId)
    );
    assert_eq!(tree.remove(&key("z")), Err(Error::NoKey));
    owners(&old, &tree);
    arena(&tree);
}

#[test]
fn full_capacity_bulk_import_and_snapshot_replay_have_exact_page_shapes() {
    let suffix = "λ".repeat(124);
    let entries: Vec<_> = (0..MAX_INDEX_ENTRIES)
        .map(|n| (padded(&format!("{n:08}{suffix}"), 1024), location(n as u64)))
        .collect();
    let tree = BPlusTree::from_sorted_stable(&entries).unwrap();
    assert_eq!(tree.page_count(), 768);
    arena(&tree);
    let snapshot = IndexSnapshot { revision: 8, tree };
    let encoded = snapshot.encode().unwrap();
    let decoded = IndexSnapshot::decode(&encoded).unwrap();
    arena(&decoded.tree);
    assert_eq!(decoded, snapshot);
    assert_eq!(
        decoded.fingerprint().unwrap(),
        snapshot.fingerprint().unwrap()
    );
    let imported = BPlusTree::from_stable_pages(
        snapshot.tree.root_id(),
        &snapshot.tree.page_images().unwrap(),
    )
    .unwrap();
    arena(&imported);
    assert_eq!(imported, snapshot.tree);
    let mut target = snapshot.tree.clone();
    let first = entries[0].0.clone();
    target.replace(&first, location(20000)).unwrap();
    let delta = snapshot.delta_to(&target).unwrap();
    assert_eq!(delta.upserts.len(), 1);
    let applied = delta.apply(&snapshot).unwrap();
    arena(&applied.tree);
    owners(&snapshot.tree, &applied.tree);
    assert_eq!(applied.tree, target);
    assert_eq!(snapshot.encode().unwrap(), encoded);
}

#[test]
fn final_historical_owner_releases_compact_replaced_page() {
    let mut tree = BPlusTree::new_stable();
    tree.insert(key("я\0"), pointer()).unwrap();
    let old = tree.clone();
    let weak = Arc::downgrade(&old.pages[&1]);
    tree.replace(&key("я\0"), location(100)).unwrap();
    drop(tree);
    assert!(weak.upgrade().is_some());
    arena(&old);
    assert_eq!(old.get(&key("я\0")).unwrap(), Some(pointer()));
    drop(old);
    assert!(weak.upgrade().is_none());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn spare_capacity_mutations_match_independent_map_and_preserve_history(
        commands in prop::collection::vec((0u8..4, 0u8..80, 0usize..4096, 1u64..10000), 1..80),
        stable in any::<bool>(),
    ) {
        let mut tree = if stable {BPlusTree::new_stable()} else {BPlusTree::new()};
        let mut expected = BTreeMap::new();
        let mut retained = Vec::new();
        for (step, (operation, number, capacity, value)) in commands.into_iter().enumerate() {
            let key = padded(&format!("k{number:03}λ\0"), capacity);
            let old = tree.clone();
            let old_bytes = old.page_images().unwrap();
            let pointer = location(value);
            match operation {
                0 => {
                    let previous = expected.get(&key).copied();
                    let canonical = key.clone();
                    prop_assert_eq!(tree.insert(key, pointer), previous.map_or(Ok(()), |_| Err(Error::Duplicate)));
                    if previous.is_none() {expected.insert(canonical, pointer);}
                },
                1 => {
                    let previous = expected.get_mut(&key).map(|v| std::mem::replace(v, pointer));
                    prop_assert_eq!(tree.replace(&key, pointer), previous.ok_or(Error::NoKey));
                },
                2 => {prop_assert_eq!(tree.remove(&key), expected.remove(&key).ok_or(Error::NoKey));},
                _ => {prop_assert_eq!(tree.get(&key).unwrap(), expected.get(&key).copied());},
            }
            owners(&old, &tree);
            rows(&tree, &expected);
            prop_assert_eq!(old.page_images().unwrap(), old_bytes);
            if step % 11 == 0 {
                if retained.len() == 4 {retained.remove(0);}
                retained.push((tree.clone(), expected.clone(), tree.page_images().unwrap()));
            }
            if step % 13 == 0 {
                let imported = if stable {
                    BPlusTree::from_stable_pages(tree.root_id(), &tree.page_images().unwrap()).unwrap()
                } else {
                    BPlusTree::from_pages(tree.root_id(), &tree.page_images().unwrap()).unwrap()
                };
                prop_assert_eq!(&imported, &tree);
                arena(&imported);
                tree = imported;
            }
            for (snapshot, rows_expected, images) in &retained {
                rows(snapshot, rows_expected);
                prop_assert_eq!(snapshot.page_images().unwrap(), images.clone());
            }
        }
        drop(tree);
        for (snapshot, rows_expected, images) in retained {
            rows(&snapshot, &rows_expected);
            prop_assert_eq!(snapshot.page_images().unwrap(), images);
        }
    }
}
