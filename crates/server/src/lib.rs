//! Bounded HTTP transport over a synchronous isolated-project/key registry.
mod account_bundle;
#[cfg(test)]
mod durability;
mod http;
mod metadata;
mod projects;
mod rate;
mod registry_archive;
mod registry_files;
pub use account_bundle::{
    ACCOUNT_BUNDLE_VERSION, AccountBundleReport, BundledAccountReport, MAX_ACCOUNT_BUNDLE_BYTES,
    inspect_account_bundle, inspect_account_bundle_bytes,
};
pub use http::{router, serve};
pub use projects::{AuthorizedProject, CreatedProject, ProjectInfo, ProjectStatus, ProjectStore};
pub use registry_archive::{
    MAX_REGISTRY_BACKUP_BYTES, REGISTRY_BACKUP_VERSION, RegistryBackupReport,
    RegistryProjectReport, inspect_registry_backup_bytes,
};
pub use registry_files::account_root::{
    AccountBundleRootManifest, AccountBundleRootReport, inspect_account_bundle_root,
    inspect_account_bundle_root_manifest_bytes, restore_account_bundle,
    restore_account_bundle_bytes,
};
pub use registry_files::{inspect_registry_backup, restore_registry_backup};

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
    #[error("invalid account bundle: {0}")]
    BundleFormat(&'static str),
    #[error("unsupported account bundle version: {0}")]
    BundleVersion(u16),
    #[error("account bundle checksum mismatch")]
    BundleChecksum,
    #[error("invalid account bundle root: {0}")]
    BundleRoot(&'static str),
    #[error("private account storage failed")]
    Accounts(#[from] emilybase_auth::accounts::Error),
    #[error("invalid project metadata")]
    Metadata,
    #[error("invalid registry backup: {0}")]
    RegistryFormat(&'static str),
    #[error("unsupported registry backup version: {0}")]
    RegistryVersion(u16),
    #[error("registry backup checksum mismatch")]
    RegistryChecksum,
    #[error("project limit or key epoch limit exceeded")]
    Limit,
    #[error("invalid project display name")]
    Name,
    #[error("publication outcome is unknown; verify destination before retrying")]
    PublicationUnknown(#[source] std::io::Error),
    #[error(transparent)]
    Auth(#[from] emilybase_auth::Error),
    #[error(transparent)]
    Transaction(#[from] emilybase_transactions::Error),
    #[error(transparent)]
    Backup(#[from] emilybase_backup::Error),
    #[error(transparent)]
    Query(#[from] emilybase_query::ExecutionError),
    #[error("project filesystem error")]
    Io(#[from] std::io::Error),
    #[error("HTTP transport failed")]
    Transport(#[source] std::io::Error),
    #[error("invalid server configuration: {0}")]
    Config(&'static str),
}
