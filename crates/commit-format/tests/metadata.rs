use emilybase_commit_format::{
    ADDRESS_BYTES, Domain, Error, IndexKeyType, MAX_HISTORY_PAGES, MAX_LIVE_KEYS,
    MAX_PRIMARY_PAGES, MAX_TRANSACTION, PageAddress, Predecessor, ROOT_BYTES, RootBinding,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};

fn identity() -> [u8; 16] {
    std::array::from_fn(|index| index as u8 + 1)
}
fn fingerprint() -> [u8; 32] {
    std::array::from_fn(|index| index as u8)
}
fn root() -> RootBinding {
    RootBinding::new(
        PageAddress::primary(identity(), 42, 1024).unwrap(),
        IndexKeyType::Text,
        7,
        29,
        9998,
        2,
        768,
        Some(Predecessor::new(6, 21, fingerprint()).unwrap()),
    )
    .unwrap()
}
fn first(database: [u8; 16], table: u64, key_type: IndexKeyType) -> RootBinding {
    RootBinding::new(
        PageAddress::primary(database, table, 1).unwrap(),
        key_type,
        1,
        2,
        0,
        0,
        1,
        None,
    )
    .unwrap()
}
fn repair(bytes: &mut [u8]) {
    let end = bytes.len() - 4;
    let crc = crc32fast::hash(&bytes[..end]);
    bytes[end..].copy_from_slice(&crc.to_le_bytes());
}
fn replace(bytes: &mut [u8], offset: usize, number: u64) {
    bytes[offset..offset + 8].copy_from_slice(&number.to_le_bytes());
    repair(bytes);
}

#[test]
fn canonical_bytes_match_independently_generated_frozen_hashes() {
    let address = PageAddress::primary(identity(), 42, 1024).unwrap();
    let encoded = address.encode().unwrap();
    assert_eq!(encoded.len(), ADDRESS_BYTES);
    assert_eq!(PageAddress::decode(&encoded).unwrap(), address);
    assert_eq!(
        format!("{:x}", Sha256::digest(encoded)),
        "490f2ec89b75fc6c136d5cd62e73ca1905d94c91e28d7f97db6f18f0f893cb30"
    );
    let expected = root();
    let encoded = expected.encode().unwrap();
    assert_eq!(encoded.len(), ROOT_BYTES);
    assert_eq!(RootBinding::decode(&encoded).unwrap(), expected);
    assert_eq!(
        format!("{:x}", Sha256::digest(encoded)),
        "ec9b1cf36a0baa19d4fe0f8e0727a2e8dfa25f689ff7e0df69bbfff17371c055"
    );
    assert_eq!(expected.address().table(), 42);
    assert_eq!(expected.address().page(), 1024);
    assert_eq!(expected.key_type(), IndexKeyType::Text);
    assert_eq!((expected.revision(), expected.transaction()), (7, 29));
    assert_eq!(
        (expected.covered(), expected.excluded(), expected.pages()),
        (9998, 2, 768)
    );
    let base = expected.predecessor().unwrap();
    assert_eq!((base.revision(), base.transaction()), (6, 21));
    assert_eq!(base.fingerprint(), fingerprint());
}

#[test]
fn database_domain_and_table_scopes_distinguish_equal_page_numbers() {
    let addresses = [
        PageAddress::history([1; 16], 1).unwrap(),
        PageAddress::primary([1; 16], 1, 1).unwrap(),
        PageAddress::primary([1; 16], 2, 1).unwrap(),
        PageAddress::primary([2; 16], 1, 1).unwrap(),
    ];
    assert_eq!(addresses.into_iter().collect::<HashSet<_>>().len(), 4);
    assert_eq!(addresses.into_iter().collect::<BTreeSet<_>>().len(), 4);
    assert_eq!(addresses[0].domain(), Domain::RelationalHistory);
    assert_eq!(addresses[0].table(), 0);
    let large_table = PageAddress::primary([1; 16], u64::MAX, MAX_PRIMARY_PAGES).unwrap();
    assert_eq!(
        PageAddress::decode(&large_table.encode().unwrap()).unwrap(),
        large_table
    );
    let last_history = PageAddress::history([1; 16], MAX_HISTORY_PAGES).unwrap();
    assert_eq!(
        PageAddress::decode(&last_history.encode().unwrap()).unwrap(),
        last_history
    );
    assert_eq!(MAX_HISTORY_PAGES, emilybase_storage::MAX_PAGES);
    assert_eq!(MAX_PRIMARY_PAGES as usize, emilybase_index::MAX_INDEX_PAGES);
    assert_eq!(MAX_LIVE_KEYS as usize, emilybase_database::MAX_ROWS);
}

#[test]
fn constructors_reject_missing_scope_identity_and_out_of_range_pages() {
    assert!(PageAddress::history([0; 16], 1).is_err());
    assert!(PageAddress::primary([0; 16], 1, 1).is_err());
    assert!(PageAddress::primary([1; 16], 0, 1).is_err());
    for page in [0, MAX_HISTORY_PAGES + 1, u64::MAX] {
        assert!(PageAddress::history([1; 16], page).is_err());
    }
    for page in [0, MAX_PRIMARY_PAGES + 1, u64::MAX] {
        assert!(PageAddress::primary([1; 16], 1, page).is_err());
    }
}

#[test]
fn every_cut_trailing_byte_and_single_bit_change_is_rejected() {
    let address = PageAddress::history(identity(), MAX_HISTORY_PAGES)
        .unwrap()
        .encode()
        .unwrap();
    let root = root().encode().unwrap();
    for bytes in [address.as_slice(), root.as_slice()] {
        let valid = bytes.len();
        for cut in 0..valid {
            let rejected = if valid == ADDRESS_BYTES {
                PageAddress::decode(&bytes[..cut]).map(|_| ())
            } else {
                RootBinding::decode(&bytes[..cut]).map(|_| ())
            };
            assert_eq!(rejected, Err(Error::Length));
        }
        for length in 1..=32 {
            let mut extended = bytes.to_vec();
            extended.extend(vec![0; length]);
            assert!(PageAddress::decode(&extended).is_err());
            assert!(RootBinding::decode(&extended).is_err());
        }
        for index in 0..valid {
            for bit in 0..8 {
                let mut changed = bytes.to_vec();
                changed[index] ^= 1 << bit;
                if valid == ADDRESS_BYTES {
                    assert!(PageAddress::decode(&changed).is_err());
                } else {
                    assert!(RootBinding::decode(&changed).is_err());
                }
            }
        }
    }
}

#[test]
fn repaired_checksums_cannot_enable_reserved_fields_or_unknown_tags() {
    let address = PageAddress::primary(identity(), 1, 1)
        .unwrap()
        .encode()
        .unwrap();
    for position in std::iter::once(11).chain(44..60) {
        let mut changed = address;
        changed[position] = 1;
        repair(&mut changed);
        assert_eq!(PageAddress::decode(&changed), Err(Error::Reserved));
    }
    for tag in (0..=u8::MAX).filter(|tag| ![1, 2].contains(tag)) {
        let mut changed = address;
        changed[10] = tag;
        repair(&mut changed);
        assert_eq!(PageAddress::decode(&changed), Err(Error::Domain(tag)));
        let mut changed = root().encode().unwrap();
        changed[10] = tag;
        repair(&mut changed);
        assert_eq!(RootBinding::decode(&changed), Err(Error::KeyType(tag)));
    }
    let root = root().encode().unwrap();
    for position in std::iter::once(11).chain(128..188) {
        let mut changed = root;
        changed[position] = 1;
        repair(&mut changed);
        assert_eq!(RootBinding::decode(&changed), Err(Error::Reserved));
    }
    for version in [0, 2, u16::MAX] {
        let mut a = address;
        a[8..10].copy_from_slice(&version.to_le_bytes());
        repair(&mut a);
        assert_eq!(PageAddress::decode(&a), Err(Error::Version(version)));
        let mut r = root;
        r[8..10].copy_from_slice(&version.to_le_bytes());
        repair(&mut r);
        assert_eq!(RootBinding::decode(&r), Err(Error::Version(version)));
    }
}

#[test]
fn repaired_address_fields_cannot_change_domain_scope_or_admission() {
    let history = PageAddress::history(identity(), 1)
        .unwrap()
        .encode()
        .unwrap();
    let primary = PageAddress::primary(identity(), 1, 1)
        .unwrap()
        .encode()
        .unwrap();
    for (image, offset, value) in [
        (history, 28, 1),
        (primary, 28, 0),
        (history, 36, 0),
        (primary, 36, 0),
        (history, 36, MAX_HISTORY_PAGES + 1),
        (primary, 36, MAX_PRIMARY_PAGES + 1),
        (primary, 36, u64::MAX),
    ] {
        let mut changed = image;
        replace(&mut changed, offset, value);
        assert!(matches!(
            PageAddress::decode(&changed),
            Err(Error::Invalid(_))
        ));
    }
    let mut changed = primary;
    changed[12..28].fill(0);
    repair(&mut changed);
    assert!(PageAddress::decode(&changed).is_err());
}

#[test]
fn roots_preserve_sparse_ids_and_explicit_long_key_coverage() {
    let address = PageAddress::primary(identity(), 1, 1024).unwrap();
    let text = RootBinding::new(address, IndexKeyType::Text, 1, 1, 0, 10000, 1, None).unwrap();
    assert_eq!(RootBinding::decode(&text.encode().unwrap()).unwrap(), text);
    assert!(RootBinding::new(address, IndexKeyType::Integer, 1, 1, 0, 1, 1, None).is_err());
    for (covered, excluded, pages) in [(10000, 1, 1), (u64::MAX, 1, 1), (0, 0, 0), (0, 0, 1025)] {
        assert!(
            RootBinding::new(
                address,
                IndexKeyType::Text,
                1,
                1,
                covered,
                excluded,
                pages,
                None
            )
            .is_err()
        );
    }
    assert!(
        RootBinding::new(
            PageAddress::history(identity(), 1).unwrap(),
            IndexKeyType::Text,
            1,
            1,
            0,
            0,
            1,
            None
        )
        .is_err()
    );
}

#[test]
fn revisions_and_database_transactions_have_distinct_exact_predecessors() {
    let previous = first(identity(), 1, IndexKeyType::Text);
    let current = RootBinding::new(
        PageAddress::primary(identity(), 1, 99).unwrap(),
        IndexKeyType::Text,
        2,
        700,
        2,
        1,
        3,
        Some(Predecessor::new(1, 2, fingerprint()).unwrap()),
    )
    .unwrap();
    current.verify_predecessor(previous, fingerprint()).unwrap();
    assert_eq!(
        current.verify_predecessor(previous, [9; 32]),
        Err(Error::Predecessor)
    );
    for previous in [
        first([9; 16], 1, IndexKeyType::Text),
        first(identity(), 2, IndexKeyType::Text),
        first(identity(), 1, IndexKeyType::Integer),
    ] {
        assert_eq!(
            current.verify_predecessor(previous, fingerprint()),
            Err(Error::Predecessor)
        );
    }
    current.verify_owner(identity(), 1, 700).unwrap();
    for (database, table, transaction) in [
        ([9; 16], 1, 700),
        (identity(), 2, 700),
        (identity(), 1, 699),
    ] {
        assert_eq!(
            current.verify_owner(database, table, transaction),
            Err(Error::Identity)
        );
    }
    assert_eq!(
        previous.verify_predecessor(previous, fingerprint()),
        Err(Error::Predecessor)
    );
}

#[test]
fn first_root_and_counter_exhaustion_require_canonical_fields() {
    let address = PageAddress::primary(identity(), 1, 1).unwrap();
    assert!(Predecessor::new(0, 1, [0; 32]).is_err());
    assert!(Predecessor::new(1, 0, [0; 32]).is_err());
    assert!(Predecessor::new(1, u64::MAX, [0; 32]).is_err());
    assert!(RootBinding::new(address, IndexKeyType::Integer, 0, 1, 0, 0, 1, None).is_err());
    assert!(RootBinding::new(address, IndexKeyType::Integer, 2, 1, 0, 0, 1, None).is_err());
    for transaction in [0, u64::MAX] {
        assert!(
            RootBinding::new(
                address,
                IndexKeyType::Integer,
                1,
                transaction,
                0,
                0,
                1,
                None
            )
            .is_err()
        );
    }
    for (revision, transaction, base_revision, base_transaction) in [
        (1, 2, 1, 1),
        (3, 2, 1, 1),
        (2, 2, 1, 2),
        (2, 2, 1, 3),
        (1, 2, u64::MAX, 1),
    ] {
        let base = Predecessor::new(base_revision, base_transaction, [0; 32]).unwrap();
        assert!(
            RootBinding::new(
                address,
                IndexKeyType::Integer,
                revision,
                transaction,
                0,
                0,
                1,
                Some(base)
            )
            .is_err()
        );
    }
    let terminal = RootBinding::new(
        address,
        IndexKeyType::Integer,
        u64::MAX,
        MAX_TRANSACTION,
        0,
        0,
        1,
        Some(Predecessor::new(u64::MAX - 1, 1, [0; 32]).unwrap()),
    )
    .unwrap();
    assert_eq!(
        RootBinding::decode(&terminal.encode().unwrap()).unwrap(),
        terminal
    );
    let first = first(identity(), 1, IndexKeyType::Integer)
        .encode()
        .unwrap();
    assert_eq!(&first[60..108], &[0; 48]);
    for position in 68..108 {
        let mut changed = first;
        changed[position] = 1;
        repair(&mut changed);
        assert!(RootBinding::decode(&changed).is_err());
    }
}

#[test]
fn repaired_root_checksums_do_not_admit_invalid_counts_or_base_fields() {
    let image = root().encode().unwrap();
    for (offset, value) in [
        (28, 0),
        (36, 0),
        (36, 1025),
        (44, 0),
        (52, 0),
        (52, u64::MAX),
        (60, 0),
        (60, 5),
        (68, 0),
        (68, 29),
        (108, 10001),
        (108, u64::MAX),
        (116, u64::MAX),
    ] {
        let mut changed = image;
        replace(&mut changed, offset, value);
        assert!(RootBinding::decode(&changed).is_err(), "offset {offset}");
    }
    for count in [0u32, 1025, u32::MAX] {
        let mut changed = image;
        changed[124..128].copy_from_slice(&count.to_le_bytes());
        repair(&mut changed);
        assert!(RootBinding::decode(&changed).is_err());
    }
    let mut changed = image;
    changed[12..28].fill(0);
    repair(&mut changed);
    assert!(RootBinding::decode(&changed).is_err());
    let mut changed = image;
    changed[10] = IndexKeyType::Integer as u8;
    repair(&mut changed);
    assert!(RootBinding::decode(&changed).is_err());
}

#[test]
fn actual_standalone_tree_fingerprints_cannot_authorize_another_base() {
    use emilybase_index::{BPlusTree, IndexSnapshot, Key, RecordPointer};
    let mut tree = BPlusTree::new_stable();
    tree.insert(
        Key::Integer(1),
        RecordPointer {
            page_id: 2,
            slot_id: 0,
        },
    )
    .unwrap();
    let previous = IndexSnapshot { revision: 1, tree };
    let previous_root = RootBinding::new(
        PageAddress::primary(identity(), 1, previous.tree.root_id()).unwrap(),
        IndexKeyType::Integer,
        1,
        2,
        1,
        0,
        previous.tree.page_count() as u32,
        None,
    )
    .unwrap();
    let mut next_tree = previous.tree.clone();
    next_tree
        .insert(
            Key::Integer(2),
            RecordPointer {
                page_id: 3,
                slot_id: 0,
            },
        )
        .unwrap();
    let delta = previous.delta_to(&next_tree).unwrap();
    let selected = delta.apply(&previous).unwrap();
    let binding = RootBinding::new(
        PageAddress::primary(identity(), 1, selected.tree.root_id()).unwrap(),
        IndexKeyType::Integer,
        selected.revision,
        9,
        selected.tree.len() as u64,
        0,
        selected.tree.page_count() as u32,
        Some(Predecessor::new(previous.revision, 2, previous.fingerprint().unwrap()).unwrap()),
    )
    .unwrap();
    binding
        .verify_predecessor(previous_root, previous.fingerprint().unwrap())
        .unwrap();
    let other = IndexSnapshot {
        revision: 1,
        tree: BPlusTree::new_stable(),
    };
    assert_eq!(
        binding.verify_predecessor(previous_root, other.fingerprint().unwrap()),
        Err(Error::Predecessor)
    );
    assert_eq!(
        RootBinding::decode(&binding.encode().unwrap()).unwrap(),
        binding
    );
    assert_eq!(previous.tree.len(), 1);
    assert_eq!(selected.tree.len(), 2);
}
