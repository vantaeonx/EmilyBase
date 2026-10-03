#![no_main]
#![forbid(unsafe_code)]
use emilybase_server::inspect_registry_backup_bytes;
use libfuzzer_sys::fuzz_target;
use sha2::{Digest, Sha256};

fuzz_target!(|bytes: &[u8]| {
    let _ = inspect_registry_backup_bytes(bytes);
    if !(128..=32768).contains(&bytes.len()) {
        return;
    }
    let mut repaired = bytes.to_vec();
    repaired[16..24].copy_from_slice(&((bytes.len() - 128) as u64).to_le_bytes());
    let digest = Sha256::digest(&repaired[128..]);
    repaired[24..56].copy_from_slice(&digest);
    let crc = crc32fast::hash(&repaired[..124]);
    repaired[124..128].copy_from_slice(&crc.to_le_bytes());
    let _ = inspect_registry_backup_bytes(&repaired);
});
