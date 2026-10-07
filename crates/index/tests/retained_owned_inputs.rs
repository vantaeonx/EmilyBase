//! Public ownership tests inspect borrowed keys before serialization can rebuild
//! their allocation. Standalone index persistence remains separate from table WAL.
use emilybase_index::{BPlusTree, Error, IndexPage, IndexSnapshot, IndexStore, Key, RecordPointer};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn input(number: usize, capacity: usize) -> Key {
    let mut value = String::with_capacity(capacity);
    value.push_str(&format!("{number:04}я\0"));
    Key::Text(value)
}
fn pointer(number: usize) -> RecordPointer {
    RecordPointer {
        page_id: number as u64 + 1,
        slot_id: number as u16,
    }
}
fn check(snapshot: &IndexSnapshot, expected: &BTreeMap<Key, RecordPointer>) {
    snapshot.validate().unwrap();
    let actual: Vec<_> = snapshot
        .tree
        .cursor(None, None)
        .unwrap()
        .map(|entry| {
            let (key, value) = entry.unwrap();
            if let Key::Text(text) = key {
                assert_eq!(text.capacity(), text.len());
            }
            (key.clone(), value)
        })
        .collect();
    assert_eq!(
        actual,
        expected
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect::<Vec<_>>()
    );
    let bytes = snapshot.encode().unwrap();
    let hash: [u8; 32] = Sha256::digest(&bytes).into();
    assert_eq!(snapshot.fingerprint().unwrap(), hash);
    assert_eq!(IndexSnapshot::decode(&bytes).unwrap(), *snapshot);
}

#[test]
fn equivalent_owned_input_capacities_produce_identical_pages_and_fingerprints() {
    for stable in [false, true] {
        let mut compact = if stable {
            BPlusTree::new_stable()
        } else {
            BPlusTree::new()
        };
        let mut padded = compact.clone();
        let mut expected = BTreeMap::new();
        for number in (0..96).rev() {
            compact.insert(input(number, 0), pointer(number)).unwrap();
            padded
                .insert(input(number, 128 * 1024), pointer(number))
                .unwrap();
            expected.insert(input(number, 0), pointer(number));
            assert_eq!(
                compact.page_images().unwrap(),
                padded.page_images().unwrap()
            );
            check(
                &IndexSnapshot {
                    revision: 1,
                    tree: padded.to_stable().unwrap(),
                },
                &expected,
            );
        }
        let old = IndexSnapshot {
            revision: 1,
            tree: padded.to_stable().unwrap(),
        };
        let old_bytes = old.encode().unwrap();
        for number in 0..64 {
            if number % 2 == 0 {
                assert_eq!(
                    padded.remove(&input(number, 128 * 1024)).unwrap(),
                    expected.remove(&input(number, 0)).unwrap()
                );
                compact.remove(&input(number, 0)).unwrap();
            } else {
                padded
                    .replace(&input(number, 128 * 1024), pointer(200 + number))
                    .unwrap();
                compact
                    .replace(&input(number, 0), pointer(200 + number))
                    .unwrap();
                expected.insert(input(number, 0), pointer(200 + number));
            }
            assert_eq!(
                compact.page_images().unwrap(),
                padded.page_images().unwrap()
            );
            check(
                &IndexSnapshot {
                    revision: 2,
                    tree: padded.to_stable().unwrap(),
                },
                &expected,
            );
            assert_eq!(old.encode().unwrap(), old_bytes);
        }
        let delta = old.delta_to(&padded.to_stable().unwrap()).unwrap();
        let replayed = delta.apply(&old).unwrap();
        check(&replayed, &expected);
        assert_eq!(replayed.tree, padded.to_stable().unwrap());
        assert_eq!(old.encode().unwrap(), old_bytes);
    }
}

#[test]
fn constructors_and_untrusted_decode_preserve_original_text_without_normalization() {
    let mut incoming = Vec::with_capacity(1024);
    let texts = ["", "é", "é", "я\0", "🌌"];
    let mut expected = Vec::new();
    for (number, text) in texts.into_iter().enumerate() {
        let mut value = String::with_capacity(128 * 1024);
        value.push_str(text);
        expected.push((Key::Text(text.to_owned()), pointer(number)));
        incoming.push((Key::Text(value), pointer(number)));
    }
    incoming.sort_by(|a, b| a.0.cmp(&b.0));
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    let page = IndexPage::leaf(1, incoming, None).unwrap();
    for key in page.keys() {
        if let Key::Text(text) = key {
            assert_eq!(text.capacity(), text.len());
        }
    }
    assert_eq!(
        page.keys(),
        expected
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>()
    );
    let image = page.encode().unwrap();
    let decoded = IndexPage::decode(&image, 1).unwrap();
    assert_eq!(decoded, page);
    let tree = BPlusTree::from_stable_pages(1, &[image]).unwrap();
    let snapshot = IndexSnapshot { revision: 1, tree };
    check(&snapshot, &expected.into_iter().collect());
    assert_eq!(
        snapshot.tree.get(&Key::Text("é".into())).unwrap(),
        Some(pointer(1))
    );
    assert_eq!(
        snapshot.tree.get(&Key::Text("é".into())).unwrap(),
        Some(pointer(2))
    );
}

#[test]
fn standalone_publication_reopens_compact_payloads_and_preserves_old_readers() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("synthetic-index");
    let mut tree = BPlusTree::new_stable();
    let mut expected = BTreeMap::new();
    for number in 0..64 {
        tree.insert(input(number, 128 * 1024), pointer(number))
            .unwrap();
        expected.insert(input(number, 0), pointer(number));
    }
    let mut store = IndexStore::create(&root, &tree).unwrap();
    let initial = store.snapshot().unwrap().clone();
    let initial_bytes = initial.encode().unwrap();
    check(&initial, &expected);
    for number in 0..24 {
        let prior = store.snapshot().unwrap().clone();
        let prior_bytes = prior.encode().unwrap();
        let mut changed = prior.tree.clone();
        changed.remove(&input(number, 128 * 1024)).unwrap();
        expected.remove(&input(number, 0));
        changed
            .insert(input(100 + number, 128 * 1024), pointer(100 + number))
            .unwrap();
        expected.insert(input(100 + number, 0), pointer(100 + number));
        if number % 2 == 0 {
            assert_eq!(store.replace(&changed).unwrap(), number as u64 + 2);
        } else {
            let delta = prior.delta_to(&changed).unwrap();
            assert_eq!(store.apply(&delta).unwrap(), number as u64 + 2);
        }
        check(store.snapshot().unwrap(), &expected);
        let bytes = store.snapshot().unwrap().encode().unwrap();
        assert_eq!(std::fs::read(root.join("tree.ebif")).unwrap(), bytes);
        assert_eq!(prior.encode().unwrap(), prior_bytes);
        assert_eq!(initial.encode().unwrap(), initial_bytes);
        drop(store);
        store = IndexStore::open(&root).unwrap();
        assert_eq!(store.snapshot().unwrap().tree, changed);
        check(store.snapshot().unwrap(), &expected);
        assert_eq!(store.snapshot().unwrap().encode().unwrap(), bytes);
    }
}

#[test]
fn invalid_owned_keys_preserve_borrowed_key_addresses_and_exact_state() {
    let mut tree = BPlusTree::new_stable();
    tree.insert(input(1, 128 * 1024), pointer(1)).unwrap();
    let old = tree.clone();
    let bytes = tree.page_images().unwrap();
    let first = tree.cursor(None, None).unwrap().next().unwrap().unwrap().0 as *const Key;
    assert_eq!(
        tree.insert(input(1, 128 * 1024), pointer(2)),
        Err(Error::Duplicate)
    );
    let mut too_long = String::with_capacity(128 * 1024);
    too_long.push_str(&"λ".repeat(129));
    assert_eq!(
        tree.insert(Key::Text(too_long), pointer(2)),
        Err(Error::KeySize)
    );
    assert_eq!(
        tree.insert(
            input(2, 128 * 1024),
            RecordPointer {
                page_id: 0,
                slot_id: 0
            }
        ),
        Err(Error::PageId)
    );
    assert_eq!(tree.remove(&input(2, 128 * 1024)), Err(Error::NoKey));
    assert_eq!(tree.page_images().unwrap(), bytes);
    assert_eq!(
        tree.cursor(None, None).unwrap().next().unwrap().unwrap().0 as *const Key,
        first
    );
    assert_eq!(old.page_images().unwrap(), bytes);
    assert_eq!(
        old.cursor(None, None).unwrap().next().unwrap().unwrap().0 as *const Key,
        first
    );
}
