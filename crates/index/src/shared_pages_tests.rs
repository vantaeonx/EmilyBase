use crate::{BPlusTree, Error, IndexSnapshot, Key, MAX_INDEX_ENTRIES, RecordPointer};
use proptest::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

fn pointer(number: u64) -> RecordPointer {
    RecordPointer {
        page_id: number + 1,
        slot_id: (number % 65536) as u16,
    }
}

fn tree(count: usize) -> BPlusTree {
    BPlusTree::from_sorted_stable(
        &(0..count)
            .map(|number| (Key::Integer(number as i64), pointer(number as u64)))
            .collect::<Vec<_>>(),
    )
    .unwrap()
}

// Value equality alone cannot detect a silent return to deep snapshot copies.
// Test the actual private owners while preserving the public owned-page API.
fn assert_shared_equal_pages(before: &BPlusTree, after: &BPlusTree) {
    for (id, old) in &before.pages {
        if let Some(new) = after.pages.get(id) {
            assert_eq!(Arc::ptr_eq(old, new), old == new, "page {id}");
        }
    }
}

fn assert_exact_owners(before: &BPlusTree, after: &BPlusTree) {
    assert_eq!(before, after);
    assert_eq!(before.pages.len(), after.pages.len());
    for (id, old) in &before.pages {
        assert!(Arc::ptr_eq(old, &after.pages[id]), "page {id}");
    }
}

fn assert_rows(value: &BPlusTree, expected: &BTreeMap<Key, RecordPointer>) {
    assert_eq!(value.validate().unwrap(), expected.len());
    assert_eq!(value.len(), expected.len());
    assert_eq!(
        value.range(None, None, MAX_INDEX_ENTRIES).unwrap(),
        expected
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect::<Vec<_>>()
    );
}

#[test]
fn cloning_full_text_arena_retains_page_and_borrowed_key_addresses() {
    let entries: Vec<_> = (0..10000)
        .map(|number| {
            (
                Key::Text(format!("{number:08}{}", "x".repeat(248))),
                pointer(number),
            )
        })
        .collect();
    let original = BPlusTree::from_sorted_stable(&entries).unwrap();
    assert_eq!(original.page_count(), 768);
    let cloned = original.clone();
    assert_exact_owners(&original, &cloned);
    for (a, b) in original
        .cursor(None, None)
        .unwrap()
        .zip(cloned.cursor(None, None).unwrap())
    {
        let (a, av) = a.unwrap();
        let (b, bv) = b.unwrap();
        assert!(std::ptr::eq(a, b));
        assert_eq!(av, bv);
        if let (Key::Text(a), Key::Text(b)) = (a, b) {
            assert_eq!(a.as_ptr(), b.as_ptr());
        } else {
            panic!("text fixture changed kind");
        }
    }
    let snapshot = IndexSnapshot {
        revision: 19,
        tree: cloned,
    };
    assert_exact_owners(&original, &snapshot.clone().tree);
    let decoded = IndexSnapshot::decode(&snapshot.encode().unwrap()).unwrap();
    assert_eq!(decoded, snapshot);
    for (id, old) in &original.pages {
        assert!(!Arc::ptr_eq(old, &decoded.tree.pages[id]));
    }
}

#[test]
fn pointer_replacement_detaches_one_leaf_and_preserves_the_old_view() {
    let base = IndexSnapshot {
        revision: 3,
        tree: tree(100),
    };
    let frozen = base.encode().unwrap();
    let key = Key::Integer(50);
    let leaf = base.tree.find_leaf(Some(&key)).unwrap();
    let mut changed = base.tree.clone();
    assert_eq!(changed.replace(&key, pointer(900)).unwrap(), pointer(50));
    for (id, page) in &base.tree.pages {
        assert_eq!(Arc::ptr_eq(page, &changed.pages[id]), *id != leaf);
    }
    assert_eq!(changed.get(&key).unwrap(), Some(pointer(900)));
    assert_eq!(base.tree.get(&key).unwrap(), Some(pointer(50)));
    let delta = base.delta_to(&changed).unwrap();
    assert_eq!(delta.upserts.len(), 1);
    let applied = delta.apply(&base).unwrap();
    assert_eq!(applied.tree, changed);
    assert_shared_equal_pages(&base.tree, &applied.tree);
    assert_eq!(base.encode().unwrap(), frozen);
}

#[test]
fn unchanged_pointer_and_failed_mutations_preserve_every_owner() {
    let mut value = tree(100);
    let before = value.clone();
    assert_eq!(
        value.replace(&Key::Integer(50), pointer(50)).unwrap(),
        pointer(50)
    );
    assert_exact_owners(&before, &value);
    assert_eq!(
        value.insert(Key::Integer(50), pointer(800)),
        Err(Error::Duplicate)
    );
    assert_exact_owners(&before, &value);
    assert_eq!(value.remove(&Key::Integer(800)), Err(Error::NoKey));
    assert_exact_owners(&before, &value);
    assert_eq!(
        value.replace(&Key::Integer(800), pointer(800)),
        Err(Error::NoKey)
    );
    assert_exact_owners(&before, &value);
    let zero = RecordPointer {
        page_id: 0,
        slot_id: 0,
    };
    assert_eq!(value.insert(Key::Integer(800), zero), Err(Error::PageId));
    assert_eq!(value.replace(&Key::Integer(50), zero), Err(Error::PageId));
    let long = Key::Text("x".repeat(257));
    assert_eq!(
        value.insert(long.clone(), pointer(800)),
        Err(Error::KeySize)
    );
    assert_eq!(value.remove(&long), Err(Error::KeySize));
    assert_eq!(value.replace(&long, pointer(800)), Err(Error::KeySize));
    assert_exact_owners(&before, &value);
}

#[test]
fn splits_rotations_merges_and_root_collapse_share_only_equal_survivors() {
    let mut value = BPlusTree::new_stable();
    let mut expected = BTreeMap::new();
    let mut splits = 0;
    for number in 0..200 {
        let old = value.clone();
        let key = Key::Integer(number);
        value.insert(key.clone(), pointer(number as u64)).unwrap();
        expected.insert(key, pointer(number as u64));
        splits += usize::from(value.page_count() > old.page_count());
        assert_shared_equal_pages(&old, &value);
        assert_eq!(old.len(), number as usize);
    }
    assert!(splits > 10);
    let retained = value.clone();
    let retained_images = retained.page_images().unwrap();
    let mut merges = 0;
    let mut collapse = false;
    for number in (0..100).flat_map(|n| [n, 199 - n]) {
        let old = value.clone();
        let key = Key::Integer(number);
        assert_eq!(value.remove(&key).unwrap(), expected.remove(&key).unwrap());
        merges += usize::from(value.page_count() < old.page_count());
        collapse |= value.root != old.root;
        assert_shared_equal_pages(&old, &value);
        assert_rows(&value, &expected);
        assert_eq!(retained.page_images().unwrap(), retained_images);
    }
    assert!(merges > 10);
    assert!(collapse);
    assert_eq!(value.page_count(), 1);
    assert!(value.is_empty());
    assert_eq!(retained.len(), 200);
}

#[test]
fn reused_arena_id_never_reuses_a_retained_retired_page() {
    let original = tree(60);
    let original_images = original.page_images().unwrap();
    let mut changed = original.clone();
    for number in 0..35 {
        changed.remove(&Key::Integer(number)).unwrap();
    }
    let retired: BTreeSet<_> = original
        .pages
        .keys()
        .filter(|id| !changed.pages.contains_key(id))
        .copied()
        .collect();
    assert!(!retired.is_empty());
    let deleted = changed.clone();
    for number in 100..180 {
        let old = changed.clone();
        changed
            .insert(Key::Integer(number), pointer(number as u64))
            .unwrap();
        assert_shared_equal_pages(&old, &changed);
    }
    let reused: Vec<_> = retired
        .iter()
        .filter(|id| changed.pages.contains_key(id))
        .collect();
    assert!(!reused.is_empty());
    for id in reused {
        assert!(!Arc::ptr_eq(&original.pages[id], &changed.pages[id]));
        assert_ne!(original.pages[id], changed.pages[id]);
        assert!(!deleted.pages.contains_key(id));
    }
    assert_eq!(original.page_images().unwrap(), original_images);
    assert_eq!(original.validate().unwrap(), 60);
    assert_eq!(deleted.validate().unwrap(), 25);
    assert_eq!(changed.validate().unwrap(), 105);
}

#[test]
fn last_historical_owner_releases_replaced_and_retired_pages() {
    let original = tree(60);
    let key = Key::Integer(10);
    let leaf = original.find_leaf(Some(&key)).unwrap();
    let weak = Arc::downgrade(&original.pages[&leaf]);
    let mut changed = original.clone();
    changed.replace(&key, pointer(800)).unwrap();
    assert!(weak.upgrade().is_some());
    let historical = original.clone();
    drop(original);
    assert!(weak.upgrade().is_some());
    drop(historical);
    assert!(weak.upgrade().is_none());
    let original = tree(60);
    let mut changed = original.clone();
    let mut pages = Vec::new();
    for number in 0..35 {
        changed.remove(&Key::Integer(number)).unwrap();
    }
    for (id, page) in &original.pages {
        if !changed.pages.contains_key(id) {
            pages.push(Arc::downgrade(page));
        }
    }
    assert!(!pages.is_empty());
    assert!(pages.iter().all(|page| page.upgrade().is_some()));
    drop(original);
    assert!(pages.iter().all(|page| page.upgrade().is_none()));
    assert_eq!(changed.validate().unwrap(), 25);
}

#[test]
fn dense_deletion_remaps_owned_links_without_modifying_shared_history() {
    let entries: Vec<_> = (0..120)
        .map(|n| (Key::Integer(n), pointer(n as u64)))
        .collect();
    let original = BPlusTree::from_sorted(&entries).unwrap();
    let frozen = original.page_images().unwrap();
    let mut changed = original.clone();
    for number in 0..90 {
        changed.remove(&Key::Integer(number)).unwrap();
        let images = changed.page_images().unwrap();
        assert_eq!(
            BPlusTree::from_pages(changed.root_id(), &images).unwrap(),
            changed
        );
        assert_eq!(original.page_images().unwrap(), frozen);
    }
    assert_eq!(original.len(), 120);
    assert_eq!(changed.len(), 30);
}

#[test]
fn independent_thread_writers_preserve_shared_reader_and_each_other() {
    let original = tree(200);
    let frozen = original.page_images().unwrap();
    let gate = std::sync::Barrier::new(5);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|worker| {
                let gate = &gate;
                let reader = &original;
                scope.spawn(move || {
                    let mut writer = reader.clone();
                    gate.wait();
                    for number in 0..80 {
                        let key = Key::Integer(number);
                        writer
                            .replace(&key, pointer(1000 + worker * 100 + number as u64))
                            .unwrap();
                        assert_eq!(reader.get(&key).unwrap(), Some(pointer(number as u64)));
                    }
                    assert_shared_equal_pages(reader, &writer);
                    assert_eq!(writer.validate().unwrap(), 200);
                    writer
                })
            })
            .collect();
        gate.wait();
        let writers: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
        for a in 0..writers.len() {
            for b in a + 1..writers.len() {
                assert_ne!(
                    writers[a].get(&Key::Integer(10)).unwrap(),
                    writers[b].get(&Key::Integer(10)).unwrap()
                );
                let untouched = original.find_leaf(Some(&Key::Integer(190))).unwrap();
                assert!(Arc::ptr_eq(
                    &writers[a].pages[&untouched],
                    &writers[b].pages[&untouched]
                ));
            }
        }
    });
    assert_eq!(original.page_images().unwrap(), frozen);
}

#[test]
fn exhausted_stable_arena_refuses_partial_splits_without_detaching_any_page() {
    let mut value = BPlusTree::new_stable();
    let mut refusal = false;
    for number in 0..=MAX_INDEX_ENTRIES {
        let old = value.clone();
        match value.insert(Key::Integer(number as i64), pointer(number as u64)) {
            Ok(()) => assert_shared_equal_pages(&old, &value),
            Err(Error::Limit) => {
                refusal = true;
                assert_exact_owners(&old, &value);
                assert_eq!(value.validate().unwrap(), number);
                break;
            }
            other => panic!("unexpected insertion: {other:?}"),
        }
    }
    assert!(refusal);
    assert_eq!(value.page_count(), crate::MAX_INDEX_PAGES);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn accepted_discarded_and_replayed_generations_preserve_independent_rows(
        steps in prop::collection::vec((0u8..4, -80i64..80, any::<u16>(), any::<bool>()), 1..140)
    ) {
        let mut current = IndexSnapshot { revision: 1, tree: BPlusTree::new_stable() };
        let mut expected = BTreeMap::new();
        let mut historical = Vec::new();
        for (kind, number, slot, accept) in steps {
            let old = current.clone();
            let mut staged = old.tree.clone();
            let mut wanted = expected.clone();
            let key = if number % 2 == 0 { Key::Integer(number) }
                else { Key::Text(format!("{number:+04}\0{}", "я".repeat(125))) };
            let value = RecordPointer { page_id: u64::from(slot) + 1, slot_id: slot };
            let result = match kind {
                0 => staged.insert(key.clone(), value).map(|()| { wanted.insert(key.clone(), value); }),
                1 => staged.replace(&key, value).map(|_| { wanted.insert(key.clone(), value); }),
                2 => staged.remove(&key).map(|_| { wanted.remove(&key); }),
                _ => { prop_assert_eq!(staged.get(&key).unwrap(), wanted.get(&key).copied()); Ok(()) }
            };
            assert_shared_equal_pages(&old.tree, &staged);
            if result.is_err() {
                assert_exact_owners(&old.tree, &staged);
            }
            if result.is_ok() && accept {
                let delta = old.delta_to(&staged).unwrap();
                current = delta.apply(&old).unwrap();
                assert_shared_equal_pages(&old.tree, &current.tree);
                prop_assert_eq!(&current.tree, &staged);
                expected = wanted;
            }
            assert_rows(&current.tree, &expected);
            if historical.len() < 8 {
                historical.push((old.clone(), old.encode().unwrap()));
            }
            for (retained, encoded) in &historical {
                prop_assert_eq!(retained.encode().unwrap(), encoded.clone());
                prop_assert_eq!(IndexSnapshot::decode(encoded).unwrap(), retained.clone());
            }
        }
    }
}
