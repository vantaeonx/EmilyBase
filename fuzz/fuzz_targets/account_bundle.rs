#![no_main]
#![forbid(unsafe_code)]
use emilybase_server::inspect_account_bundle_bytes;
use libfuzzer_sys::fuzz_target;
use sha2::{Digest, Sha256};

fn seal(bytes: &mut [u8]) {
    let length = (bytes.len() - 128) as u64;
    bytes[24..32].copy_from_slice(&length.to_le_bytes());
    let digest = Sha256::digest(&bytes[128..]);
    bytes[32..64].copy_from_slice(&digest);
    let crc = crc32fast::hash(&bytes[..124]);
    bytes[124..128].copy_from_slice(&crc.to_le_bytes());
}
fuzz_target!(|bytes: &[u8]| {
    let _ = inspect_account_bundle_bytes(bytes);
    if !(128..=262144).contains(&bytes.len()) {
        return;
    }
    let mut repaired = bytes.to_vec();
    seal(&mut repaired);
    let _ = inspect_account_bundle_bytes(&repaired);
    // Seed mutations must also reach nested registry parsing beyond its checksum.
    let length = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
    let Ok(length) = usize::try_from(length) else {
        return;
    };
    let Some(end) = 128_usize.checked_add(length) else {
        return;
    };
    if let Some(registry) = repaired.get_mut(128..end).filter(|r| r.len() >= 128) {
        let payload = (registry.len() - 128) as u64;
        registry[16..24].copy_from_slice(&payload.to_le_bytes());
        let hash = Sha256::digest(&registry[128..]);
        registry[24..56].copy_from_slice(&hash);
        let crc = crc32fast::hash(&registry[..124]);
        registry[124..128].copy_from_slice(&crc.to_le_bytes());
        seal(&mut repaired);
        let _ = inspect_account_bundle_bytes(&repaired);
    }
});
