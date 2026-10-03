//! Serialized table transactions backed by the mandatory authoritative redo log.
mod database;
mod replay;
mod transaction;

pub use database::Database;
pub use transaction::Transaction;
pub const MAX_TRANSACTION_EVENTS: usize = 256;

/// Pure, bounded recovery for verification, offline inspection and fuzzing.
pub fn recover_snapshot(
    bytes: &[u8],
    expected_id: Option<emilybase_wal::DatabaseId>,
) -> Result<emilybase_database::Snapshot> {
    replay::replay(emilybase_wal::recover(bytes, expected_id)?)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Database(#[from] emilybase_database::Error),
    #[error(transparent)]
    Wal(#[from] emilybase_wal::Error),
    #[error(transparent)]
    Storage(#[from] emilybase_storage::Error),
    #[error(transparent)]
    Catalog(#[from] emilybase_catalog::Error),
    #[error("transaction filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("transaction limit reached")]
    Limit,
    #[error("transaction is aborted; rollback or drop it")]
    Aborted,
    #[error("malformed committed history: {0}")]
    History(&'static str),
    #[error("database encountered an ambiguous write failure; reopen it")]
    Poisoned,
    #[error("operating-system randomness is unavailable")]
    Randomness,
}

pub type Result<T> = std::result::Result<T, Error>;
