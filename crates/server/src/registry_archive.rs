//! Bounded canonical registry images; no filesystem access during inspection.
use crate::metadata::{self, Metadata};
use crate::{Error, MAX_METADATA_BYTES, MAX_PROJECTS, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const REGISTRY_BACKUP_VERSION: u16 = 1;
pub const MAX_REGISTRY_BACKUP_BYTES: usize = 128 * 1024 * 1024;
pub(crate) const HEADER: usize = 128;
pub(crate) const ENTRY_HEADER: usize = 48;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RegistryProjectReport {
    pub id: String,
    pub name: String,
    pub key_epoch: u64,
    pub database_id: [u8; 16],
    pub transaction: u64,
    pub wal_version: u16,
    pub tables: usize,
    pub rows: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RegistryBackupReport {
    pub projects: Vec<RegistryProjectReport>,
    pub archive_bytes: usize,
}

pub(crate) struct Entry<'a> {
    pub metadata: Metadata,
    pub database: &'a [u8],
}
pub(crate) struct Archive<'a> {
    pub entries: Vec<Entry<'a>>,
    pub report: RegistryBackupReport,
}

/// Inspect checksums, canonical metadata, unique identities and every nested WAL.
/// Returns public metadata/counts, never stored key digests or row contents.
pub fn inspect_registry_backup_bytes(bytes: &[u8]) -> Result<RegistryBackupReport> {
    Ok(decode(bytes)?.report)
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Archive<'_>> {
    if bytes.len() > MAX_REGISTRY_BACKUP_BYTES {
        return Err(Error::Limit);
    }
    let header = bytes
        .get(..HEADER)
        .ok_or(Error::RegistryFormat("truncated header"))?;
    if &header[..8] != b"EMILYREG" {
        return Err(Error::RegistryFormat("magic"));
    }
    let version = u16::from_le_bytes([header[8], header[9]]);
    if version != REGISTRY_BACKUP_VERSION {
        return Err(Error::RegistryVersion(version));
    }
    if u16::from_le_bytes([header[10], header[11]]) as usize != HEADER
        || header[56..124].iter().any(|b| *b != 0)
    {
        return Err(Error::RegistryFormat("header size or reserved bytes"));
    }
    if u32_at(header, 124) != crc32fast::hash(&header[..124]) {
        return Err(Error::RegistryChecksum);
    }
    let count = u32_at(header, 12) as usize;
    let payload_bytes = usize::try_from(u64_at(header, 16)).map_err(|_| Error::Limit)?;
    if count > MAX_PROJECTS || payload_bytes != bytes.len() - HEADER {
        return Err(Error::RegistryFormat("project count or payload length"));
    }
    if Sha256::digest(&bytes[HEADER..]).as_slice() != &header[24..56] {
        return Err(Error::RegistryChecksum);
    }
    let mut cursor = Cursor { bytes, at: HEADER };
    let mut entries = Vec::with_capacity(count);
    let mut projects = Vec::with_capacity(count);
    let mut database_ids = BTreeSet::new();
    let mut previous_id: Option<String> = None;
    for _ in 0..count {
        let entry = cursor.take(ENTRY_HEADER)?;
        let id = std::str::from_utf8(&entry[..32])
            .map_err(|_| Error::RegistryFormat("project identity encoding"))?;
        if !emilybase_auth::valid_project_id(id)
            || previous_id.as_deref().is_some_and(|prior| prior >= id)
        {
            return Err(Error::RegistryFormat("project identity or order"));
        }
        let metadata_bytes = u32_at(entry, 32) as usize;
        let database_bytes = usize::try_from(u64_at(entry, 40)).map_err(|_| Error::Limit)?;
        if entry[36..40].iter().any(|b| *b != 0)
            || metadata_bytes == 0
            || metadata_bytes as u64 > MAX_METADATA_BYTES
            || !(emilybase_backup::HEADER_SIZE..=emilybase_backup::MAX_BACKUP_BYTES)
                .contains(&database_bytes)
        {
            return Err(Error::RegistryFormat("entry bounds or reserved bytes"));
        }
        let encoded = cursor.take(metadata_bytes)?;
        let metadata = metadata::decode(encoded, id)?;
        if metadata::encoded(&metadata)? != encoded {
            return Err(Error::RegistryFormat("noncanonical metadata"));
        }
        let database = cursor.take(database_bytes)?;
        let recovered = emilybase_backup::inspect_bytes(database)?;
        if !database_ids.insert(recovered.database_id) {
            return Err(Error::RegistryFormat("duplicate database identity"));
        }
        previous_id = Some(id.to_owned());
        projects.push(RegistryProjectReport {
            id: metadata.id.clone(),
            name: metadata.name.clone(),
            key_epoch: metadata.epoch,
            database_id: recovered.database_id,
            transaction: recovered.last_transaction,
            wal_version: recovered.wal_version,
            tables: recovered.tables,
            rows: recovered.rows,
        });
        entries.push(Entry { metadata, database });
    }
    if cursor.at != bytes.len() {
        return Err(Error::RegistryFormat("trailing payload"));
    }
    Ok(Archive {
        entries,
        report: RegistryBackupReport {
            projects,
            archive_bytes: bytes.len(),
        },
    })
}

pub(crate) fn append(bytes: &mut Vec<u8>, metadata: &Metadata, database: &[u8]) -> Result<()> {
    let metadata_bytes = metadata::encoded(metadata)?;
    let additional = ENTRY_HEADER
        .checked_add(metadata_bytes.len())
        .and_then(|size| size.checked_add(database.len()))
        .ok_or(Error::Limit)?;
    if bytes
        .len()
        .checked_add(additional)
        .is_none_or(|size| size > MAX_REGISTRY_BACKUP_BYTES)
    {
        return Err(Error::Limit);
    }
    let mut entry = [0; ENTRY_HEADER];
    if !emilybase_auth::valid_project_id(&metadata.id) {
        return Err(Error::Metadata);
    }
    entry[..32].copy_from_slice(metadata.id.as_bytes());
    entry[32..36].copy_from_slice(&(metadata_bytes.len() as u32).to_le_bytes());
    entry[40..48].copy_from_slice(&(database.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&entry);
    bytes.extend_from_slice(&metadata_bytes);
    bytes.extend_from_slice(database);
    Ok(())
}

pub(crate) fn finish(bytes: &mut [u8], count: usize) -> Result<()> {
    if !(HEADER..=MAX_REGISTRY_BACKUP_BYTES).contains(&bytes.len()) || count > MAX_PROJECTS {
        return Err(Error::Limit);
    }
    let digest = Sha256::digest(&bytes[HEADER..]);
    let payload = (bytes.len() - HEADER) as u64;
    let header = &mut bytes[..HEADER];
    header.fill(0);
    header[..8].copy_from_slice(b"EMILYREG");
    header[8..10].copy_from_slice(&REGISTRY_BACKUP_VERSION.to_le_bytes());
    header[10..12].copy_from_slice(&(HEADER as u16).to_le_bytes());
    header[12..16].copy_from_slice(&(count as u32).to_le_bytes());
    header[16..24].copy_from_slice(&payload.to_le_bytes());
    header[24..56].copy_from_slice(&digest);
    let crc = crc32fast::hash(&header[..124]);
    header[124..].copy_from_slice(&crc.to_le_bytes());
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(length).ok_or(Error::Limit)?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or(Error::RegistryFormat("truncated entry"))?;
        self.at = end;
        Ok(slice)
    }
}

// Private numeric readers are called only on validated fixed-size headers.
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    let mut value = [0; 4];
    value.copy_from_slice(&bytes[at..at + 4]);
    u32::from_le_bytes(value)
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    let mut value = [0; 8];
    value.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(value)
}

#[cfg(test)]
mod tests;
