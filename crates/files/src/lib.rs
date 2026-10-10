//! Standalone native file references in the original engine; no user authority.
mod archive;
mod mutation;
mod quota;
pub use quota::QuotaState;
mod records;
mod snapshot;
mod store;
pub use archive::{
    FILE_ARCHIVE_HEADER_BYTES, FILE_ARCHIVE_VERSION, FileArchiveReader, MAX_FILE_ARCHIVE_BYTES,
    VerifiedFileArchive, verify_file_archive,
};
pub use archive::{
    FileArchiveReport, initialize_file_root, inspect_file_archive, publish_file_archive,
    restore_file_archive, restore_file_archive_file,
};
use emilybase_object_storage::{FileReport, ObjectId, WriteLimits};
pub use mutation::FileRemoval;
pub use snapshot::FileSnapshot;
pub use store::{FileStore, FileUsage};

pub const MAX_FILE_NAME_BYTES: usize = 256;

/// A logical catalog identity, distinct from an immutable object identity.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId([u8; 16]);
impl FileId {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}
impl std::str::FromStr for FileId {
    type Err = Error;
    fn from_str(text: &str) -> Result<Self> {
        let object: ObjectId = text.parse().map_err(|_| Error::Identity)?;
        Ok(Self(*object.as_bytes()))
    }
}
impl std::fmt::Display for FileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&ObjectId::from_bytes(self.0), f)
    }
}
impl std::fmt::Debug for FileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FileId({self})")
    }
}

/// Operator-selected persisted physical payload limits, not a caller request cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileQuota {
    objects: usize,
    payload_bytes: u64,
}
impl FileQuota {
    pub fn new(objects: usize, payload_bytes: u64) -> Result<Self> {
        WriteLimits::new(objects, payload_bytes).map_err(|_| Error::Quota)?;
        Ok(Self {
            objects,
            payload_bytes,
        })
    }
    pub const fn objects(self) -> usize {
        self.objects
    }
    pub const fn payload_bytes(self) -> u64 {
        self.payload_bytes
    }
    pub(crate) fn limits(self) -> Result<WriteLimits> {
        Ok(WriteLimits::new(self.objects, self.payload_bytes)?)
    }
}

/// Stored metadata only. Owner bytes are supplied by a trusted native caller;
/// they do not prove an account exists or grant a session/file permission.
#[derive(Clone, PartialEq, Eq)]
pub struct FileInfo {
    pub(crate) id: FileId,
    pub(crate) object: ObjectId,
    pub(crate) owner: [u8; 16],
    pub(crate) name: String,
    pub(crate) report: FileReport,
    pub(crate) revision: u64,
}
impl FileInfo {
    pub const fn id(&self) -> FileId {
        self.id
    }
    pub const fn object(&self) -> ObjectId {
        self.object
    }
    pub const fn owner(&self) -> &[u8; 16] {
        &self.owner
    }
    /// Bounded display text; never used as a path or HTTP header.
    pub fn name(&self) -> &str {
        &self.name
    }
    pub const fn report(&self) -> &FileReport {
        &self.report
    }
    pub const fn revision(&self) -> u64 {
        self.revision
    }
}
impl std::fmt::Debug for FileInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileInfo")
            .field("bytes", &self.report.payload_bytes)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

#[derive(thiserror::Error)]
pub enum Error {
    #[error("invalid canonical logical file identity")]
    Identity,
    #[error("invalid bounded file display name")]
    Name,
    #[error("invalid persisted file quota")]
    Quota,
    #[error("bounded native file allocation failed")]
    Allocation,
    #[error("invalid native file archive")]
    Archive,
    #[error("unsupported native file archive version: {0}")]
    ArchiveVersion(u16),
    #[error("native file archive checksum mismatch")]
    ArchiveChecksum,
    #[error("invalid native file archive destination or private file")]
    Destination,
    #[error("native file archive filesystem operation failed")]
    Io(#[from] std::io::Error),
    #[error("native file archive publication failed")]
    Publication(#[from] emilybase_storage::Error),
    #[error("invalid native file catalog or object graph")]
    Corrupt,
    #[error("native file project or metadata database identity differs")]
    Scope,
    #[error("file catalog initialization needs pristine metadata and empty objects")]
    NotEmpty,
    #[error("logical file identity already exists")]
    Exists,
    #[error("logical file revision does not match")]
    Conflict,
    #[error("logical file identity does not exist")]
    Missing,
    #[error("native file revision exhausted")]
    Revision,
    #[error("native file catalog requires reopen after an uncertain write")]
    Poisoned,
    #[error("native file operation may have published data; inspect before retrying")]
    OutcomeUnknown(#[source] Box<Error>),
    #[error("file metadata engine operation failed")]
    Engine(#[from] emilybase_transactions::Error),
    #[error("native file object operation failed")]
    Objects(#[from] emilybase_object_storage::Error),
    #[error("native file metadata backup failed")]
    Backup(#[from] emilybase_backup::Error),
}
impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

/// Pure bounded inspection of a private catalog row, not a native descriptor,
/// current catalog membership, account/session proof or file authorization.
pub fn inspect_file_record(
    row: &[emilybase_catalog::Value],
    last_transaction: u64,
) -> Result<FileInfo> {
    records::decode(row, last_transaction)
}

#[cfg(test)]
static TEST_IO: std::sync::Mutex<()> = std::sync::Mutex::new(());
