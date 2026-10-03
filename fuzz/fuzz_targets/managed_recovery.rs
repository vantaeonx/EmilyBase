#![no_main]
#![forbid(unsafe_code)]

use emilybase_transactions::recover_snapshot;
use emilybase_wal::{FRAME_SIZE, HEADER_SIZE, encode_header};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let _ = recover_snapshot(bytes, None);
    if bytes.len() < HEADER_SIZE || bytes.len() > 20000 {
        return;
    }
    let mut repaired = bytes.to_vec();
    repaired[..HEADER_SIZE].copy_from_slice(&encode_header([7; 16]).unwrap());
    let mut digest = crc32fast::Hasher::new();
    let (frames, _) = repaired[HEADER_SIZE..].as_chunks_mut::<FRAME_SIZE>();
    for frame in frames {
        if frame[6] == 1 {
            let mut page_crc = crc32fast::Hasher::new();
            page_crc.update(&frame[60..88]);
            page_crc.update(&frame[92..4156]);
            frame[88..92].copy_from_slice(&page_crc.finalize().to_le_bytes());
        } else if frame[6] == 2 {
            frame[64..68].copy_from_slice(&digest.clone().finalize().to_le_bytes());
        }
        let crc = crc32fast::hash(&frame[..4156]);
        frame[4156..].copy_from_slice(&crc.to_le_bytes());
        if frame[6] == 1 {
            digest.update(frame);
        } else if frame[6] == 2 {
            digest = crc32fast::Hasher::new();
        }
    }
    let _ = recover_snapshot(&repaired, Some([7; 16]));
});
