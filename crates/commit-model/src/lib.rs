//! Experimental synchronous in-memory table/index transaction model.
//! This module writes no files or WAL and provides no durable commit acknowledgment.
mod admission;
mod components;
mod selection;
mod staging;
mod state;

pub use admission::{
    AdmissionLimit, AdmissionLimits, AdmissionUsage, AdmittedPrepared, AdmittedStage,
    MAX_LIFETIME_SLOTS, MAX_MODEL_PROJECTS, MAX_MODEL_WRITERS, ModelPool, ModelProject,
    ModelReader,
};
pub use components::EncodedComponents;
pub use selection::Selection;
pub use staging::{Prepared, Staged};
pub use state::Model;
pub const MAX_EVENTS: usize = 256;
/// Combined live image bound, distinct from the per-table 1024-page arena bound.
/// This is an encoded-index limit, not a Rust heap or server memory reservation.
pub const MAX_SELECTED_INDEX_PAGES: usize = 2048;

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
    #[error("invalid model admission configuration")]
    AdmissionConfiguration,
    #[error("model admission refused: {0:?}")]
    Admission(AdmissionLimit),
    #[error("database identity is already registered in this model pool")]
    DuplicateDatabase,
    #[error("model admission lock is poisoned")]
    AdmissionPoisoned,
}

pub type Result<T> = std::result::Result<T, Error>;
