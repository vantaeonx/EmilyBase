use std::collections::BTreeMap;

use emilybase_index::*;
use proptest::prelude::*;

fn pointer(key: i64) -> RecordPointer {
    RecordPointer {
        page_id: key.unsigned_abs() + 1,
        slot_id: key as u16,
    }
}
fn entries(count: usize) -> Vec<(Key, RecordPointer)> {
    (0..count as i64)
        .map(|key| (Key::Integer(key), pointer(key)))
        .collect()
}
fn images(tree: &BPlusTree) -> BTreeMap<u64, [u8; PAGE_SIZE]> {
    tree.page_images()
        .unwrap()
        .into_iter()
        .map(|image| (u64::from_le_bytes(image[8..16].try_into().unwrap()), image))
        .collect()
}
fn reopen(tree: &BPlusTree) -> BPlusTree {
    let restored =
        BPlusTree::from_stable_pages(tree.root_id(), &tree.page_images().unwrap()).unwrap();
    assert_eq!(&restored, tree);
    restored
}
fn repair_header(bytes: &mut [u8]) {
    let mut crc = crc32fast::Hasher::new();
    crc.update(&bytes[..60]);
    crc.update(&bytes[64..PAGE_SIZE]);
    bytes[60..64].copy_from_slice(&crc.finalize().to_le_bytes());
}
fn snapshot(count: usize) -> IndexSnapshot {
    IndexSnapshot {
        revision: 1,
        tree: BPlusTree::from_sorted_stable(&entries(count)).unwrap(),
    }
}

#[test]
fn merged_leaf_keeps_other_ids_and_insertion_reuses_the_hole_without_overwriting_root() {
    let mut tree = BPlusTree::from_sorted_stable(&entries(30)).unwrap();
    assert_eq!(tree.root_id(), 4);
    let first_image = images(&tree)[&1];
    for key in 20..27 {
        tree.remove(&Key::Integer(key)).unwrap();
    }
    assert_eq!(tree.root_id(), 4);
    assert_eq!(
        images(&tree).keys().copied().collect::<Vec<_>>(),
        vec![1, 2, 4]
    );
    assert_eq!(images(&tree)[&1], first_image);
    for key in 40..56 {
        tree.insert(Key::Integer(key), pointer(key)).unwrap();
    }
    assert!(images(&tree).contains_key(&3));
    assert_eq!(tree.root_id(), 4);
    for key in (0..20).chain(27..30).chain(40..56) {
        assert_eq!(tree.get(&Key::Integer(key)).unwrap(), Some(pointer(key)));
    }
    reopen(&tree);
}

#[test]
fn sparse_root_collapse_and_empty_tree_preserve_the_surviving_leaf() {
    let left = IndexPage::leaf(7, entries(7), Some(100)).unwrap();
    let right = IndexPage::leaf(
        100,
        (20..27)
            .map(|key| (Key::Integer(key), pointer(key)))
            .collect(),
        None,
    )
    .unwrap();
    let root = IndexPage::branch(900, vec![Key::Integer(20)], vec![7, 100]).unwrap();
    let mut tree = BPlusTree::from_stable_pages(
        900,
        &[
            left.encode().unwrap(),
            right.encode().unwrap(),
            root.encode().unwrap(),
        ],
    )
    .unwrap();
    tree.remove(&Key::Integer(20)).unwrap();
    assert_eq!(tree.root_id(), 7);
    assert_eq!(tree.page_count(), 1);
    for key in (0..7).chain(21..27) {
        tree.remove(&Key::Integer(key)).unwrap();
    }
    assert!(tree.is_empty());
    assert_eq!(tree.root_id(), 7);
    reopen(&tree);
    for key in 50..80 {
        tree.insert(Key::Integer(key), pointer(key)).unwrap();
    }
    assert!(images(&tree).contains_key(&7));
    assert_eq!(tree.validate().unwrap(), 30);
    reopen(&tree);
}

#[test]
fn dense_contract_and_frozen_page_bytes_remain_separate_from_sparse_mode() {
    let source = entries(225);
    let dense = BPlusTree::from_sorted(&source).unwrap();
    let stable = BPlusTree::from_sorted_stable(&source).unwrap();
    assert!(!dense.has_stable_ids());
    assert!(stable.has_stable_ids());
    assert_eq!(dense.page_images().unwrap(), stable.page_images().unwrap());
    assert_eq!(dense.root_id(), stable.root_id());
    assert_eq!(
        BPlusTree::from_pages(dense.root_id(), &dense.page_images().unwrap()).unwrap(),
        dense
    );
    let mut sparse = stable;
    for key in 100..120 {
        sparse.remove(&Key::Integer(key)).unwrap();
    }
    assert!(BPlusTree::from_pages(sparse.root_id(), &sparse.page_images().unwrap()).is_err());
    reopen(&sparse);
}

#[test]
fn sparse_import_rejects_duplicates_reordering_bad_domains_and_missing_targets() {
    let root = IndexPage::leaf(100, entries(7), None)
        .unwrap()
        .encode()
        .unwrap();
    assert_eq!(
        BPlusTree::from_stable_pages(100, &[root, root]),
        Err(Error::PageId)
    );
    assert!(BPlusTree::from_stable_pages(1, &[root]).is_err());
    let outside = IndexPage::leaf(MAX_INDEX_PAGES as u64 + 1, entries(7), None)
        .unwrap()
        .encode()
        .unwrap();
    assert_eq!(
        BPlusTree::from_stable_pages(MAX_INDEX_PAGES as u64 + 1, &[outside]),
        Err(Error::PageId)
    );
    let three = BPlusTree::from_sorted_stable(&entries(30)).unwrap();
    let mut pages = three.page_images().unwrap();
    pages.swap(0, 1);
    assert_eq!(
        BPlusTree::from_stable_pages(three.root_id(), &pages),
        Err(Error::PageId)
    );
    assert!(BPlusTree::from_stable_pages(1, &[]).is_err());
    assert!(BPlusTree::from_stable_pages(1, &vec![root; MAX_INDEX_PAGES + 1]).is_err());
}

#[test]
fn sparse_bounds_and_failed_mutations_preserve_exact_images() {
    let mut tree = BPlusTree::new_stable();
    let key = Key::Text("я".repeat(128));
    tree.insert(key.clone(), pointer(1)).unwrap();
    let before = tree.clone();
    assert_eq!(tree.insert(key.clone(), pointer(2)), Err(Error::Duplicate));
    assert_eq!(
        tree.insert(Key::Text("я".repeat(129)), pointer(3)),
        Err(Error::KeySize)
    );
    assert_eq!(tree.remove(&Key::Integer(3)), Err(Error::NoKey));
    assert_eq!(
        tree.replace(
            &key,
            RecordPointer {
                page_id: 0,
                slot_id: 0
            }
        ),
        Err(Error::PageId)
    );
    assert_eq!(tree, before);
    let mut full = BPlusTree::from_sorted_stable(&entries(MAX_INDEX_ENTRIES)).unwrap();
    let before = full.clone();
    assert_eq!(
        full.insert(Key::Integer(20_000), pointer(20_000)),
        Err(Error::Limit)
    );
    assert_eq!(full, before);
    full.remove(&Key::Integer(100)).unwrap();
    full.insert(Key::Integer(20_000), pointer(20_000)).unwrap();
    reopen(&full);
}

#[test]
fn snapshot_round_trips_empty_sparse_multilevel_and_maximum_trees() {
    for count in [0, 1, 14, 15, 225, MAX_INDEX_ENTRIES] {
        let mut value = snapshot(count);
        if count >= 225 {
            for key in 40..70 {
                value.tree.remove(&Key::Integer(key)).unwrap();
            }
        }
        value.revision = u64::MAX;
        let bytes = value.encode().unwrap();
        assert_eq!(bytes.len(), (value.tree.page_count() + 1) * PAGE_SIZE);
        assert_eq!(&bytes[..8], b"EBIF\0\0\0\0");
        let restored = IndexSnapshot::decode(&bytes).unwrap();
        assert_eq!(restored, value);
        assert_eq!(restored.encode().unwrap(), bytes);
        assert_eq!(
            restored.fingerprint().unwrap(),
            value.fingerprint().unwrap()
        );
    }
}

#[test]
fn every_small_snapshot_cut_and_single_byte_damage_fails() {
    let bytes = snapshot(14).encode().unwrap();
    for end in 0..bytes.len() {
        assert!(IndexSnapshot::decode(&bytes[..end]).is_err(), "cut {end}");
    }
    for offset in 0..bytes.len() {
        let mut damaged = bytes.clone();
        damaged[offset] ^= 0x80;
        assert!(
            IndexSnapshot::decode(&damaged).is_err(),
            "mutation {offset}"
        );
    }
    let mut longer = bytes;
    longer.push(0);
    assert!(IndexSnapshot::decode(&longer).is_err());
    assert!(IndexSnapshot::decode(&vec![0; MAX_SNAPSHOT_BYTES + PAGE_SIZE]).is_err());
}

#[test]
fn repaired_header_checksums_cannot_hide_invalid_format_or_counts() {
    let base = snapshot(30).encode().unwrap();
    for (offset, value) in [
        (10, 1),
        (12, 1),
        (16, 0),
        (24, 0),
        (32, 0),
        (36, 1),
        (40, 0),
        (48, 1),
        (64, 1),
        (PAGE_SIZE - 1, 1),
    ] {
        let mut bytes = base.clone();
        bytes[offset] = value;
        repair_header(&mut bytes);
        assert!(
            IndexSnapshot::decode(&bytes).is_err(),
            "header offset {offset}"
        );
    }
    let mut future = base.clone();
    future[8..10].copy_from_slice(&2u16.to_le_bytes());
    repair_header(&mut future);
    assert_eq!(IndexSnapshot::decode(&future), Err(Error::Version(2)));
    let mut count = base.clone();
    count[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
    repair_header(&mut count);
    assert!(IndexSnapshot::decode(&count).is_err());
    let mut entries = base;
    entries[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
    repair_header(&mut entries);
    assert!(IndexSnapshot::decode(&entries).is_err());
    assert!(
        IndexSnapshot {
            revision: 0,
            tree: BPlusTree::new_stable()
        }
        .encode()
        .is_err()
    );
    assert!(
        IndexSnapshot {
            revision: 1,
            tree: BPlusTree::new()
        }
        .encode()
        .is_err()
    );
}

#[test]
fn repaired_page_checksum_still_requires_complete_topology_and_header_entry_count() {
    let value = snapshot(30);
    let base = value.encode().unwrap();
    let root_position = value
        .tree
        .page_images()
        .unwrap()
        .iter()
        .position(|image| {
            u64::from_le_bytes(image[8..16].try_into().unwrap()) == value.tree.root_id()
        })
        .unwrap();
    let start = (root_position + 1) * PAGE_SIZE;
    let mut bytes = base.clone();
    bytes[start + 32..start + 40].copy_from_slice(&1000u64.to_le_bytes());
    let crc = {
        let mut hash = crc32fast::Hasher::new();
        hash.update(&bytes[start..start + 60]);
        hash.update(&bytes[start + 64..start + PAGE_SIZE]);
        hash.finalize()
    };
    bytes[start + 60..start + 64].copy_from_slice(&crc.to_le_bytes());
    assert!(IndexSnapshot::decode(&bytes).is_err());
    let mut wrong_count = base;
    wrong_count[40..48].copy_from_slice(&29u64.to_le_bytes());
    repair_header(&mut wrong_count);
    assert_eq!(
        IndexSnapshot::decode(&wrong_count),
        Err(Error::Layout("snapshot entry count"))
    );
}

#[test]
fn delta_changes_only_one_replacement_image_and_is_bound_to_exact_base() {
    let base = snapshot(300);
    let original = base.clone();
    let mut tree = base.tree.clone();
    tree.replace(&Key::Integer(117), pointer(900)).unwrap();
    let delta = base.delta_to(&tree).unwrap();
    assert_eq!(delta.upserts.len(), 1);
    assert!(delta.retired.is_empty());
    assert_eq!(delta.revision, 2);
    assert_eq!(delta.root, base.tree.root_id());
    let changed = delta.apply(&base).unwrap();
    assert_eq!(changed.tree, tree);
    assert_eq!(base, original);
    assert!(delta.apply(&changed).is_err());
    let mut other = base.clone();
    other.tree.replace(&Key::Integer(9), pointer(901)).unwrap();
    assert_eq!(
        delta.apply(&other),
        Err(Error::Layout("delta base fingerprint"))
    );
    let mut wrong = delta;
    wrong.base_fingerprint[0] ^= 1;
    assert!(wrong.apply(&base).is_err());
}

#[test]
fn delta_root_split_merge_retirement_and_hole_reuse_match_whole_snapshots() {
    let mut current = IndexSnapshot {
        revision: 1,
        tree: BPlusTree::new_stable(),
    };
    for key in 0..225 {
        let mut tree = current.tree.clone();
        tree.insert(Key::Integer(key), pointer(key)).unwrap();
        let delta = current.delta_to(&tree).unwrap();
        current = delta.apply(&current).unwrap();
        assert_eq!(current.tree, tree);
    }
    let mut retired = 0;
    for key in 0..225 {
        let mut tree = current.tree.clone();
        tree.remove(&Key::Integer(key)).unwrap();
        let delta = current.delta_to(&tree).unwrap();
        retired += delta.retired.len();
        current = delta.apply(&current).unwrap();
        assert_eq!(current.tree, tree);
        assert_eq!(
            IndexSnapshot::decode(&current.encode().unwrap()).unwrap(),
            current
        );
    }
    assert!(retired > 0);
    assert!(current.tree.is_empty());
    assert_eq!(current.revision, 451);
    let mut tree = current.tree.clone();
    for key in 500..600 {
        tree.insert(Key::Integer(key), pointer(key)).unwrap();
    }
    let delta = current.delta_to(&tree).unwrap();
    assert_eq!(delta.apply(&current).unwrap().tree, tree);
}

#[test]
fn malformed_deltas_refuse_without_mutating_base_or_accepting_partial_changes() {
    let base = snapshot(30);
    let original = base.clone();
    let mut tree = base.tree.clone();
    for key in 20..27 {
        tree.remove(&Key::Integer(key)).unwrap();
    }
    let delta = base.delta_to(&tree).unwrap();
    assert!(!delta.retired.is_empty());
    let mut cases = Vec::new();
    let mut bad = delta.clone();
    bad.revision = 4;
    cases.push(bad);
    let mut bad = delta.clone();
    bad.root = 1000;
    cases.push(bad);
    let mut bad = delta.clone();
    bad.entries += 1;
    cases.push(bad);
    let mut bad = delta.clone();
    bad.retired.push(delta.retired[0]);
    cases.push(bad);
    let mut bad = delta.clone();
    bad.retired = vec![1000];
    cases.push(bad);
    let mut bad = delta.clone();
    bad.upserts.push(delta.upserts[0]);
    cases.push(bad);
    let mut bad = delta.clone();
    bad.upserts[0][100] ^= 1;
    cases.push(bad);
    let mut bad = delta.clone();
    bad.upserts = base.tree.page_images().unwrap();
    cases.push(bad);
    let mut bad = delta.clone();
    bad.retired = vec![1; MAX_INDEX_PAGES + 1];
    cases.push(bad);
    let mut bad = delta.clone();
    bad.upserts = vec![delta.upserts[0]; MAX_INDEX_PAGES + 1];
    cases.push(bad);
    for bad in cases {
        assert!(bad.apply(&base).is_err());
        assert_eq!(base, original);
    }
    let maximum = IndexSnapshot {
        revision: u64::MAX,
        tree: base.tree.clone(),
    };
    assert_eq!(maximum.delta_to(&maximum.tree), Err(Error::Limit));
    let unchanged = base.delta_to(&base.tree).unwrap();
    assert!(unchanged.upserts.is_empty() && unchanged.retired.is_empty());
    assert_eq!(
        unchanged.apply(&base).unwrap(),
        IndexSnapshot {
            revision: 2,
            tree: base.tree
        }
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn sparse_mutations_snapshots_and_deltas_match_an_independent_map(
        ops in prop::collection::vec((0u8..3, -80i64..80, 1u64..1000),1..180)
    ) {
        let mut model=BTreeMap::new();
        let mut current=IndexSnapshot {revision:1,tree:BPlusTree::new_stable()};
        for (op,key,value) in ops {
            let key=Key::Integer(key);
            let value=RecordPointer {page_id:value,slot_id:0};
            let mut tree=current.tree.clone();
            let success=match op {
                0=>{let expected=!model.contains_key(&key); let result=tree.insert(key.clone(),value); prop_assert_eq!(result.is_ok(),expected); if expected {model.insert(key,value);} expected},
                1=>{let expected=model.remove(&key); prop_assert_eq!(tree.remove(&key).ok(),expected); expected.is_some()},
                _=>{let expected=model.get_mut(&key).map(|old|std::mem::replace(old,value)); prop_assert_eq!(tree.replace(&key,value).ok(),expected); expected.is_some()},
            };
            if success {current=current.delta_to(&tree).unwrap().apply(&current).unwrap();}
            else {prop_assert_eq!(&tree,&current.tree);}
            let bytes=current.encode().unwrap();
            current=IndexSnapshot::decode(&bytes).unwrap();
            prop_assert_eq!(current.tree.range(None,None,MAX_INDEX_ENTRIES).unwrap(),model.iter().map(|(key,value)|(key.clone(),*value)).collect::<Vec<_>>());
        }
    }
    #[test]
    fn arbitrary_snapshot_input_is_bounded_and_accepted_bytes_are_canonical(
        bytes in prop::collection::vec(any::<u8>(),0..20000)
    ) {
        if let Ok(snapshot)=IndexSnapshot::decode(&bytes) {prop_assert_eq!(snapshot.encode().unwrap(),bytes);}
    }
}
