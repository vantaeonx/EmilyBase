#![cfg(feature = "heap-profile")]
//! One isolated native test process measures operation-local requested bytes.
//! Retained fixtures are built before each profiler and are outside its counters.
use emilybase_index::{BPlusTree, IndexSnapshot, Key, RecordPointer};
use sha2::{Digest, Sha256};

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

fn transient(operation: impl FnOnce()) -> dhat::HeapStats {
    let profiler = dhat::Profiler::builder().testing().build();
    operation();
    let stats = dhat::HeapStats::get();
    drop(profiler);
    stats
}

#[test]
fn full_capacity_short_key_validation_hashing_and_deltas_avoid_complete_temporary_images() {
    let entries: Vec<_> = (0..10000)
        .map(|number| {
            (
                Key::Text(format!("{number:08}{}", "x".repeat(248))),
                RecordPointer {
                    page_id: number as u64 + 1,
                    slot_id: number as u16,
                },
            )
        })
        .collect();
    let base = IndexSnapshot {
        revision: 1,
        tree: BPlusTree::from_sorted_stable(&entries).unwrap(),
    };
    assert_eq!(base.tree.page_count(), 768);
    let original = base.encode().unwrap();
    let expected: [u8; 32] = Sha256::digest(&original).into();
    let local_cap = 128 * 1024;
    let cloning = transient(|| {
        let cloned = base.clone();
        assert_eq!(cloned, base);
    });
    assert_eq!(cloning.curr_bytes, 0);
    assert!(
        cloning.max_bytes < local_cap,
        "clone peak {}",
        cloning.max_bytes
    );
    let validate = transient(|| base.validate().unwrap());
    assert_eq!(validate.curr_bytes, 0);
    assert!(
        validate.max_bytes < local_cap,
        "validate peak {}",
        validate.max_bytes
    );
    let hash = transient(|| assert_eq!(base.fingerprint().unwrap(), expected));
    assert_eq!(hash.curr_bytes, 0);
    assert!(
        hash.max_bytes < local_cap,
        "fingerprint peak {}",
        hash.max_bytes
    );
    let encoding = transient(|| assert_eq!(base.encode().unwrap(), original));
    assert_eq!(encoding.curr_bytes, 0);
    assert!(
        encoding.max_bytes < original.len() + local_cap,
        "encode peak {}",
        encoding.max_bytes
    );
    let decoding = transient(|| assert_eq!(IndexSnapshot::decode(&original).unwrap(), base));
    assert_eq!(decoding.curr_bytes, 0);
    assert!(
        decoding.max_bytes < original.len() + 1024 * 1024,
        "decode peak {}",
        decoding.max_bytes
    );
    let unchanged = transient(|| {
        let delta = base.delta_to(&base.tree).unwrap();
        assert!(delta.upserts.is_empty());
        assert!(delta.retired.is_empty());
    });
    assert_eq!(unchanged.curr_bytes, 0);
    assert!(
        unchanged.max_bytes < local_cap,
        "no-op delta peak {}",
        unchanged.max_bytes
    );
    let mut changed = base.tree.clone();
    let key = entries[5000].0.clone();
    let next = RecordPointer {
        page_id: 60000,
        slot_id: 17,
    };
    changed.replace(&key, next).unwrap();
    let delta_stats = transient(|| {
        let delta = base.delta_to(&changed).unwrap();
        assert_eq!(delta.upserts.len(), 1);
        assert!(delta.retired.is_empty());
    });
    assert_eq!(delta_stats.curr_bytes, 0);
    assert!(
        delta_stats.max_bytes < local_cap,
        "one-page delta peak {}",
        delta_stats.max_bytes
    );
    let delta = base.delta_to(&changed).unwrap();
    let applied = transient(|| {
        let result = delta.apply(&base).unwrap();
        assert_eq!(result.tree, changed);
        assert_eq!(result.tree.get(&key).unwrap(), Some(next));
    });
    assert_eq!(applied.curr_bytes, 0);
    let apply_cap = local_cap;
    assert!(
        applied.max_bytes < apply_cap,
        "apply peak {}, cap {}",
        applied.max_bytes,
        apply_cap
    );
    let independent: [IndexSnapshot; 4] =
        std::array::from_fn(|_| IndexSnapshot::decode(&original).unwrap());
    let parallel = transient(|| {
        let gate = std::sync::Barrier::new(5);
        std::thread::scope(|scope| {
            let workers: Vec<_> = independent
                .iter()
                .map(|snapshot| {
                    let gate = &gate;
                    scope.spawn(move || {
                        gate.wait();
                        snapshot.validate().unwrap();
                        assert_eq!(snapshot.fingerprint().unwrap(), expected);
                        let delta = snapshot.delta_to(&snapshot.tree).unwrap();
                        assert!(delta.upserts.is_empty());
                    })
                })
                .collect();
            gate.wait();
            for worker in workers {
                worker.join().unwrap();
            }
        });
    });
    assert_eq!(parallel.curr_bytes, 0);
    assert!(
        parallel.max_bytes < 512 * 1024,
        "parallel peak {}",
        parallel.max_bytes
    );
    assert_eq!(base.tree.get(&key).unwrap(), Some(entries[5000].1));
    assert_eq!(base.fingerprint().unwrap(), expected);
    eprintln!(
        "requested peaks: clone={} validate={} hash={} encode={} decode={} no_op={} one_page={} apply={} parallel={}",
        cloning.max_bytes,
        validate.max_bytes,
        hash.max_bytes,
        encoding.max_bytes,
        decoding.max_bytes,
        unchanged.max_bytes,
        delta_stats.max_bytes,
        applied.max_bytes,
        parallel.max_bytes
    );
}
