//! Synchronous bounded redo log. Only synced commit records acknowledge a batch.
mod codec;
mod io;
mod journal;
mod recovery;

pub use codec::{FRAME_SIZE, HEADER_SIZE, encode_header};
pub use journal::{Pending, Wal};
pub use recovery::{Committed, Recovery, recover};

pub type DatabaseId = [u8; 16];
pub const MAX_TRANSACTION_PAGES: usize = 256;
pub const MAX_WAL_BYTES: usize = 64 * 1024 * 1024;
pub const WAL_VERSION: u16 = 1;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("journal filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Storage(#[from] emilybase_storage::Error),
    #[error("malformed journal: {0}")]
    Format(&'static str),
    #[error("unsupported journal version: {0}")]
    Version(u16),
    #[error("journal checksum mismatch")]
    Checksum,
    #[error("journal belongs to a different database")]
    Identity,
    #[error("journal is already locked")]
    Busy,
    #[error("journal limit reached: {0}")]
    Limit(&'static str),
    #[error("journal encountered an I/O failure; reopen before further writes")]
    Poisoned,
    #[error("commit outcome is unknown; reopen and inspect transaction {transaction}")]
    OutcomeUnknown {
        transaction: u64,
        #[source]
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
