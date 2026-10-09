//! Experimental original object envelope, not an HTTP upload or access authority.
mod archive;
mod directory;
mod format;
mod inspect;
pub use archive::{
    ARCHIVE_HEADER_BYTES, ArchiveReport, ArchivedObject, MAX_ARCHIVE_BYTES, VerifiedArchive,
    encode_archive, encode_verified_archive, inspect_archive_file, verify_archive,
};
pub use directory::{
    Inventory, InventoryEntry, MAX_INVENTORY_BYTES, MAX_INVENTORY_OBJECTS, ObjectSnapshot,
    ProjectDirectory, StoredObject, object_id_from_name,
};
pub use format::{
    HEADER_BYTES, MAX_PAYLOAD_BYTES, ObjectId, ProjectId, VerifiedObject, encode, verify,
};
pub use inspect::{FileReport, inspect_file, publish_file};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid canonical object identity")]
    Identity,
    #[error("invalid object envelope")]
    Format,
    #[error("unsupported object format version {0}")]
    Version(u16),
    #[error("object header checksum mismatch")]
    HeaderChecksum,
    #[error("object payload checksum mismatch")]
    PayloadChecksum,
    #[error("object project or identity mismatch")]
    Scope,
    #[error("object size limit exceeded")]
    Limit,
    #[error("object allocation failed")]
    Allocation,
    #[error("object file must be private, regular, singly linked and unchanged")]
    File,
    #[error("object directory must be private, owned and correctly initialized")]
    Directory,
    #[error("object directory already has a cooperating owner")]
    Busy,
    #[error("object inventory contains an unknown or invalid entry")]
    Inventory,
    #[error("object inventory changed during verification")]
    InventoryChanged,
    #[error("invalid object archive")]
    Archive,
    #[error("unsupported object archive version {0}")]
    ArchiveVersion(u16),
    #[error("object archive checksum mismatch")]
    ArchiveChecksum,
    #[error("object was published but its final durability or contents require inspection")]
    PublicationUnknown,
    #[error("owned object publication failed")]
    Publication(#[source] emilybase_storage::Error),
    #[error("object file I/O failed")]
    Io(#[source] std::io::Error),
}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod crash_tests;
#[cfg(test)]
mod directory_crash_tests;
#[cfg(test)]
mod directory_tests;
#[cfg(test)]
mod tests;
