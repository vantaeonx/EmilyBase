//! Bounded HTTP transport over a synchronous isolated-project/key registry.
#[cfg(test)]
mod durability;
mod http;
mod metadata;
mod projects;
mod rate;
pub use http::{router, serve};
pub use projects::{AuthorizedProject, CreatedProject, ProjectInfo, ProjectStatus, ProjectStore};

/// Pure, bounded inspection without credentials, filesystem operations or data access.
pub fn inspect_project_metadata(id: &str, bytes: &[u8]) -> Result<ProjectInfo> {
    let metadata = metadata::decode(bytes, id)?;
    Ok(ProjectInfo {
        id: metadata.id,
        name: metadata.name,
        key_epoch: metadata.epoch,
    })
}

pub const MAX_PROJECTS: usize = 128;
pub const MAX_PROJECT_NAME_BYTES: usize = 128;
pub const MAX_METADATA_BYTES: u64 = 4096;
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("project access denied")]
    Denied,
    #[error("project registry ownership is busy")]
    Busy,
    #[error("project registry is poisoned; reopen before use")]
    Poisoned,
    #[error("unsafe project filesystem path or permissions")]
    Path,
    #[error("invalid project metadata")]
    Metadata,
    #[error("project limit or key epoch limit exceeded")]
    Limit,
    #[error("invalid project display name")]
    Name,
    #[error("project publication outcome is unknown; reopen before use")]
    PublicationUnknown(#[source] std::io::Error),
    #[error(transparent)]
    Auth(#[from] emilybase_auth::Error),
    #[error(transparent)]
    Transaction(#[from] emilybase_transactions::Error),
    #[error(transparent)]
    Query(#[from] emilybase_query::ExecutionError),
    #[error("project filesystem error")]
    Io(#[from] std::io::Error),
    #[error("HTTP transport failed")]
    Transport(#[source] std::io::Error),
    #[error("invalid server configuration: {0}")]
    Config(&'static str),
}
