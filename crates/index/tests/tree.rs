use emilybase_index::*;

fn pointer(value: u64) -> RecordPointer {
    RecordPointer {
        page_id: value + 1,
        slot_id: value as u16,
    }
}
fn populated(count: i64) -> BPlusTree {
    let mut tree = BPlusTree::new();
    for value in (0..count).rev() {
        tree.insert(Key::Integer(value), pointer(value as u64))
            .unwrap();
    }
    tree
}
fn repair(bytes: &mut [u8; PAGE_SIZE]) {
    let mut crc = crc32fast::Hasher::new();
    crc.update(&bytes[..60]);
    crc.update(&bytes[64..]);
    bytes[60..64].copy_from_slice(&crc.finalize().to_le_bytes());
}

#[test]
fn splits_propagate_to_branches_and_leaf_scans_survive_serialization() {
    let tree = populated(1200);
    assert!(tree.page_count() > 100);
    assert_eq!(tree.validate().unwrap(), 1200);
    for value in 0..1200 {
        assert_eq!(
            tree.get(&Key::Integer(value)).unwrap(),
            Some(pointer(value as u64))
        );
    }
    for absent in [-1, 1200, i64::MAX] {
        assert_eq!(tree.get(&Key::Integer(absent)).unwrap(), None);
    }
    let all = tree.range(None, None, MAX_INDEX_ENTRIES).unwrap();
    assert_eq!(
        all,
        (0..1200)
            .map(|i| (Key::Integer(i), pointer(i as u64)))
            .collect::<Vec<_>>()
    );
    let restored = BPlusTree::from_pages(tree.root_id(), &tree.page_images().unwrap()).unwrap();
    assert_eq!(restored, tree);
    assert_eq!(
        restored
            .range(Some(&Key::Integer(101)), Some(&Key::Integer(901)), 19)
            .unwrap(),
        all[101..120]
    );
    assert!(
        restored
            .range(Some(&Key::Integer(9)), Some(&Key::Integer(9)), 1)
            .unwrap()
            .is_empty()
    );
    assert!(
        restored
            .range(Some(&Key::Integer(10)), Some(&Key::Integer(9)), 1)
            .unwrap()
            .is_empty()
    );
    assert!(restored.range(None, None, 0).unwrap().is_empty());
}

#[test]
fn signed_integers_and_unicode_sort_without_collation() {
    let mut tree = BPlusTree::new();
    let mut keys = vec![
        Key::Text("🌌".into()),
        Key::Integer(i64::MIN),
        Key::Integer(-1),
        Key::Integer(i64::MAX),
        Key::Text("".into()),
        Key::Text("é".into()),
        Key::Text("é".into()),
        Key::Text("я".into()),
    ];
    for key in &keys {
        tree.insert(
            key.clone(),
            RecordPointer {
                page_id: u64::MAX,
                slot_id: u16::MAX,
            },
        )
        .unwrap();
    }
    keys.sort();
    assert_eq!(
        tree.range(None, None, 100)
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect::<Vec<_>>(),
        keys
    );
    let reopened = BPlusTree::from_pages(tree.root_id(), &tree.page_images().unwrap()).unwrap();
    assert_eq!(reopened, tree);
}

#[test]
fn failed_insert_and_invalid_scan_leave_exact_images_unchanged() {
    let mut tree = populated(100);
    let before = tree.clone();
    assert_eq!(
        tree.insert(Key::Integer(12), pointer(999)),
        Err(Error::Duplicate)
    );
    assert_eq!(
        tree.insert(Key::Text("я".repeat(129)), pointer(0)),
        Err(Error::KeySize)
    );
    assert_eq!(
        tree.insert(
            Key::Integer(101),
            RecordPointer {
                page_id: 0,
                slot_id: 0
            }
        ),
        Err(Error::PageId)
    );
    assert_eq!(
        tree.range(None, None, MAX_INDEX_ENTRIES + 1),
        Err(Error::Limit)
    );
    assert_eq!(tree.get(&Key::Text("x".repeat(257))), Err(Error::KeySize));
    assert_eq!(tree, before);
    assert_eq!(tree.page_images().unwrap(), before.page_images().unwrap());
}

#[test]
fn actual_page_capacity_failure_is_atomic_even_during_a_split() {
    let mut tree = BPlusTree::new();
    for key in 0..=MAX_INDEX_ENTRIES {
        let before = tree.clone();
        match tree.insert(Key::Integer(key as i64), pointer(key as u64)) {
            Ok(()) => continue,
            Err(Error::Limit) => {
                assert_eq!(tree, before);
                assert!(tree.page_count() <= MAX_INDEX_PAGES);
                assert_eq!(tree.validate().unwrap(), key);
                assert_eq!(tree.get(&Key::Integer(key as i64)).unwrap(), None);
                let reopened =
                    BPlusTree::from_pages(tree.root_id(), &tree.page_images().unwrap()).unwrap();
                assert_eq!(reopened, tree);
                return;
            }
            other => panic!("unexpected insertion outcome: {other:?}"),
        }
    }
    panic!("configured bounds were not enforced");
}

#[test]
fn largest_keys_and_empty_root_round_trip_in_fixed_pages() {
    let entries = (0..MAX_KEYS)
        .map(|i| {
            (
                Key::Text(format!("{i:02}{}", "x".repeat(254))),
                pointer(i as u64),
            )
        })
        .collect();
    let leaf = IndexPage::leaf(1, entries, Some(2)).unwrap();
    let bytes = leaf.encode().unwrap();
    assert_eq!(IndexPage::decode(&bytes, 1).unwrap(), leaf);
    let branch = IndexPage::branch(20, leaf.keys().to_vec(), (1..=15).collect()).unwrap();
    assert_eq!(
        IndexPage::decode(&branch.encode().unwrap(), 20).unwrap(),
        branch
    );
    let empty = BPlusTree::new();
    assert!(empty.is_empty());
    assert_eq!(
        BPlusTree::from_pages(1, &empty.page_images().unwrap()).unwrap(),
        empty
    );
    assert_eq!(leaf.pointers().unwrap().len(), MAX_KEYS);
    assert_eq!(branch.children().unwrap().len(), MAX_KEYS + 1);
}

#[test]
fn every_page_cut_and_single_byte_change_is_rejected() {
    let page = IndexPage::leaf(1, vec![(Key::Integer(7), pointer(0))], None)
        .unwrap()
        .encode()
        .unwrap();
    for cut in 0..PAGE_SIZE {
        assert!(IndexPage::decode(&page[..cut], 1).is_err(), "cut={cut}");
    }
    for offset in 0..PAGE_SIZE {
        let mut changed = page;
        changed[offset] ^= 1;
        assert!(IndexPage::decode(&changed, 1).is_err(), "offset={offset}");
    }
    assert_eq!(IndexPage::decode(&page, 2), Err(Error::PageId));
    let mut unknown = page;
    unknown[4..6].copy_from_slice(&2u16.to_le_bytes());
    repair(&mut unknown);
    assert_eq!(IndexPage::decode(&unknown, 1), Err(Error::Version(2)));
}

#[test]
fn valid_checksum_does_not_hide_local_layout_corruption() {
    let original = IndexPage::leaf(
        1,
        vec![(Key::Integer(7), pointer(0)), (Key::Integer(8), pointer(1))],
        None,
    )
    .unwrap()
    .encode()
    .unwrap();
    let mutations: Vec<(usize, Vec<u8>)> = vec![
        (6, vec![3]),
        (7, vec![1]),
        (16, 15u16.to_le_bytes().to_vec()),
        (18, 63u16.to_le_bytes().to_vec()),
        (20, vec![1]),
        (24, 1u64.to_le_bytes().to_vec()),
        (32, 1u64.to_le_bytes().to_vec()),
        (40, vec![1]),
        (64, vec![99]),
        (65, 9u16.to_le_bytes().to_vec()),
        (75, 0u64.to_le_bytes().to_vec()),
        (85, vec![1]),
        (94, 7i64.to_le_bytes().to_vec()),
        (4095, vec![1]),
    ];
    for (offset, data) in mutations {
        let mut bytes = original;
        bytes[offset..offset + data.len()].copy_from_slice(&data);
        repair(&mut bytes);
        assert!(IndexPage::decode(&bytes, 1).is_err(), "offset={offset}");
    }
    let mut utf8 = IndexPage::leaf(1, vec![(Key::Text("x".into()), pointer(0))], None)
        .unwrap()
        .encode()
        .unwrap();
    utf8[67] = 255;
    repair(&mut utf8);
    assert_eq!(IndexPage::decode(&utf8, 1), Err(Error::Layout("UTF-8")));
}

#[test]
fn constructors_reject_bad_local_structure() {
    assert_eq!(IndexPage::leaf(0, Vec::new(), None), Err(Error::PageId));
    assert!(
        IndexPage::leaf(
            1,
            vec![(Key::Integer(2), pointer(0)), (Key::Integer(1), pointer(1))],
            None
        )
        .is_err()
    );
    assert!(IndexPage::branch(1, vec![Key::Integer(3)], vec![2, 2]).is_err());
    assert!(IndexPage::branch(1, vec![Key::Integer(3)], vec![2]).is_err());
    assert!(IndexPage::branch(1, vec![Key::Integer(3)], vec![2, 1]).is_err());
    assert!(IndexPage::leaf(1, Vec::new(), Some(0)).is_err());
}

#[test]
fn valid_pages_still_require_matching_separators_chain_and_reachability() {
    let leaf = |id, begin, next| {
        IndexPage::leaf(
            id,
            (begin..begin + 7)
                .map(|i| (Key::Integer(i), pointer(i as u64)))
                .collect(),
            next,
        )
        .unwrap()
    };
    let root = IndexPage::branch(3, vec![Key::Integer(7)], vec![1, 2]).unwrap();
    let images = vec![
        leaf(1, 0, Some(2)).encode().unwrap(),
        leaf(2, 7, None).encode().unwrap(),
        root.encode().unwrap(),
    ];
    assert_eq!(BPlusTree::from_pages(3, &images).unwrap().len(), 14);
    for root in [0, 4, u64::MAX] {
        assert!(BPlusTree::from_pages(root, &images).is_err());
    }
    let mut wrong = images.clone();
    wrong[2] = IndexPage::branch(3, vec![Key::Integer(8)], vec![1, 2])
        .unwrap()
        .encode()
        .unwrap();
    assert_eq!(
        BPlusTree::from_pages(3, &wrong),
        Err(Error::Layout("separator or child key range"))
    );
    wrong = images.clone();
    wrong[0] = leaf(1, 0, None).encode().unwrap();
    assert_eq!(
        BPlusTree::from_pages(3, &wrong),
        Err(Error::Layout("leaf chain disagrees with tree"))
    );
    wrong = images.clone();
    wrong[2] = IndexPage::branch(3, vec![Key::Integer(7)], vec![1, 4])
        .unwrap()
        .encode()
        .unwrap();
    assert_eq!(
        BPlusTree::from_pages(3, &wrong),
        Err(Error::Layout("missing child"))
    );
    wrong = images.clone();
    wrong.push(
        IndexPage::leaf(4, Vec::new(), None)
            .unwrap()
            .encode()
            .unwrap(),
    );
    assert_eq!(
        BPlusTree::from_pages(3, &wrong),
        Err(Error::Layout("unreachable pages"))
    );
    wrong = images.clone();
    wrong[0] = IndexPage::leaf(1, vec![(Key::Integer(0), pointer(0))], Some(2))
        .unwrap()
        .encode()
        .unwrap();
    assert_eq!(
        BPlusTree::from_pages(3, &wrong),
        Err(Error::Layout("underfull child"))
    );
    wrong = images;
    wrong.swap(0, 1);
    assert_eq!(BPlusTree::from_pages(3, &wrong), Err(Error::PageId));
}
