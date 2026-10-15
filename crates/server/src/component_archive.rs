//! Canonical immutable component envelope; no publication or common restore.
use crate::{
    AccountBundleReport, AccountFileRootManifest, Error, MAX_ACCOUNT_BUNDLE_BYTES,
    MAX_ACCOUNT_FILE_ROOT_MANIFEST_BYTES, MAX_COMPONENT_FILE_STORES, MAX_COMPONENT_IMAGE_BYTES,
    MAX_COMPONENT_OBJECTS, Result, account_bundle,
    component_snapshot::{charge, hex, unique_ids},
    inspect_account_file_root_manifest_bytes,
};
use emilybase_files::{FILE_ARCHIVE_HEADER_BYTES, MAX_FILE_ARCHIVE_BYTES, VerifiedFileArchive};
use emilybase_object_storage::{ARCHIVE_HEADER_BYTES, HEADER_BYTES, ProjectId};
use sha2::{Digest, Sha256};
pub(crate) mod publication;
mod reader;
pub use reader::AccountFileArchiveReader;

pub const ACCOUNT_FILE_ARCHIVE_VERSION: u16 = 1;
const HEADER: usize = 128;
const FRAME: usize = 48;
const MAGIC: &[u8; 8] = b"EMILYAFB";
/// Exact worst-case framing beyond the separately bounded component images.
pub const MAX_ACCOUNT_FILE_ARCHIVE_BYTES: usize = MAX_COMPONENT_IMAGE_BYTES
    + HEADER
    + MAX_COMPONENT_FILE_STORES * (FRAME + FILE_ARCHIVE_HEADER_BYTES + ARCHIVE_HEADER_BYTES)
    + MAX_COMPONENT_OBJECTS * 24;

/// Borrowed verified sensitive bytes, not source owners, provenance, current
/// permission, durable publication or a common restored root. No Clone/Serialize.
pub struct VerifiedAccountFileArchive<'a> {
    manifest: AccountFileRootManifest,
    manifest_bytes: &'a [u8],
    accounts: &'a [u8],
    account_report: AccountBundleReport,
    files: Vec<VerifiedFileArchive<'a>>,
    file_bytes: Vec<&'a [u8]>,
    copied_bytes: usize,
    objects: usize,
    length: usize,
}
impl VerifiedAccountFileArchive<'_> {
    pub fn manifest(&self) -> &AccountFileRootManifest {
        &self.manifest
    }
    pub fn account_bytes(&self) -> &[u8] {
        self.accounts
    }
    pub fn account_report(&self) -> &AccountBundleReport {
        &self.account_report
    }
    pub fn files(&self) -> &[VerifiedFileArchive<'_>] {
        &self.files
    }
    pub fn file_bytes(&self, index: usize) -> Option<&[u8]> {
        self.file_bytes.get(index).copied()
    }
    pub const fn copied_bytes(&self) -> usize {
        self.copied_bytes
    }
    pub const fn physical_objects(&self) -> usize {
        self.objects
    }
    pub const fn encoded_bytes(&self) -> usize {
        self.length
    }
}
impl std::fmt::Debug for VerifiedAccountFileArchive<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedAccountFileArchive")
            .field("file_stores", &self.files.len())
            .field("physical_objects", &self.objects)
            .field("encoded_bytes", &self.length)
            .finish_non_exhaustive()
    }
}

/// Check outer bounds/CRC/body hash before nested parsing/replay. Exact manifest,
/// account roster, native file scopes/database IDs, aggregate counts/bytes and
/// canonical partitioning must agree. Checksums do not authenticate origin.
pub fn verify_account_file_archive(bytes: &[u8]) -> Result<VerifiedAccountFileArchive<'_>> {
    if bytes.len() > MAX_ACCOUNT_FILE_ARCHIVE_BYTES {
        return Err(Error::Limit);
    }
    let h = bytes
        .get(..HEADER)
        .ok_or(Error::ComponentArchive("truncated header"))?;
    if &h[..8] != MAGIC {
        return Err(Error::ComponentArchive("magic"));
    }
    let version = u16::from_le_bytes([h[8], h[9]]);
    if version != ACCOUNT_FILE_ARCHIVE_VERSION {
        return Err(Error::ComponentArchiveVersion(version));
    }
    if u16::from_le_bytes([h[10], h[11]]) as usize != HEADER
        || h[20..24].iter().chain(&h[72..124]).any(|&b| b != 0)
    {
        return Err(Error::ComponentArchive("header fields"));
    }
    if crc32fast::hash(&h[..124]) != u32_at(h, 124) {
        return Err(Error::ComponentArchiveChecksum);
    }
    let count = u32_at(h, 12) as usize;
    let manifest_length = u32_at(h, 16) as usize;
    let account_length = u64_at(h, 24);
    if count > MAX_COMPONENT_FILE_STORES
        || manifest_length > MAX_ACCOUNT_FILE_ROOT_MANIFEST_BYTES
        || account_length > MAX_ACCOUNT_BUNDLE_BYTES as u64
    {
        return Err(Error::Limit);
    }
    if manifest_length == 0
        || account_length < account_bundle::HEADER as u64
        || u64_at(h, 32) != (bytes.len() - HEADER) as u64
    {
        return Err(Error::ComponentArchive("component lengths"));
    }
    if Sha256::digest(&bytes[HEADER..]).as_slice() != &h[40..72] {
        return Err(Error::ComponentArchiveChecksum);
    }
    let mut at = HEADER;
    let manifest_bytes = take(bytes, &mut at, manifest_length)?;
    let manifest = inspect_account_file_root_manifest_bytes(manifest_bytes)?;
    let accounts = take(bytes, &mut at, account_length as usize)?;
    let account_report = account_bundle::decode(accounts)?.report;
    if manifest.file_stores().len() != count
        || manifest
            .private_projects()
            .iter()
            .map(String::as_str)
            .ne(account_report
                .private_accounts
                .iter()
                .map(|p| p.project.as_str()))
    {
        return Err(Error::ComponentArchive("declared roster"));
    }
    let mut copied_bytes = charge(
        accounts.len(),
        manifest_bytes.len(),
        MAX_COMPONENT_IMAGE_BYTES,
    )?;
    let mut objects = 0;
    let mut files = Vec::with_capacity(count);
    let mut file_bytes = Vec::with_capacity(count);
    for binding in manifest.file_stores() {
        let frame = take(bytes, &mut at, FRAME)?;
        if &frame[..32] != binding.project().as_bytes() || frame[40..].iter().any(|&b| b != 0) {
            return Err(Error::ComponentArchive(
                "file frame scope or reserved fields",
            ));
        }
        let length = u64_at(frame, 32);
        if length > MAX_FILE_ARCHIVE_BYTES as u64 {
            return Err(Error::Limit);
        }
        if length < FILE_ARCHIVE_HEADER_BYTES as u64 {
            return Err(Error::ComponentArchive("file frame length"));
        }
        let image = take(bytes, &mut at, length as usize)?;
        let project: ProjectId = binding
            .project()
            .parse()
            .map_err(|_| Error::ComponentArchive("project"))?;
        let file = emilybase_files::verify_file_archive(image, project)?;
        if hex(&file.metadata_report().database_id) != binding.database_id() {
            return Err(Error::ComponentArchive("file database binding"));
        }
        objects = charge(
            objects,
            file.objects().objects().len(),
            MAX_COMPONENT_OBJECTS,
        )?;
        copied_bytes = charge(
            copied_bytes,
            file.metadata_bytes().len(),
            MAX_COMPONENT_IMAGE_BYTES,
        )?;
        for object in file.objects().objects() {
            copied_bytes = charge(copied_bytes, HEADER_BYTES, MAX_COMPONENT_IMAGE_BYTES)?;
            copied_bytes = charge(
                copied_bytes,
                object.payload().len(),
                MAX_COMPONENT_IMAGE_BYTES,
            )?;
        }
        files.push(file);
        file_bytes.push(image);
    }
    if at != bytes.len() {
        return Err(Error::ComponentArchive("trailing bytes"));
    }
    unique_ids(
        account_report
            .registry
            .projects
            .iter()
            .map(|p| p.database_id)
            .chain(
                account_report
                    .private_accounts
                    .iter()
                    .map(|p| p.inventory.database.database_id),
            )
            .chain(files.iter().map(|p| p.metadata_report().database_id)),
    )?;
    Ok(VerifiedAccountFileArchive {
        manifest,
        manifest_bytes,
        accounts,
        account_report,
        files,
        file_bytes,
        copied_bytes,
        objects,
        length: bytes.len(),
    })
}

fn take<'a>(bytes: &'a [u8], at: &mut usize, length: usize) -> Result<&'a [u8]> {
    let end = at
        .checked_add(length)
        .ok_or(Error::ComponentArchive("length overflow"))?;
    let part = bytes
        .get(*at..end)
        .ok_or(Error::ComponentArchive("truncated component"))?;
    *at = end;
    Ok(part)
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    let mut n = [0; 4];
    n.copy_from_slice(&bytes[at..at + 4]);
    u32::from_le_bytes(n)
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    let mut n = [0; 8];
    n.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(n)
}

#[cfg(test)]
mod tests;
