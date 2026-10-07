//! Bounded registry plus explicit private roster; inspection grants no authority.
use crate::{Error, MAX_PROJECTS, RegistryBackupReport, Result, registry_archive};
use emilybase_auth::accounts::{PrivateArchiveReport, inspect_private_account_backup_bytes};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const ACCOUNT_BUNDLE_VERSION: u16 = 1;
pub const MAX_ACCOUNT_BUNDLE_BYTES: usize = 128 * 1024 * 1024;
pub(crate) const HEADER: usize = 128;
pub(crate) const ENTRY: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledAccountReport {
    pub project: String,
    pub inventory: PrivateArchiveReport,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountBundleReport {
    pub registry: RegistryBackupReport,
    pub private_accounts: Vec<BundledAccountReport>,
    pub archive_bytes: usize,
}
/// Pure bounded integrity, scope and complete nested engine/private validation.
/// Metadata describes supplied contents, not capture provenance or permissions.
pub fn inspect_account_bundle_bytes(bytes: &[u8]) -> Result<AccountBundleReport> {
    if bytes.len() > MAX_ACCOUNT_BUNDLE_BYTES {
        return Err(Error::Limit);
    }
    let header = bytes
        .get(..HEADER)
        .ok_or(Error::BundleFormat("truncated header"))?;
    if &header[..8] != b"EMILYBND" {
        return Err(Error::BundleFormat("magic"));
    }
    let version = u16::from_le_bytes([header[8], header[9]]);
    if version != ACCOUNT_BUNDLE_VERSION {
        return Err(Error::BundleVersion(version));
    }
    if u16::from_le_bytes([header[10], header[11]]) as usize != HEADER
        || header[64..124].iter().any(|b| *b != 0)
    {
        return Err(Error::BundleFormat("header size or reserved bytes"));
    }
    if u32_at(header, 124) != crc32fast::hash(&header[..124]) {
        return Err(Error::BundleChecksum);
    }
    let count = u32_at(header, 12) as usize;
    let registry_len = usize::try_from(u64_at(header, 16)).map_err(|_| Error::Limit)?;
    let payload_len = usize::try_from(u64_at(header, 24)).map_err(|_| Error::Limit)?;
    if count > MAX_PROJECTS
        || payload_len != bytes.len() - HEADER
        || !(registry_archive::HEADER..=crate::MAX_REGISTRY_BACKUP_BYTES).contains(&registry_len)
    {
        return Err(Error::BundleFormat("counts or payload bounds"));
    }
    if Sha256::digest(&bytes[HEADER..]).as_slice() != &header[32..64] {
        return Err(Error::BundleChecksum);
    }
    let mut cursor = Cursor { bytes, at: HEADER };
    let registry = cursor.take(registry_len)?;
    let decoded_registry = registry_archive::decode(registry)?;
    let projects: BTreeSet<_> = decoded_registry
        .report
        .projects
        .iter()
        .map(|p| p.id.as_str())
        .collect();
    let mut database_ids: BTreeSet<_> = decoded_registry
        .report
        .projects
        .iter()
        .map(|p| p.database_id)
        .collect();
    let mut previous: Option<&str> = None;
    let mut reports = Vec::with_capacity(count);
    for _ in 0..count {
        let entry = cursor.take(ENTRY)?;
        let project = std::str::from_utf8(&entry[..32])
            .map_err(|_| Error::BundleFormat("project encoding"))?;
        if !emilybase_auth::valid_project_id(project)
            || !projects.contains(project)
            || previous.is_some_and(|p| p >= project)
        {
            return Err(Error::BundleFormat("project scope or order"));
        }
        let length = usize::try_from(u64_at(entry, 32)).map_err(|_| Error::Limit)?;
        if !(emilybase_backup::HEADER_SIZE..=emilybase_backup::MAX_BACKUP_BYTES).contains(&length) {
            return Err(Error::BundleFormat("private archive bounds"));
        }
        let archive = cursor.take(length)?;
        let inventory = inspect_private_account_backup_bytes(archive, project)?;
        if !database_ids.insert(inventory.database.database_id) {
            return Err(Error::BundleFormat("duplicate database identity"));
        }
        previous = Some(project);
        reports.push(BundledAccountReport {
            project: project.into(),
            inventory,
        });
    }
    if cursor.at != bytes.len() {
        return Err(Error::BundleFormat("trailing payload"));
    }
    Ok(AccountBundleReport {
        registry: decoded_registry.report,
        private_accounts: reports,
        archive_bytes: bytes.len(),
    })
}

pub(crate) fn encode(registry: &[u8], mut accounts: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>> {
    if accounts.len() > MAX_PROJECTS {
        return Err(Error::Limit);
    }
    let count = accounts.len();
    let base = HEADER
        .checked_add(registry.len())
        .filter(|n| *n <= MAX_ACCOUNT_BUNDLE_BYTES)
        .ok_or(Error::Limit)?;
    let total = accounts
        .iter()
        .try_fold(base, |size, (_, bytes)| extend_size(size, bytes.len()))?;
    accounts.sort_by(|a, b| a.0.cmp(&b.0));
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(total).map_err(|_| Error::Limit)?;
    bytes.resize(HEADER, 0);
    bytes.extend_from_slice(registry);
    for (project, archive) in accounts {
        if !emilybase_auth::valid_project_id(&project) {
            return Err(Error::BundleFormat("project identity"));
        }
        bytes.extend_from_slice(project.as_bytes());
        bytes.extend_from_slice(&(archive.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&archive);
    }
    bytes[..8].copy_from_slice(b"EMILYBND");
    bytes[8..10].copy_from_slice(&ACCOUNT_BUNDLE_VERSION.to_le_bytes());
    bytes[10..12].copy_from_slice(&(HEADER as u16).to_le_bytes());
    bytes[12..16].copy_from_slice(&(count as u32).to_le_bytes());
    bytes[16..24].copy_from_slice(&(registry.len() as u64).to_le_bytes());
    let payload_len = (bytes.len() - HEADER) as u64;
    bytes[24..32].copy_from_slice(&payload_len.to_le_bytes());
    let hash = Sha256::digest(&bytes[HEADER..]);
    bytes[32..64].copy_from_slice(&hash);
    let crc = crc32fast::hash(&bytes[..124]);
    bytes[124..128].copy_from_slice(&crc.to_le_bytes());
    inspect_account_bundle_bytes(&bytes)?;
    Ok(bytes)
}
pub(crate) fn extend_size(current: usize, archive: usize) -> Result<usize> {
    current
        .checked_add(ENTRY)
        .and_then(|n| n.checked_add(archive))
        .filter(|n| *n <= MAX_ACCOUNT_BUNDLE_BYTES)
        .ok_or(Error::Limit)
}
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(length).ok_or(Error::Limit)?;
        let result = self
            .bytes
            .get(self.at..end)
            .ok_or(Error::BundleFormat("truncated payload"))?;
        self.at = end;
        Ok(result)
    }
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
        bytes[at + 4],
        bytes[at + 5],
        bytes[at + 6],
        bytes[at + 7],
    ])
}

#[cfg(test)]
mod tests;
