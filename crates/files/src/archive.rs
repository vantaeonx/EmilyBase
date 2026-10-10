//! Experimental paired archive bytes and native publication, never user authority.
use crate::{Error, FileInfo, FileQuota, Result, records};
use emilybase_object_storage::{ProjectId, VerifiedArchive};
use sha2::{Digest, Sha256};
mod header;
mod initialize;
mod publication;
mod reader;
mod restore;
mod source;
pub use initialize::initialize_file_root;
pub use publication::{FileArchiveReport, inspect_file_archive, publish_file_archive};
pub use reader::FileArchiveReader;
pub use restore::restore_file_archive;
pub use source::restore_file_archive_file;

pub const FILE_ARCHIVE_VERSION: u16 = 1;
pub const FILE_ARCHIVE_HEADER_BYTES: usize = 192;
pub const MAX_FILE_ARCHIVE_BYTES: usize = FILE_ARCHIVE_HEADER_BYTES
    + emilybase_backup::MAX_BACKUP_BYTES
    + emilybase_object_storage::MAX_ARCHIVE_BYTES;

/// A checked borrowed image, never a current user capability, source owner,
/// unique-ancestry proof or durable publication receipt. Input bytes are sensitive.
pub struct VerifiedFileArchive<'a> {
    project: ProjectId,
    metadata: &'a [u8],
    object_bytes: &'a [u8],
    report: emilybase_backup::Report,
    quota: FileQuota,
    files: Vec<FileInfo>,
    objects: VerifiedArchive<'a>,
}
impl VerifiedFileArchive<'_> {
    pub const fn project(&self) -> ProjectId {
        self.project
    }
    pub fn metadata_bytes(&self) -> &[u8] {
        self.metadata
    }
    pub const fn metadata_report(&self) -> &emilybase_backup::Report {
        &self.report
    }
    pub const fn quota(&self) -> FileQuota {
        self.quota
    }
    pub fn files(&self) -> &[FileInfo] {
        &self.files
    }
    pub const fn objects(&self) -> &VerifiedArchive<'_> {
        &self.objects
    }
}
impl std::fmt::Debug for VerifiedFileArchive<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedFileArchive")
            .field("references", &self.files.len())
            .field("objects", &self.objects.objects().len())
            .field("payload_bytes", &self.objects.payload_bytes())
            .finish_non_exhaustive()
    }
}

/// Check fixed bounds/header CRC and both outer SHA-256 digests before nested
/// replay. Exact schema, project/database/revision, physical quota and every
/// logical reference-to-object hash/length must agree. Canonical nested formats
/// and exact partitioning forbid gaps, duplicates, overlap and trailing data.
/// This does not authenticate a backup's origin or authorize any restore target.
pub fn verify_file_archive(bytes: &[u8], project: ProjectId) -> Result<VerifiedFileArchive<'_>> {
    let header = header::decode(bytes, project)?;
    let middle = FILE_ARCHIVE_HEADER_BYTES + header.metadata_bytes;
    let metadata = &bytes[FILE_ARCHIVE_HEADER_BYTES..middle];
    let object_bytes = &bytes[middle..];
    if <[u8; 32]>::from(Sha256::digest(metadata)) != header.metadata_hash
        || <[u8; 32]>::from(Sha256::digest(object_bytes)) != header.object_hash
    {
        return Err(Error::ArchiveChecksum);
    }
    let backup = emilybase_backup::decode_verified(metadata)?;
    let image = backup.image();
    if image.database_id != header.database_id || image.last_transaction != header.last_transaction
    {
        return Err(Error::Scope);
    }
    let (quota, files) = records::metadata_image(
        &image.snapshot,
        image.database_id,
        image.last_transaction,
        project,
    )?;
    let report = backup.report().clone();
    drop(backup);
    let objects = emilybase_object_storage::verify_archive(object_bytes, project)?;
    if objects.objects().len() > quota.objects() || objects.payload_bytes() > quota.payload_bytes()
    {
        return Err(Error::Quota);
    }
    for info in &files {
        let object = objects
            .objects()
            .iter()
            .find(|o| o.object() == info.object())
            .ok_or(Error::Corrupt)?;
        if object.payload().len() != info.report().payload_bytes
            || object.sha256() != &info.report().sha256
        {
            return Err(Error::Corrupt);
        }
    }
    Ok(VerifiedFileArchive {
        project,
        metadata,
        object_bytes,
        report,
        quota,
        files,
        objects,
    })
}

#[cfg(test)]
mod tests;
