use emilybase_wal::{FRAME_SIZE, HEADER_SIZE, encode_header};

/// Repair integrity fields so mutations also reach structural/history checks.
pub fn repaired_wal(bytes: &[u8]) -> Option<Vec<u8>> {
    if !(HEADER_SIZE..=20000).contains(&bytes.len()) {
        return None;
    }
    let mut repaired = bytes.to_vec();
    repaired[..HEADER_SIZE].copy_from_slice(&encode_header([7; 16]).unwrap());
    if bytes[8..10] == 2u16.to_le_bytes() {
        repaired[8..10].copy_from_slice(&2u16.to_le_bytes());
        repaired[32..44].copy_from_slice(&bytes[32..44]);
        let checksum = crc32fast::hash(&repaired[..60]);
        repaired[60..64].copy_from_slice(&checksum.to_le_bytes());
    }
    let mut digest = crc32fast::Hasher::new();
    let (frames, _) = repaired[HEADER_SIZE..].as_chunks_mut::<FRAME_SIZE>();
    for frame in frames {
        if matches!(frame[6], 1 | 3) {
            let mut page_crc = crc32fast::Hasher::new();
            page_crc.update(&frame[60..88]);
            page_crc.update(&frame[92..4156]);
            frame[88..92].copy_from_slice(&page_crc.finalize().to_le_bytes());
        } else if matches!(frame[6], 2 | 4) {
            frame[64..68].copy_from_slice(&digest.clone().finalize().to_le_bytes());
        }
        let crc = crc32fast::hash(&frame[..4156]);
        frame[4156..].copy_from_slice(&crc.to_le_bytes());
        if matches!(frame[6], 1 | 3) {
            digest.update(frame);
        } else if matches!(frame[6], 2 | 4) {
            digest = crc32fast::Hasher::new();
        }
    }
    Some(repaired)
}
