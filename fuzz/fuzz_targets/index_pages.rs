#![no_main]
#![forbid(unsafe_code)]
use emilybase_index::{BPlusTree, IndexPage, MAX_INDEX_ENTRIES, PAGE_SIZE};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let _ = IndexPage::decode(bytes, 1);
    if bytes.len() < 8 || bytes.len() > 8 + 16 * PAGE_SIZE {
        return;
    }
    let root = u64::from_le_bytes(bytes[..8].try_into().unwrap());
    let mut images = Vec::new();
    for (i, chunk) in bytes[8..].as_chunks::<PAGE_SIZE>().0.iter().enumerate() {
        let mut image = *chunk;
        let _ = IndexPage::decode(&image, i as u64 + 1);
        let mut crc = crc32fast::Hasher::new();
        crc.update(&image[..60]);
        crc.update(&image[64..]);
        image[60..64].copy_from_slice(&crc.finalize().to_le_bytes());
        if let Ok(page) = IndexPage::decode(&image, i as u64 + 1) {
            assert_eq!(page.encode().unwrap(), image);
        }
        images.push(image);
    }
    if let Ok(tree) = BPlusTree::from_pages(root, &images) {
        assert_eq!(tree.page_images().unwrap(), images);
        let entries = tree.range(None, None, MAX_INDEX_ENTRIES).unwrap();
        assert_eq!(entries.len(), tree.len());
        for (key, value) in entries {
            assert_eq!(tree.get(&key).unwrap(), Some(value));
        }
    }
});
