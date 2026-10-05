//! Relational file records built on the original EmilyBase page engine.
mod engine;
mod event;
mod location;
mod primary;
mod primary_rows;
#[cfg(test)]
mod primary_rows_tests;
mod projection;
mod range;
#[cfg(test)]
mod sharing_tests;
mod snapshot;
mod state;
mod text_range;

pub use engine::Database;
pub use event::{DATABASE_MARKER, Event, EventKind};
pub use location::RowLocation;
pub use primary::PrimaryIndexInfo;
pub use primary_rows::PrimaryRows;
pub use snapshot::Snapshot;

pub const MAX_TABLES: usize = 128;
pub const MAX_ROWS: usize = 10000;
pub const MAX_EVENTS: usize = 100000;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("integer primary-key range requires an integer primary key")]
    IntegerRangeType,
    #[error("text range requires a text primary key")]
    TextRangeType,
    #[error("invalid derived primary index: {0}")]
    PrimaryIndex(&'static str),
    #[error(transparent)]
    Storage(#[from] emilybase_storage::Error),
    #[error(transparent)]
    Catalog(#[from] emilybase_catalog::Error),
    #[error("malformed relational event: {0}")]
    Event(&'static str),
    #[error("unsupported relational event version: {0}")]
    EventVersion(u16),
    #[error("file is not an initialized table database")]
    NotTableFile,
    #[error("table already exists")]
    TableExists,
    #[error("table does not exist")]
    NoTable,
    #[error("primary key already exists")]
    DuplicateKey,
    #[error("row does not exist")]
    NoRow,
    #[error("row location is stale or does not identify the current row image")]
    StaleLocation,
    #[error("updates cannot change a primary key")]
    PrimaryKeyChange,
    #[error("database limit reached: {0}")]
    Limit(&'static str),
    #[error("database encountered a write failure; close and inspect the file")]
    Poisoned,
}

pub type Result<T> = std::result::Result<T, Error>;
