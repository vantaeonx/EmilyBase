use emilybase_index::{BPlusTree, Error, IndexSnapshot, Key, RecordPointer, SnapshotDelta};
use proptest::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn pointer(number: i64) -> RecordPointer {
    RecordPointer {
        page_id: number.unsigned_abs() + 1,
        slot_id: number as u16,
    }
}
fn assert_state(snapshot: &IndexSnapshot, expected: &BTreeMap<Key, RecordPointer>) {
    snapshot.validate().unwrap();
    assert_eq!(snapshot.tree.len(), expected.len());
    let entries: Vec<_> = expected
        .iter()
        .map(|(key, value)| (key.clone(), *value))
        .collect();
    assert_eq!(snapshot.tree.range(None, None, 10000).unwrap(), entries);
    let encoded = snapshot.encode().unwrap();
    assert_eq!(
        snapshot.fingerprint().unwrap(),
        <[u8; 32]>::from(Sha256::digest(&encoded))
    );
    let decoded = IndexSnapshot::decode(&encoded).unwrap();
    assert_eq!(decoded, snapshot.clone());
}

#[test]
fn sparse_holes_root_collapse_and_reuse_keep_exact_delta_predecessors() {
    let entries: Vec<_> = (0..300)
        .map(|key| (Key::Integer(key), pointer(key)))
        .collect();
    let base = IndexSnapshot {
        revision: 1,
        tree: BPlusTree::from_sorted_stable(&entries).unwrap(),
    };
    let mut target = base.tree.clone();
    for key in 0..290 {
        target.remove(&Key::Integer(key)).unwrap();
    }
    let delta = base.delta_to(&target).unwrap();
    assert!(!delta.retired.is_empty());
    let next = delta.apply(&base).unwrap();
    assert_eq!(next.tree, target);
    assert_eq!(base.tree.len(), 300);
    assert_eq!(next.tree.page_count(), 1);
    let mut reused = next.tree.clone();
    for key in 0..120 {
        reused.insert(Key::Integer(-key - 1), pointer(key)).unwrap();
    }
    let second = next.delta_to(&reused).unwrap();
    let selected = second.apply(&next).unwrap();
    assert_eq!(selected.tree, reused);
    assert!(second.apply(&base).is_err());
    assert!(delta.apply(&selected).is_err());
    let expected: BTreeMap<_, _> = (290..300)
        .map(|key| (Key::Integer(key), pointer(key)))
        .chain((0..120).map(|key| (Key::Integer(-key - 1), pointer(key))))
        .collect();
    assert_state(&selected, &expected);
}

#[test]
fn malformed_complete_candidate_is_refused_without_changing_the_base() {
    let entries: Vec<_> = (0..225)
        .map(|key| (Key::Integer(key), pointer(key)))
        .collect();
    let base = IndexSnapshot {
        revision: 5,
        tree: BPlusTree::from_sorted_stable(&entries).unwrap(),
    };
    let before = base.encode().unwrap();
    let original = base.delta_to(&base.tree).unwrap();
    for (root, entries) in [
        (0, 225),
        (u64::MAX, 225),
        (base.tree.root_id(), 224),
        (base.tree.root_id(), usize::MAX),
    ] {
        let mut wrong = original.clone();
        wrong.root = root;
        wrong.entries = entries;
        assert!(wrong.apply(&base).is_err());
        assert_eq!(base.encode().unwrap(), before);
    }
    let mut wrong = original.clone();
    wrong.upserts.push(base.tree.page_images().unwrap()[0]);
    assert_eq!(
        wrong.apply(&base).unwrap_err(),
        Error::Layout("delta unchanged page")
    );
    let mut wrong = original;
    wrong.base_fingerprint[0] ^= 1;
    assert_eq!(
        wrong.apply(&base).unwrap_err(),
        Error::Layout("delta base fingerprint")
    );
    assert_eq!(base.encode().unwrap(), before);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn accepted_and_discarded_complete_deltas_match_independent_mixed_key_rows(
        actions in prop::collection::vec((0u8..3,-100i64..100,any::<bool>(),any::<bool>()),1..48)
    ) {
        let mut base = IndexSnapshot { revision: 1, tree: BPlusTree::new_stable() };
        let mut expected = BTreeMap::new();
        for (operation,number,text,publish) in actions {
            let key = if text { Key::Text(format!("λ{number}\0")) } else { Key::Integer(number) };
            let before = base.encode().unwrap();
            let mut target = base.tree.clone();
            let value = RecordPointer { page_id: number.unsigned_abs()+1+base.revision*1024, slot_id: base.revision as u16 };
            let succeeds = match operation { 0 => !expected.contains_key(&key), _ => expected.contains_key(&key) };
            let result = match operation {
                0 => target.insert(key.clone(),value),
                1 => target.replace(&key,value).map(|_|()),
                _ => target.remove(&key).map(|_|()),
            };
            prop_assert_eq!(result.is_ok(),succeeds);
            if !succeeds { prop_assert_eq!(base.encode().unwrap(),before); continue; }
            let delta = base.delta_to(&target).unwrap();
            let output = delta.apply(&base).unwrap();
            let mut candidate = expected.clone();
            if operation == 2 { candidate.remove(&key); } else { candidate.insert(key,value); }
            assert_state(&output,&candidate);
            prop_assert_eq!(&output.tree,&target);
            prop_assert_eq!(base.encode().unwrap(),before);
            if publish { base=output;expected=candidate; }
            assert_state(&base,&expected);
        }
    }
}

#[test]
fn unchanged_full_capacity_snapshot_delta_keeps_all_keys_and_frozen_bytes() {
    let expected: BTreeMap<_, _> = (0..10000)
        .map(|key| (Key::Integer(key), pointer(key)))
        .collect();
    let entries: Vec<_> = expected
        .iter()
        .map(|(key, value)| (key.clone(), *value))
        .collect();
    let base = IndexSnapshot {
        revision: u64::MAX - 1,
        tree: BPlusTree::from_sorted_stable(&entries).unwrap(),
    };
    let before = base.encode().unwrap();
    let delta = base.delta_to(&base.tree).unwrap();
    assert!(delta.upserts.is_empty());
    assert!(delta.retired.is_empty());
    let next = delta.apply(&base).unwrap();
    assert_eq!(next.revision, u64::MAX);
    assert_state(&next, &expected);
    assert_eq!(base.encode().unwrap(), before);
    assert_eq!(next.delta_to(&next.tree).unwrap_err(), Error::Limit);
    let wrong = SnapshotDelta {
        revision: 0,
        ..delta
    };
    assert!(wrong.apply(&base).is_err());
}

#[test]
fn manually_packed_1024_page_arena_preserves_wire_domain_and_one_page_delta() {
    use emilybase_index::{IndexPage, MAX_INDEX_PAGES, MAX_KEYS, PAGE_SIZE};
    // 954 seven-entry leaves +64 lower branches +5 upper branches +root.
    // This independent shape reaches exactly the accepted arena boundary.
    let leaves = 954;
    let mut images = Vec::<[u8; PAGE_SIZE]>::new();
    let mut layer = Vec::<(u64, Key)>::new();
    let mut expected = BTreeMap::new();
    for leaf in 0..leaves {
        let id = leaf as u64 + 1;
        let entries: Vec<_> = (0..7)
            .map(|slot| {
                let number = leaf * 7 + slot;
                let key = Key::Integer(number as i64);
                let value = RecordPointer {
                    page_id: u64::MAX,
                    slot_id: number as u16,
                };
                expected.insert(key.clone(), value);
                (key, value)
            })
            .collect();
        let next = (leaf + 1 < leaves).then_some(id + 1);
        layer.push((id, entries[0].0.clone()));
        images.push(
            IndexPage::leaf(id, entries, next)
                .unwrap()
                .encode()
                .unwrap(),
        );
    }
    while layer.len() > 1 {
        let groups = layer.len().div_ceil(MAX_KEYS + 1);
        let width = layer.len() / groups;
        let extra = layer.len() % groups;
        let mut next = Vec::new();
        let mut start = 0;
        for group in 0..groups {
            let size = width + usize::from(group < extra);
            let children = &layer[start..start + size];
            let id = images.len() as u64 + 1;
            next.push((id, children[0].1.clone()));
            images.push(
                IndexPage::branch(
                    id,
                    children[1..].iter().map(|(_, key)| key.clone()).collect(),
                    children.iter().map(|(id, _)| *id).collect(),
                )
                .unwrap()
                .encode()
                .unwrap(),
            );
            start += size;
        }
        assert_eq!(start, layer.len());
        layer = next;
    }
    assert_eq!(images.len(), MAX_INDEX_PAGES);
    assert_eq!(expected.len(), 6678);
    let base = IndexSnapshot {
        revision: 19,
        tree: BPlusTree::from_stable_pages(layer[0].0, &images).unwrap(),
    };
    assert_state(&base, &expected);
    let original = base.encode().unwrap();
    assert_eq!(original.len(), (MAX_INDEX_PAGES + 1) * PAGE_SIZE);
    let mut target = base.tree.clone();
    let key = Key::Integer(5000);
    let replacement = RecordPointer {
        page_id: 17,
        slot_id: u16::MAX,
    };
    target.replace(&key, replacement).unwrap();
    let delta = base.delta_to(&target).unwrap();
    assert_eq!(delta.upserts.len(), 1);
    assert!(delta.retired.is_empty());
    let selected = delta.apply(&base).unwrap();
    expected.insert(key, replacement);
    assert_state(&selected, &expected);
    assert_eq!(selected.tree.page_count(), 1024);
    assert_eq!(base.encode().unwrap(), original);
}
