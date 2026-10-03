use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use proptest::prelude::*;
use sha2::{Digest, Sha256};
use std::sync::Arc;

fn physical(snapshot: &Snapshot) -> [u8; 32] {
    let mut hash = Sha256::new();
    for page in snapshot.pages() {
        hash.update(page.encode());
    }
    hash.finalize().into()
}
fn initialized() -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "t".into(),
                columns: vec![Column {
                    name: "id".into(),
                    data_type: DataType::Integer,
                    nullable: false,
                }],
                primary_key: 0,
            }),
        })
        .unwrap();
    snapshot
}
fn insert(snapshot: &mut Snapshot, key: i64) {
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(vec![Value::Integer(key)]),
        })
        .unwrap();
}

#[test]
fn cold_and_initialized_clone_branches_keep_their_exact_physical_fingerprints() {
    for initialize_first in [false, true] {
        let mut snapshot = initialized();
        insert(&mut snapshot, 1);
        let expected = physical(&snapshot);
        if initialize_first {
            assert_eq!(snapshot.page_fingerprint(), expected);
        }
        let old = snapshot.clone();
        insert(&mut snapshot, 2);
        assert_ne!(physical(&snapshot), expected);
        assert_eq!(snapshot.page_fingerprint(), physical(&snapshot));
        assert_eq!(old.page_fingerprint(), expected);
        let mut branch = old.clone();
        insert(&mut branch, 3);
        assert_eq!(branch.page_fingerprint(), physical(&branch));
        assert_ne!(branch.page_fingerprint(), snapshot.page_fingerprint());
        assert_eq!(old.page_fingerprint(), expected);
    }
}

#[test]
fn failed_events_and_cache_install_preserve_digest_but_identical_row_replacement_retires_it() {
    let mut snapshot = initialized();
    insert(&mut snapshot, 1);
    let before = snapshot.page_fingerprint();
    assert!(
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Integer(1)])
            })
            .is_err()
    );
    assert!(
        snapshot
            .apply(Event {
                table_id: u64::MAX,
                kind: EventKind::Drop
            })
            .is_err()
    );
    assert_eq!(snapshot.page_fingerprint(), before);
    let tree = snapshot.export_primary_tree("t").unwrap();
    snapshot.install_primary_tree("t", tree).unwrap();
    assert_eq!(snapshot.page_fingerprint(), before);
    let old = snapshot.clone();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Replace(vec![Value::Integer(1)]),
        })
        .unwrap();
    assert_eq!(snapshot.scan("t", 10).unwrap(), old.scan("t", 10).unwrap());
    assert_ne!(snapshot.page_fingerprint(), before);
    assert_eq!(snapshot.page_fingerprint(), physical(&snapshot));
    assert_eq!(old.page_fingerprint(), before);
}

#[test]
fn eight_shared_readers_observe_one_exact_immutable_history() {
    let mut snapshot = initialized();
    for key in 0..200 {
        insert(&mut snapshot, key);
    }
    let expected = physical(&snapshot);
    let shared = Arc::new(snapshot);
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let readers = (0..8)
        .map(|_| {
            let shared = Arc::clone(&shared);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..100 {
                    assert_eq!(shared.page_fingerprint(), expected);
                }
            })
        })
        .collect::<Vec<_>>();
    for reader in readers {
        reader.join().unwrap();
    }
    assert_eq!(physical(&shared), expected);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn physical_history_digest_survives_generated_forks_failures_and_replay(
        operations in proptest::collection::vec((-8i64..8, any::<bool>(), any::<bool>()), 0..64)
    ) {
        let mut snapshot = initialized();
        for (key, remove, initialized) in operations {
            let before = physical(&snapshot);
            if initialized { prop_assert_eq!(snapshot.page_fingerprint(), before); }
            let old = snapshot.clone();
            let exists = snapshot.get("t", &Key::Integer(key)).unwrap().is_some();
            let kind = if remove { EventKind::Delete(Key::Integer(key)) }
                else if exists { EventKind::Replace(vec![Value::Integer(key)]) }
                else { EventKind::Insert(vec![Value::Integer(key)]) };
            let result = snapshot.apply(Event { table_id: 1, kind });
            if remove && !exists { prop_assert!(result.is_err()); prop_assert_eq!(physical(&snapshot), before); }
            else { prop_assert!(result.is_ok()); prop_assert_ne!(physical(&snapshot), before); }
            prop_assert_eq!(snapshot.page_fingerprint(), physical(&snapshot));
            prop_assert_eq!(old.page_fingerprint(), before);
            let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
            prop_assert_eq!(replay.page_fingerprint(), snapshot.page_fingerprint());
        }
    }
}
