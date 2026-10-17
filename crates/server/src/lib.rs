//! Bounded HTTP transport over a synchronous isolated-project/key registry.
mod account_bundle;
mod component_archive;
mod component_snapshot;
pub use component_archive::publication::{
    AccountFileArchiveReport, inspect_account_file_archive, publish_account_file_archive,
};
pub use component_archive::{
    ACCOUNT_FILE_ARCHIVE_VERSION, AccountFileArchiveReader, MAX_ACCOUNT_FILE_ARCHIVE_BYTES,
    VerifiedAccountFileArchive, verify_account_file_archive,
};
pub use component_snapshot::{
    AccountFileSnapshot, MAX_COMPONENT_FILE_STORES, MAX_COMPONENT_IMAGE_BYTES,
    MAX_COMPONENT_OBJECTS, capture_account_file_components,
};
mod file_root_manifest;
pub use file_root_manifest::{
    ACCOUNT_FILE_ROOT_MANIFEST_VERSION, AccountFileRootManifest, FileRootBinding,
    MAX_ACCOUNT_FILE_ROOT_MANIFEST_BYTES, inspect_account_file_root_manifest_bytes,
};
mod account_http;
pub use account_http::{
    PolicyTransportError, SessionRequest, SessionRequestError, USER_ACCESS_HEADER, account_router,
    validate_policy_install_request, validate_session_request,
};
#[cfg(test)]
mod durability;
mod http;
mod metadata;
mod migration_api;
pub use migration_api::{MigrationError, validate_migration_request};
mod projects;
mod rate;
mod row_api;
mod table_api;
mod user_rows;
pub use table_api::TableError;
pub use user_rows::{
    MAX_USER_PAGE_ROWS, UserRowsError, UserTableOperation, UserTableResult, UserWrite,
};
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
    AccountBundleRootManifest, AccountBundleRootReport, AccountRoot, AccountRootSnapshot,
    MAX_ACTIVE_PRIVATE_STORES, backup_account_bundle_root, capture_account_bundle_root,
    initialize_account_root, inspect_account_bundle_root,
    inspect_account_bundle_root_manifest_bytes, restore_account_bundle,
    restore_account_bundle_bytes,
};
pub use registry_files::{
    AccountFileRootReport, inspect_registry_backup, restore_account_file_archive_bytes_at,
    restore_registry_backup, with_restored_registry_backup_bytes_at,
};

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
    #[error("invalid file-root manifest: {0}")]
    FileRootManifest(&'static str),
    #[error("unsupported file-root manifest version: {0}")]
    FileRootManifestVersion(u16),
    #[error("invalid account/file component snapshot: {0}")]
    ComponentSnapshot(&'static str),
    #[error("invalid account/file archive: {0}")]
    ComponentArchive(&'static str),
    #[error("unsupported account/file archive version: {0}")]
    ComponentArchiveVersion(u16),
    #[error("account/file archive checksum mismatch")]
    ComponentArchiveChecksum,
    #[error("joined archive publication outcome is unknown; inspect before retrying")]
    ComponentPublicationUnknown(#[source] Box<Error>),
    #[error("joined root restore outcome is unknown; inspect before retrying")]
    ComponentRestoreUnknown(#[source] Box<Error>),
    #[error("prepared registry restore outcome is unknown; inspect before retrying")]
    RegistryRestoreUnknown(#[source] Box<Error>),
    #[error("native storage publication failed")]
    Storage(#[from] emilybase_storage::Error),
    #[error("native file storage failed")]
    Files(#[from] emilybase_files::Error),
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
    #[error(transparent)]
    Transfer(#[from] emilybase_transfer::Error),
    #[error(transparent)]
    Tables(#[from] TableError),
    #[error(transparent)]
    Migrations(#[from] MigrationError),
    #[error(transparent)]
    Policies(#[from] PolicyTransportError),
    #[error(transparent)]
    UserRows(#[from] UserRowsError),
    #[error(transparent)]
    UserTransport(#[from] UserRowTransportError),
    #[error("project filesystem error")]
    Io(#[from] std::io::Error),
    #[error("HTTP transport failed")]
    Transport(#[source] std::io::Error),
    #[error("invalid server configuration: {0}")]
    Config(&'static str),
}

pub use row_api::{
    Operation as RowOperation, UserRowTransportError, validate_row_request,
    validate_user_row_request,
};
