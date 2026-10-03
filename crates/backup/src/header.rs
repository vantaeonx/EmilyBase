use emilybase_storage::FORMAT_VERSION;
use emilybase_wal::{DatabaseId, MAX_WAL_BYTES, WAL_VERSION};

use crate::{BACKUP_VERSION, Error, HEADER_SIZE, Report, Result};

pub(crate) struct Metadata {
    pub id: DatabaseId,
    pub transaction: u64,
    pub wal_bytes: usize,
    pub digest: [u8; 32],
}

pub(crate) fn encode(report: &Report, digest: [u8; 32]) -> [u8; HEADER_SIZE] {
    let mut bytes = [0; HEADER_SIZE];
    bytes[..8].copy_from_slice(b"EMILYBAK");
    bytes[8..10].copy_from_slice(&BACKUP_VERSION.to_le_bytes());
    bytes[10..12].copy_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
    bytes[12..14].copy_from_slice(&WAL_VERSION.to_le_bytes());
    bytes[14..16].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    bytes[16..32].copy_from_slice(&report.database_id);
    bytes[32..40].copy_from_slice(&report.last_transaction.to_le_bytes());
    bytes[40..48].copy_from_slice(&(report.wal_bytes as u64).to_le_bytes());
    bytes[48..80].copy_from_slice(&digest);
    let crc = crc32fast::hash(&bytes[..124]);
    bytes[124..].copy_from_slice(&crc.to_le_bytes());
    bytes
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Metadata> {
    if bytes.len() != HEADER_SIZE || &bytes[..8] != b"EMILYBAK" {
        return Err(Error::Format("header length or magic"));
    }
    let version = u16_at(bytes, 8);
    if version != BACKUP_VERSION {
        return Err(Error::Version(version));
    }
    if u16_at(bytes, 10) as usize != HEADER_SIZE
        || u16_at(bytes, 12) != WAL_VERSION
        || u16_at(bytes, 14) != FORMAT_VERSION
    {
        return Err(Error::Format("header sizes or dependency versions"));
    }
    let mut crc_bytes = [0; 4];
    crc_bytes.copy_from_slice(&bytes[124..]);
    if u32::from_le_bytes(crc_bytes) != crc32fast::hash(&bytes[..124]) {
        return Err(Error::Checksum);
    }
    if bytes[80..124].iter().any(|&b| b != 0) {
        return Err(Error::Format("reserved header bytes"));
    }
    let mut id = [0; 16];
    id.copy_from_slice(&bytes[16..32]);
    let transaction = u64_at(bytes, 32);
    let wal_bytes = usize::try_from(u64_at(bytes, 40)).map_err(|_| Error::Limit)?;
    if id == [0; 16] || transaction == 0 {
        return Err(Error::Format("zero database or transaction ID"));
    }
    if !(emilybase_wal::HEADER_SIZE..=MAX_WAL_BYTES).contains(&wal_bytes) {
        return Err(Error::Limit);
    }
    let mut digest = [0; 32];
    digest.copy_from_slice(&bytes[48..80]);
    Ok(Metadata {
        id,
        transaction,
        wal_bytes,
        digest,
    })
}

// Private readers are called only after the exact header size is checked.
fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    let mut value = [0; 8];
    value.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(value)
}
