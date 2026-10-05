#![no_main]
#![forbid(unsafe_code)]

use emilybase_commit_format::{ADDRESS_BYTES, PageAddress, ROOT_BYTES, RootBinding};
use libfuzzer_sys::fuzz_target;

fn address(bytes: &[u8]) {
    if let Ok(value) = PageAddress::decode(bytes) {
        assert_eq!(value.encode().unwrap().as_slice(), bytes);
    }
}
fn root(bytes: &[u8]) {
    if let Ok(value) = RootBinding::decode(bytes) {
        assert_eq!(value.encode().unwrap().as_slice(), bytes);
        let selected = value.address();
        value
            .verify_owner(selected.database(), selected.table(), value.transaction())
            .unwrap();
        assert!(
            value
                .verify_owner([0; 16], selected.table(), value.transaction())
                .is_err()
        );
    }
}

fuzz_target!(|bytes: &[u8]| {
    if bytes.len() > 4096 {
        return;
    }
    address(bytes);
    root(bytes);
    // Repair envelopes to reach namespace/root field admission beyond CRC checks.
    for (size, magic, decode) in [
        (ADDRESS_BYTES, b"EBNS\0\0\0\0", address as fn(&[u8])),
        (ROOT_BYTES, b"EBIR\0\0\0\0", root as fn(&[u8])),
    ] {
        if bytes.len() == size {
            let mut repaired = bytes.to_vec();
            repaired[..8].copy_from_slice(magic);
            repaired[8..10].copy_from_slice(&1u16.to_le_bytes());
            let checksum = crc32fast::hash(&repaired[..size - 4]);
            repaired[size - 4..].copy_from_slice(&checksum.to_le_bytes());
            decode(&repaired);
        }
    }
});
