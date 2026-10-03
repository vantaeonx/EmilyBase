#![no_main]
#![forbid(unsafe_code)]

use emilybase_backup::{HEADER_SIZE, inspect_bytes};
use emilybase_wal::FRAME_SIZE;
use libfuzzer_sys::fuzz_target;
use sha2::{Digest, Sha256};
mod support;

fn repair_envelope(bytes: &mut [u8]) {
    let digest = Sha256::digest(&bytes[HEADER_SIZE..]);
    bytes[48..80].copy_from_slice(&digest);
    let crc = crc32fast::hash(&bytes[..124]);
    bytes[124..128].copy_from_slice(&crc.to_le_bytes());
}

fuzz_target!(|bytes: &[u8]| {
    let _ = inspect_bytes(bytes);
    if !(HEADER_SIZE..=32768).contains(&bytes.len()) {
        return;
    }
    let mut repaired = bytes.to_vec();
    repair_envelope(&mut repaired);
    let _ = inspect_bytes(&repaired);
    if let Some(wal) = support::repaired_wal(&bytes[HEADER_SIZE..]) {
        let transactions = wal[emilybase_wal::HEADER_SIZE..]
            .as_chunks::<FRAME_SIZE>()
            .0
            .iter()
            .filter(|frame| frame[6] == 2)
            .count() as u64;
        let mut envelope = vec![0; HEADER_SIZE];
        envelope[..16].copy_from_slice(b"EMILYBAK\x01\0\x80\0\x01\0\x01\0");
        envelope[16..32].copy_from_slice(&[7; 16]);
        envelope[32..40].copy_from_slice(&transactions.to_le_bytes());
        envelope[40..48].copy_from_slice(&(wal.len() as u64).to_le_bytes());
        envelope.extend_from_slice(&wal);
        repair_envelope(&mut envelope);
        let _ = inspect_bytes(&envelope);
    }
});
