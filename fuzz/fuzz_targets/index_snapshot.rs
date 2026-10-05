#![no_main]
#![forbid(unsafe_code)]

use emilybase_index::{BPlusTree, IndexSnapshot, Key, MAX_INDEX_ENTRIES, PAGE_SIZE, RecordPointer};
use libfuzzer_sys::fuzz_target;
use sha2::{Digest, Sha256};

fn check(bytes: &[u8]) {
    if let Ok(snapshot) = IndexSnapshot::decode(bytes) {
        assert_eq!(snapshot.encode().unwrap(), bytes);
        snapshot.validate().unwrap();
        let canonical_hash: [u8; 32] = Sha256::digest(bytes).into();
        assert_eq!(snapshot.fingerprint().unwrap(), canonical_hash);
        assert_eq!(snapshot.tree.validate().unwrap(), snapshot.tree.len());
        let mut tree = snapshot.tree.clone();
        let next = Key::Text("fuzz-synthetic".into());
        let pointer = RecordPointer {
            page_id: 1,
            slot_id: 0,
        };
        let _ = tree.insert(next.clone(), pointer);
        if let Ok(delta) = snapshot.delta_to(&tree) {
            assert_eq!(delta.apply(&snapshot).unwrap().tree, tree);
        }
        let rows = snapshot.tree.range(None, None, MAX_INDEX_ENTRIES).unwrap();
        if let Some((key, _)) = rows.first() {
            let mut removed = snapshot.tree.clone();
            removed.remove(key).unwrap();
            if let Ok(delta) = snapshot.delta_to(&removed) {
                assert_eq!(delta.apply(&snapshot).unwrap().tree, removed);
            }
        }
    }
}

fuzz_target!(|bytes: &[u8]| {
    check(bytes);
    if bytes.len() < 2 * PAGE_SIZE
        || bytes.len() > 17 * PAGE_SIZE
        || !bytes.len().is_multiple_of(PAGE_SIZE)
    {
        return;
    }
    let mut repaired = bytes.to_vec();
    for image in repaired.as_chunks_mut::<PAGE_SIZE>().0 {
        let mut crc = crc32fast::Hasher::new();
        crc.update(&image[..60]);
        crc.update(&image[64..]);
        image[60..64].copy_from_slice(&crc.finalize().to_le_bytes());
    }
    check(&repaired);
    // Canonical sparse-tree seeds also exercise mutations and whole-delta replay.
    let tree = BPlusTree::new_stable();
    let snapshot = IndexSnapshot { revision: 1, tree };
    assert_eq!(
        IndexSnapshot::decode(&snapshot.encode().unwrap()).unwrap(),
        snapshot
    );
});
