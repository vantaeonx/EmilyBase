#![no_main]
#![forbid(unsafe_code)]

use emilybase_database::{MAX_ROWS, Snapshot};
use emilybase_index::IndexSnapshot;
use emilybase_transactions::{INDEX_IMAGE_HEADER, inspect_primary_index_image, recover_snapshot};
use libfuzzer_sys::fuzz_target;
use sha2::{Digest, Sha256};
mod support;

fn inspect(bytes: &[u8]) {
    if let Ok(report) = inspect_primary_index_image(bytes) {
        let index = IndexSnapshot::decode(&bytes[INDEX_IMAGE_HEADER..]).unwrap();
        assert_eq!(index.encode().unwrap(), &bytes[INDEX_IMAGE_HEADER..]);
        assert_eq!(report.transaction, index.revision);
        assert_eq!(report.entries, index.tree.validate().unwrap());
        assert_eq!(report.pages, index.tree.page_count());
        assert_eq!(report.root_id, index.tree.root_id());
    }
}
fn project(mut snapshot: Snapshot) {
    let before = snapshot.page_fingerprint();
    for schema in snapshot.schemas() {
        let rows = snapshot.scan(&schema.name, MAX_ROWS).unwrap();
        let tree = snapshot.export_primary_tree(&schema.name).unwrap();
        let encoded = IndexSnapshot { revision: 1, tree }.encode().unwrap();
        let tree = IndexSnapshot::decode(&encoded).unwrap().tree;
        snapshot.install_primary_tree(&schema.name, tree).unwrap();
        for row in rows.iter().take(32) {
            let key = schema.key(row).unwrap();
            assert_eq!(snapshot.get(&schema.name, &key).unwrap(), Some(row));
        }
    }
    assert_eq!(snapshot.page_fingerprint(), before);
}

fuzz_target!(|bytes: &[u8]| {
    if bytes.len() > 32768 {
        return;
    }
    inspect(bytes);
    if bytes.len() >= INDEX_IMAGE_HEADER + 8192
        && (bytes.len() - INDEX_IMAGE_HEADER).is_multiple_of(4096)
    {
        // Repaired checksums allow mutations to reach nested topology and binding checks.
        let mut repaired = bytes.to_vec();
        for image in repaired[INDEX_IMAGE_HEADER..].as_chunks_mut::<4096>().0 {
            let mut crc = crc32fast::Hasher::new();
            crc.update(&image[..60]);
            crc.update(&image[64..]);
            image[60..64].copy_from_slice(&crc.finalize().to_le_bytes());
        }
        let size = (repaired.len() - INDEX_IMAGE_HEADER) as u64;
        repaired[80..88].copy_from_slice(&size.to_le_bytes());
        let hash = Sha256::digest(&repaired[INDEX_IMAGE_HEADER..]);
        repaired[88..120].copy_from_slice(&hash);
        let crc = crc32fast::hash(&repaired[..124]);
        repaired[124..128].copy_from_slice(&crc.to_le_bytes());
        inspect(&repaired);
    }
    if let Ok(snapshot) = recover_snapshot(bytes, None) {
        project(snapshot);
    }
    if let Some(repaired) = support::repaired_wal(bytes)
        && let Ok(snapshot) = recover_snapshot(&repaired, Some([7; 16]))
    {
        project(snapshot);
    }
});
