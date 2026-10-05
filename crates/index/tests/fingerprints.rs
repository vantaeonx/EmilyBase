use emilybase_index::{BPlusTree, Error, IndexPage, IndexSnapshot, Key, RecordPointer};
use proptest::prelude::*;
use sha2::{Digest, Sha256};

fn encoded_digest(snapshot: &IndexSnapshot) -> [u8; 32] {
    Sha256::digest(snapshot.encode().unwrap()).into()
}

fn pointer(key: i64) -> RecordPointer {
    RecordPointer {
        page_id: key.unsigned_abs() + 1,
        slot_id: key as u16,
    }
}

#[test]
fn frozen_envelopes_from_independent_struct_crc_encoder_keep_their_sha256() {
    // Constructed separately with Python struct/zlib, before this refactor.
    let empty = IndexSnapshot {
        revision: 1,
        tree: BPlusTree::new_stable(),
    };
    assert_eq!(
        format!("{:x}", Sha256::digest(empty.encode().unwrap())),
        "4546637c59762f417489480dee11b9bd938c9ab375ba35212e8adc81acb68461"
    );
    assert_eq!(empty.fingerprint().unwrap(), encoded_digest(&empty));
    let keys = [
        Key::Integer(i64::MIN),
        Key::Integer(i64::MAX),
        Key::Text(String::new()),
        Key::Text("\0".into()),
        Key::Text("é".into()),
        Key::Text("é".into()),
        Key::Text("я".into()),
        Key::Text("🌌".into()),
    ];
    let entries = keys
        .into_iter()
        .map(|key| {
            (
                key,
                RecordPointer {
                    page_id: u64::MAX,
                    slot_id: u16::MAX,
                },
            )
        })
        .collect();
    let image = IndexPage::leaf(1024, entries, None)
        .unwrap()
        .encode()
        .unwrap();
    let sparse = IndexSnapshot {
        revision: u64::MAX,
        tree: BPlusTree::from_stable_pages(1024, &[image]).unwrap(),
    };
    assert_eq!(
        format!("{:x}", Sha256::digest(sparse.encode().unwrap())),
        "47633ccf86be655e3c803eb8c2183e315e81a7e8d8f27eed130ab466422b2b6e"
    );
    assert_eq!(sparse.fingerprint().unwrap(), encoded_digest(&sparse));
}

#[test]
fn fingerprints_equal_full_encoding_for_empty_dense_sparse_and_full_capacity() {
    for count in [0, 1, 14, 15, 225, 10000] {
        let entries: Vec<_> = (0..count)
            .map(|key| (Key::Integer(key), pointer(key)))
            .collect();
        let mut value = IndexSnapshot {
            revision: 1,
            tree: BPlusTree::from_sorted_stable(&entries).unwrap(),
        };
        let before = value.tree.page_images().unwrap();
        value.validate().unwrap();
        assert_eq!(value.fingerprint().unwrap(), encoded_digest(&value));
        assert_eq!(value.tree.page_images().unwrap(), before);
        if count >= 225 {
            for key in 40..70 {
                value.tree.remove(&Key::Integer(key)).unwrap();
            }
            value.revision = u64::MAX;
            assert_eq!(value.fingerprint().unwrap(), encoded_digest(&value));
        }
        let reopened = IndexSnapshot::decode(&value.encode().unwrap()).unwrap();
        assert_eq!(reopened.fingerprint().unwrap(), encoded_digest(&value));
    }
}

#[test]
fn fingerprint_and_encoding_refuse_the_same_revision_and_arena_errors() {
    for value in [
        IndexSnapshot {
            revision: 0,
            tree: BPlusTree::new_stable(),
        },
        IndexSnapshot {
            revision: 1,
            tree: BPlusTree::new(),
        },
    ] {
        let expected = || Error::Layout("snapshot requires revision and stable IDs");
        assert_eq!(value.fingerprint(), Err(expected()));
        assert_eq!(value.validate(), Err(expected()));
        assert_eq!(value.encode().unwrap_err(), expected());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_mutations_keep_exact_canonical_fingerprints(
        keys in prop::collection::btree_set(-400i64..400, 0..300),
        revision in 1u64..u64::MAX,
    ) {
        let entries: Vec<_> = keys.iter().map(|key| (Key::Integer(*key), pointer(*key))).collect();
        let base = IndexSnapshot {
            revision,
            tree: BPlusTree::from_sorted_stable(&entries).unwrap(),
        };
        let base_digest = encoded_digest(&base);
        prop_assert_eq!(base.fingerprint().unwrap(), base_digest);
        let mut changed = base.clone();
        for key in keys.iter().filter(|key| **key % 3 == 0) {
            changed.tree.remove(&Key::Integer(*key)).unwrap();
        }
        changed.revision += 1;
        prop_assert_eq!(changed.fingerprint().unwrap(), encoded_digest(&changed));
        prop_assert_eq!(base.fingerprint().unwrap(), base_digest);
    }
}
