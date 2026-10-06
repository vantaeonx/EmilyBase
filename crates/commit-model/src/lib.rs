//! Experimental synchronous in-memory table/index transaction model.
//! This module writes no files or WAL and provides no durable commit acknowledgment.
mod admission;
mod components;
mod images;
mod selection;
mod staging;
mod state;

pub use admission::{
    AdmissionLimit, AdmissionLimits, AdmissionUsage, AdmittedPrepared, AdmittedReplay,
    AdmittedStage, MAX_LIFETIME_SLOTS, MAX_MODEL_PROJECTS, MAX_MODEL_WRITERS, ModelPool,
    ModelProject, ModelReader,
};
pub use components::EncodedComponents;
pub use images::{
    AdmittedEnvelope, AdmittedPlan, DecodedPlanLimit, DecodedPlanLimits, DecodedPlanPool,
    DecodedPlanUsage, EnvelopeLimit, EnvelopeLimits, EnvelopePool, EnvelopeUsage,
    IMAGE_PLAN_HEADER_BYTES, IMAGE_PLAN_MAX_BYTES, IMAGE_PLAN_VERSION, ImagePlan,
    MAX_DECODED_PLAN_BYTES, MAX_DECODED_PLAN_VECTOR_BYTES, MAX_DECODED_PLANS, MAX_ENVELOPE_BUFFERS,
    MAX_ENVELOPE_BYTES, PageWrite, PlanCounts, RetiredRoot, RootChange,
};
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
    #[error("invalid experimental image plan: {0}")]
    Plan(&'static str),
    #[error("invalid experimental image envelope length")]
    PlanLength,
    #[error("unsupported experimental image envelope version {0}")]
    PlanVersion(u16),
    #[error("experimental image envelope checksum mismatch")]
    PlanChecksum,
    #[error("experimental image envelope allocation refused")]
    PlanAllocation,
    #[error("invalid experimental envelope admission configuration")]
    EnvelopeConfiguration,
    #[error("experimental envelope admission refused: {0:?}")]
    EnvelopeAdmission(EnvelopeLimit),
    #[error("experimental envelope admission lock is poisoned")]
    EnvelopePoisoned,
    #[error("invalid decoded plan admission configuration")]
    DecodedConfiguration,
    #[error("decoded plan admission refused: {0:?}")]
    DecodedAdmission(DecodedPlanLimit),
    #[error("decoded plan admission lock is poisoned")]
    DecodedPoisoned,
    #[error("decoded vector capacity disagrees with its reservation")]
    DecodedAllocationShape,
}

pub type Result<T> = std::result::Result<T, Error>;
