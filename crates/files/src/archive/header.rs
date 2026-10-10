use super::{FILE_ARCHIVE_HEADER_BYTES, FILE_ARCHIVE_VERSION, MAX_FILE_ARCHIVE_BYTES};
use crate::{Error, Result};
use emilybase_object_storage::ProjectId;
const MAGIC: &[u8; 8] = b"EMILYFBK";

pub(super) struct Header {
    pub database_id: [u8; 16],
    pub last_transaction: u64,
    pub metadata_bytes: usize,
    pub object_bytes: usize,
    pub metadata_hash: [u8; 32],
    pub object_hash: [u8; 32],
}
pub(super) fn encode(project: ProjectId, h: &Header) -> [u8; FILE_ARCHIVE_HEADER_BYTES] {
    let mut bytes = [0; FILE_ARCHIVE_HEADER_BYTES];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..10].copy_from_slice(&FILE_ARCHIVE_VERSION.to_le_bytes());
    bytes[10..12].copy_from_slice(&(FILE_ARCHIVE_HEADER_BYTES as u16).to_le_bytes());
    bytes[16..32].copy_from_slice(project.as_bytes());
    bytes[32..48].copy_from_slice(&h.database_id);
    bytes[48..56].copy_from_slice(&h.last_transaction.to_le_bytes());
    bytes[56..64].copy_from_slice(&(h.metadata_bytes as u64).to_le_bytes());
    bytes[64..72].copy_from_slice(&(h.object_bytes as u64).to_le_bytes());
    bytes[72..104].copy_from_slice(&h.metadata_hash);
    bytes[104..136].copy_from_slice(&h.object_hash);
    let crc = crc32fast::hash(&bytes[..188]);
    bytes[188..].copy_from_slice(&crc.to_le_bytes());
    bytes
}
pub(super) fn decode(bytes: &[u8], project: ProjectId) -> Result<Header> {
    if bytes.len() < FILE_ARCHIVE_HEADER_BYTES || bytes.len() > MAX_FILE_ARCHIVE_BYTES {
        return Err(Error::Archive);
    }
    let mut fixed = [0; FILE_ARCHIVE_HEADER_BYTES];
    fixed.copy_from_slice(&bytes[..FILE_ARCHIVE_HEADER_BYTES]);
    if &fixed[..8] != MAGIC {
        return Err(Error::Archive);
    }
    let version = u16::from_le_bytes([fixed[8], fixed[9]]);
    if version != FILE_ARCHIVE_VERSION {
        return Err(Error::ArchiveVersion(version));
    }
    if u16::from_le_bytes([fixed[10], fixed[11]]) as usize != FILE_ARCHIVE_HEADER_BYTES
        || fixed[12..16].iter().any(|b| *b != 0)
        || fixed[136..188].iter().any(|b| *b != 0)
    {
        return Err(Error::Archive);
    }
    let mut crc = [0; 4];
    crc.copy_from_slice(&fixed[188..]);
    if crc32fast::hash(&fixed[..188]) != u32::from_le_bytes(crc) {
        return Err(Error::ArchiveChecksum);
    }
    if &fixed[16..32] != project.as_bytes() {
        return Err(Error::Scope);
    }
    let mut database_id = [0; 16];
    database_id.copy_from_slice(&fixed[32..48]);
    let number = |offset| {
        let mut value = [0; 8];
        value.copy_from_slice(&fixed[offset..offset + 8]);
        u64::from_le_bytes(value)
    };
    let last_transaction = number(48);
    let metadata = number(56);
    let objects = number(64);
    if last_transaction == 0
        || metadata < emilybase_backup::HEADER_SIZE as u64
        || metadata > emilybase_backup::MAX_BACKUP_BYTES as u64
        || objects < emilybase_object_storage::ARCHIVE_HEADER_BYTES as u64
        || objects > emilybase_object_storage::MAX_ARCHIVE_BYTES as u64
        || FILE_ARCHIVE_HEADER_BYTES as u64 + metadata + objects != bytes.len() as u64
    {
        return Err(Error::Archive);
    }
    let mut metadata_hash = [0; 32];
    metadata_hash.copy_from_slice(&fixed[72..104]);
    let mut object_hash = [0; 32];
    object_hash.copy_from_slice(&fixed[104..136]);
    Ok(Header {
        database_id,
        last_transaction,
        metadata_bytes: metadata as usize,
        object_bytes: objects as usize,
        metadata_hash,
        object_hash,
    })
}
