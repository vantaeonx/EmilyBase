use super::{ARCHIVE_HEADER_BYTES, MAGIC, MAX_ARCHIVE_BYTES};
use crate::{Error, MAX_INVENTORY_BYTES, MAX_INVENTORY_OBJECTS, ProjectId, Result};

pub(super) struct Header {
    pub count: usize,
    pub payload_bytes: u64,
    pub body_bytes: usize,
    pub digest: [u8; 32],
    pub body_sha256: [u8; 32],
}

pub(super) fn encode(project: ProjectId, header: &Header) -> [u8; ARCHIVE_HEADER_BYTES] {
    let mut bytes = [0; ARCHIVE_HEADER_BYTES];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
    bytes[12..16].copy_from_slice(&(ARCHIVE_HEADER_BYTES as u32).to_le_bytes());
    bytes[16..32].copy_from_slice(project.as_bytes());
    bytes[32..36].copy_from_slice(&(header.count as u32).to_le_bytes());
    bytes[40..48].copy_from_slice(&header.payload_bytes.to_le_bytes());
    bytes[48..56].copy_from_slice(&(header.body_bytes as u64).to_le_bytes());
    bytes[56..88].copy_from_slice(&header.digest);
    bytes[88..120].copy_from_slice(&header.body_sha256);
    let crc = crc32fast::hash(&bytes[..124]);
    bytes[124..128].copy_from_slice(&crc.to_le_bytes());
    bytes
}

pub(super) fn check_total(total: usize) -> Result<()> {
    if total > MAX_ARCHIVE_BYTES {
        return Err(Error::Limit);
    }
    if total < ARCHIVE_HEADER_BYTES {
        return Err(Error::Archive);
    }
    Ok(())
}

pub(super) fn decode(
    bytes: &[u8; ARCHIVE_HEADER_BYTES],
    total: usize,
    project: ProjectId,
) -> Result<Header> {
    check_total(total)?;
    let u32_at = |start| {
        u32::from_le_bytes([
            bytes[start],
            bytes[start + 1],
            bytes[start + 2],
            bytes[start + 3],
        ])
    };
    let u64_at = |start| {
        u64::from_le_bytes([
            bytes[start],
            bytes[start + 1],
            bytes[start + 2],
            bytes[start + 3],
            bytes[start + 4],
            bytes[start + 5],
            bytes[start + 6],
            bytes[start + 7],
        ])
    };
    if crc32fast::hash(&bytes[..124]) != u32_at(124) {
        return Err(Error::ArchiveChecksum);
    }
    if &bytes[..8] != MAGIC {
        return Err(Error::Archive);
    }
    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if version != 1 {
        return Err(Error::ArchiveVersion(version));
    }
    if bytes[10..12] != [0; 2]
        || u32_at(12) != ARCHIVE_HEADER_BYTES as u32
        || bytes[36..40] != [0; 4]
        || bytes[120..124] != [0; 4]
    {
        return Err(Error::Archive);
    }
    if &bytes[16..32] != project.as_bytes() {
        return Err(Error::Scope);
    }
    let count = u32_at(32) as usize;
    let payload_bytes = u64_at(40);
    let body_bytes = u64_at(48);
    if count > MAX_INVENTORY_OBJECTS
        || payload_bytes > MAX_INVENTORY_BYTES
        || body_bytes > (MAX_ARCHIVE_BYTES - ARCHIVE_HEADER_BYTES) as u64
    {
        return Err(Error::Limit);
    }
    if body_bytes != (total - ARCHIVE_HEADER_BYTES) as u64 {
        return Err(Error::Archive);
    }
    let mut digest = [0; 32];
    digest.copy_from_slice(&bytes[56..88]);
    let mut body_sha256 = [0; 32];
    body_sha256.copy_from_slice(&bytes[88..120]);
    Ok(Header {
        count,
        payload_bytes,
        body_bytes: body_bytes as usize,
        digest,
        body_sha256,
    })
}
