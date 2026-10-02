#![no_main]
#![forbid(unsafe_code)]

use emilybase_storage::{PAGE_SIZE, Page, header};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = header::decode(data);
    let _ = Page::decode(data, 1);
    if data.len() == PAGE_SIZE {
        // Repair only CRC to exercise structure checks past checksum rejection.
        let mut bytes = [0; PAGE_SIZE];
        bytes.copy_from_slice(data);
        let crc = crc32fast::hash(&bytes[..PAGE_SIZE - 4]);
        bytes[PAGE_SIZE - 4..].copy_from_slice(&crc.to_le_bytes());
        let _ = header::decode(&bytes);
        bytes.copy_from_slice(data);
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&bytes[..28]);
        hasher.update(&bytes[32..]);
        bytes[28..32].copy_from_slice(&hasher.finalize().to_le_bytes());
        if let Ok(page) = Page::decode(&bytes, 1) {
            assert_eq!(Page::decode(&page.encode(), 1).unwrap(), page);
        }
    }
});
