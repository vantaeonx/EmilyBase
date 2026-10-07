//! Private local file boundaries for the independently validated bundle envelope.
use super::{AccountBundleReport, MAX_ACCOUNT_BUNDLE_BYTES, inspect_account_bundle_bytes};
use crate::{
    Result,
    registry_files::{self, FileArchive},
};
use std::path::Path;

/// Reads one private regular single-link file without following its final symlink.
/// This returns metadata, not credential authority or proof of capture provenance.
pub fn inspect_account_bundle(path: impl AsRef<Path>) -> Result<AccountBundleReport> {
    let bytes = registry_files::read_bounded(path.as_ref(), MAX_ACCOUNT_BUNDLE_BYTES)?;
    inspect_account_bundle_bytes(&bytes)
}
pub(crate) fn publish(bytes: &[u8], target: &Path) -> Result<AccountBundleReport> {
    registry_files::publish_checked(
        bytes,
        target,
        inspect_account_bundle_bytes,
        FileArchive::AccountBundle,
    )
}
