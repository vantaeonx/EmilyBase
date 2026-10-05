//! Experimental synchronous in-memory table/index transaction model.
//! This module writes no files or WAL and provides no durable commit acknowledgment.
mod selection;
mod staging;
mod state;

pub use selection::Selection;
pub use staging::{Prepared, Staged};
pub use state::Model;
pub const MAX_EVENTS: usize = 256;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Metadata(#[from] emilybase_commit_format::Error),
    #[error(transparent)]
    Database(#[from] emilybase_database::Error),
    #[error(transparent)]
    Index(#[from] emilybase_index::Error),
    #[error("experimental model transaction is aborted")]
    Aborted,
    #[error("experimental model transaction has no changes")]
    Empty,
    #[error("experimental model bound exceeded")]
    Limit,
    #[error("invalid complete table/index selection: {0}")]
    Selection(&'static str),
    #[error("prepared model does not match the exact current state")]
    Conflict,
}

pub type Result<T> = std::result::Result<T, Error>;
