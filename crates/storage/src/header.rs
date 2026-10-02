//! Encoding and validation of the immutable database header.
use crate::{Error, FORMAT_VERSION, PAGE_SIZE, Result};

const MAGIC: &[u8; 8] = b"EMILYDB\0";
const CHECKSUM_OFFSET: usize = PAGE_SIZE - 4;

pub fn encode() -> [u8; PAGE_SIZE] {
    let mut bytes = [0; PAGE_SIZE];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..10].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    bytes[10..12].copy_from_slice(&(PAGE_SIZE as u16).to_le_bytes());
    let checksum = crc32fast::hash(&bytes[..CHECKSUM_OFFSET]);
    bytes[CHECKSUM_OFFSET..].copy_from_slice(&checksum.to_le_bytes());
    bytes
}

pub fn decode(bytes: &[u8]) -> Result<()> {
    if bytes.len() != PAGE_SIZE {
        return Err(Error::Layout("header length"));
    }
    if &bytes[..8] != MAGIC {
        return Err(Error::Magic);
    }
    let version = u16_at(bytes, 8);
    if version != FORMAT_VERSION {
        return Err(Error::Version(version));
    }
    let size = u16_at(bytes, 10);
    if usize::from(size) != PAGE_SIZE {
        return Err(Error::PageSize(size));
    }
    if u32_at(bytes, CHECKSUM_OFFSET) != crc32fast::hash(&bytes[..CHECKSUM_OFFSET]) {
        return Err(Error::Checksum);
    }
    if bytes[12..CHECKSUM_OFFSET].iter().any(|&b| b != 0) {
        return Err(Error::Layout("nonzero reserved header bytes"));
    }
    Ok(())
}

// Callers check fixed buffer lengths before using these private helpers.
pub(crate) fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

pub(crate) fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trip_and_corruption() {
        let mut bytes = encode();
        decode(&bytes).unwrap();
        assert_eq!(&bytes[..12], b"EMILYDB\0\x01\0\0\x10");
        bytes[100] ^= 1;
        assert!(matches!(decode(&bytes), Err(Error::Checksum)));
        assert!(decode(&bytes[..100]).is_err());
    }
}
