use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("filesystem error: {0}")]
    Io(#[from] io::Error),
    #[error("invalid file length: {0}")]
    FileLength(u64),
    #[error("invalid magic bytes")]
    Magic,
    #[error("unsupported format version: {0}")]
    Version(u16),
    #[error("unsupported page size: {0}")]
    PageSize(u16),
    #[error("checksum mismatch")]
    Checksum,
    #[error("malformed page: {0}")]
    Layout(&'static str),
    #[error("invalid page ID: {0}")]
    PageId(u64),
    #[error("record exceeds the maximum page record size")]
    RecordTooLarge,
    #[error("page has insufficient free space")]
    PageFull,
    #[error("slot {0} is missing or deleted")]
    Slot(u16),
    #[error("database is already locked")]
    Busy,
    #[error("database page limit reached")]
    PageLimit,
    #[error("pager has encountered an I/O failure; close and inspect the file")]
    Poisoned,
    #[error("page-file path must be regular, single-link and not a symlink")]
    Path,
    #[error("page-file destination or staging identity changed")]
    PathChanged,
    #[error("operating-system randomness is unavailable")]
    Randomness,
    #[error("page file was published but its durability or destination requires inspection")]
    PublicationUnknown(#[source] io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
