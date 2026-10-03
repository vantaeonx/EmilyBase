//! Immutable bounded backups of validated, committed original-engine history.
mod archive;
mod files;
mod header;
mod publish;
mod restore;

pub use archive::{encode, inspect_bytes};
pub use files::{create, inspect};
pub use restore::restore;

pub const BACKUP_VERSION: u16 = 1;
pub const HEADER_SIZE: usize = 128;
pub const MAX_BACKUP_BYTES: usize = HEADER_SIZE + emilybase_wal::MAX_WAL_BYTES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub database_id: emilybase_wal::DatabaseId,
    pub last_transaction: u64,
    pub wal_bytes: usize,
    pub tables: usize,
    pub rows: usize,
    pub pages: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("backup filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Transactions(#[from] emilybase_transactions::Error),
    #[error("malformed backup: {0}")]
    Format(&'static str),
    #[error("unsupported backup version: {0}")]
    Version(u16),
    #[error("backup checksum mismatch")]
    Checksum,
    #[error("backup size limit exceeded")]
    Limit,
    #[error("operating-system randomness is unavailable")]
    Randomness,
    #[error(
        "backup was published but durability is uncertain; verify its destination before retrying"
    )]
    PublicationUnknown(#[source] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
