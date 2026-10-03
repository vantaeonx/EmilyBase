//! Serialized table transactions backed by the mandatory authoritative redo log.
mod compaction;
mod database;
mod location;
mod ownership;
mod replay;
mod transaction;

pub use compaction::Compaction;
pub use database::Database;
pub use location::BoundRowLocation;
pub use transaction::Transaction;
pub const MAX_TRANSACTION_EVENTS: usize = 256;

#[cfg(test)]
pub(crate) static PROCESS_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Pure, bounded recovery for verification, offline inspection and fuzzing.
pub fn recover_snapshot(
    bytes: &[u8],
    expected_id: Option<emilybase_wal::DatabaseId>,
) -> Result<emilybase_database::Snapshot> {
    Ok(recover_image(bytes, expected_id)?.snapshot)
}

pub struct RecoveredImage {
    pub snapshot: emilybase_database::Snapshot,
    pub database_id: emilybase_wal::DatabaseId,
    pub last_transaction: u64,
    pub wal_version: u16,
    pub committed_bytes: usize,
    pub discarded_bytes: usize,
}

/// Recover state and its durable boundary using the same strict replay protocol.
pub fn recover_image(
    bytes: &[u8],
    expected_id: Option<emilybase_wal::DatabaseId>,
) -> Result<RecoveredImage> {
    let recovery = emilybase_wal::recover(bytes, expected_id)?;
    let database_id = recovery.database_id;
    let last_transaction = recovery.last_transaction();
    let wal_version = recovery.format_version;
    let committed_bytes = recovery.valid_bytes;
    let discarded_bytes = recovery.discarded_bytes;
    Ok(RecoveredImage {
        snapshot: replay::replay(recovery)?,
        database_id,
        last_transaction,
        wal_version,
        committed_bytes,
        discarded_bytes,
    })
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
    #[error("row location belongs to a different database identity")]
    LocationDatabase,
    #[error("transaction is aborted; rollback or drop it")]
    Aborted,
    #[error("malformed committed history: {0}")]
    History(&'static str),
    #[error("database encountered an ambiguous write failure; reopen it")]
    Poisoned,
    #[error("operating-system randomness is unavailable")]
    Randomness,
    #[error("journal replacement was published but durability is uncertain; reopen before writing")]
    MaintenanceUnknown(#[source] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
