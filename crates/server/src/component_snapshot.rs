//! Explicit native component capture; no filesystem roster adoption or user API.
use crate::{
    AccountFileRootManifest, AccountRoot, AccountRootSnapshot, Error, FileRootBinding, Result,
};
use emilybase_files::{FileSnapshot, NativeFileRoot, NativeFileRootInfo};
use std::collections::BTreeSet;

pub const MAX_COMPONENT_FILE_STORES: usize = 4;
/// Actual physical objects across all components, including every orphan.
pub const MAX_COMPONENT_OBJECTS: usize = 128;
/// Retained encoded image bytes, excluding models, descriptors and copy scratch.
pub const MAX_COMPONENT_IMAGE_BYTES: usize = 128 * 1024 * 1024;

/// Sensitive immutable component copies. Not a serialized common archive, a
/// current permission, filesystem authority, publication or restored destination.
pub struct AccountFileSnapshot {
    accounts: AccountRootSnapshot,
    files: Vec<FileSnapshot>,
    manifest: AccountFileRootManifest,
    manifest_bytes: Vec<u8>,
    copied_bytes: usize,
    physical_objects: usize,
}
impl AccountFileSnapshot {
    pub fn accounts(&self) -> &AccountRootSnapshot {
        &self.accounts
    }
    /// Same strict project order as the manifest and caller's original owners.
    pub fn files(&self) -> &[FileSnapshot] {
        &self.files
    }
    pub fn manifest(&self) -> &AccountFileRootManifest {
        &self.manifest
    }
    pub fn manifest_bytes(&self) -> &[u8] {
        &self.manifest_bytes
    }
    pub const fn copied_bytes(&self) -> usize {
        self.copied_bytes
    }
    pub const fn physical_objects(&self) -> usize {
        self.physical_objects
    }
}
impl std::fmt::Debug for AccountFileSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountFileSnapshot")
            .field("file_stores", &self.files.len())
            .field("physical_objects", &self.physical_objects)
            .field("copied_bytes", &self.copied_bytes)
            .finish_non_exhaustive()
    }
}

/// Trusted native readonly operation over already selected original owners.
/// Files must be strictly sorted by project and belong to the account root's
/// declared private roster. All original account/public owners and every actual
/// payload descriptor span the innermost capture. Nothing is returned unless
/// every source passes final checks. No clocks, sessions or sources are changed.
///
/// The physical object cap is admitted before any capture readers are retained.
/// The copied-image cap is checked after each bounded native copy: temporary
/// component buffers and decoded models are additional memory, not a reservation.
/// A single common filesystem root, archive encoder and restore are separate work.
pub fn capture_account_file_components(
    accounts: &mut AccountRoot,
    files: &mut [NativeFileRoot],
) -> Result<AccountFileSnapshot> {
    capture_with(accounts, files, &mut |_| Ok(()))
}

pub(crate) fn capture_with(
    accounts: &mut AccountRoot,
    files: &mut [NativeFileRoot],
    work: &mut dyn FnMut(&AccountFileSnapshot) -> Result<()>,
) -> Result<AccountFileSnapshot> {
    capture_with_limit(accounts, files, work, MAX_COMPONENT_IMAGE_BYTES)
}

pub(crate) fn capture_with_limit(
    accounts: &mut AccountRoot,
    files: &mut [NativeFileRoot],
    work: &mut dyn FnMut(&AccountFileSnapshot) -> Result<()>,
    image_limit: usize,
) -> Result<AccountFileSnapshot> {
    if image_limit > MAX_COMPONENT_IMAGE_BYTES {
        return Err(Error::Limit);
    }
    if files.len() > MAX_COMPONENT_FILE_STORES {
        return Err(Error::Limit);
    }
    let original_manifest = accounts.snapshot_manifest()?;
    let mut admitted = Vec::with_capacity(files.len());
    let mut bindings = Vec::with_capacity(files.len());
    let mut object_count = 0;
    for file in files.iter_mut() {
        let info = file.inspect()?;
        let project = info.project().to_string();
        if original_manifest
            .private_projects
            .binary_search(&project)
            .is_err()
            || bindings
                .last()
                .is_some_and(|previous: &FileRootBinding| previous.project() >= project.as_str())
        {
            return Err(Error::ComponentSnapshot("private membership or file order"));
        }
        object_count = charge(
            object_count,
            info.usage().physical_objects,
            MAX_COMPONENT_OBJECTS,
        )?;
        bindings.push(FileRootBinding::new(&project, &hex(info.database_id()))?);
        admitted.push(info);
    }
    let manifest = AccountFileRootManifest::new(
        original_manifest.private_projects.clone(),
        bindings,
        original_manifest.reset_at,
    )?;
    let manifest_bytes = manifest.to_bytes()?;
    checkpoint("components_admitted");
    accounts.capture_with(|snapshot| {
        if snapshot.manifest() != &original_manifest {
            return Err(Error::ComponentSnapshot("account roster changed"));
        }
        check_database_ids(&snapshot, &admitted)?;
        let copied_bytes = charge(
            snapshot.bundle_bytes().len(),
            manifest_bytes.len(),
            image_limit,
        )?;
        let mut result = AccountFileSnapshot {
            accounts: snapshot,
            files: Vec::with_capacity(files.len()),
            manifest,
            manifest_bytes,
            copied_bytes,
            physical_objects: object_count,
        };
        checkpoint("components_account_copied");
        capture_files(files, &admitted, &mut result, work, image_limit)?;
        Ok(result)
    })
}

// Fixed signature keeps recursion bounded by the pre-admitted four components;
// every native callback's outer result is flattened before returning any copies.
fn capture_files(
    roots: &mut [NativeFileRoot],
    admitted: &[NativeFileRootInfo],
    result: &mut AccountFileSnapshot,
    work: &mut dyn FnMut(&AccountFileSnapshot) -> Result<()>,
    image_limit: usize,
) -> Result<()> {
    let Some((root, remaining)) = roots.split_first_mut() else {
        checkpoint("components_retained");
        return work(result);
    };
    let expected = &admitted[0];
    root.with_snapshot(|snapshot| -> Result<()> {
        if snapshot.project() != expected.project()
            || &snapshot.metadata_report().database_id != expected.database_id()
            || snapshot.metadata_report().last_transaction != expected.revision()
            || snapshot.quota() != expected.quota()
            || snapshot.files().len() != expected.usage().references
            || snapshot.objects().objects().len() != expected.usage().physical_objects
            || snapshot.objects().inventory().payload_bytes() != expected.usage().payload_bytes
        {
            return Err(Error::ComponentSnapshot(
                "file component changed since admission",
            ));
        }
        let mut copied_bytes = charge(
            result.copied_bytes,
            snapshot.metadata_bytes().len(),
            image_limit,
        )?;
        for object in snapshot.objects().objects() {
            copied_bytes = charge(
                copied_bytes,
                emilybase_object_storage::HEADER_BYTES,
                image_limit,
            )?;
            copied_bytes = charge(copied_bytes, object.payload().len(), image_limit)?;
        }
        result.copied_bytes = copied_bytes;
        result.files.push(snapshot);
        capture_files(remaining, &admitted[1..], result, work, image_limit)
    })??;
    Ok(())
}

fn check_database_ids(accounts: &AccountRootSnapshot, files: &[NativeFileRootInfo]) -> Result<()> {
    let ids = accounts
        .report()
        .registry
        .projects
        .iter()
        .map(|p| p.database_id)
        .chain(
            accounts
                .report()
                .private_accounts
                .iter()
                .map(|p| p.inventory.database.database_id),
        )
        .chain(files.iter().map(|p| *p.database_id()));
    unique_ids(ids)
}
fn unique_ids(ids: impl Iterator<Item = [u8; 16]>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(Error::ComponentSnapshot(
                "duplicate component database identity",
            ));
        }
    }
    Ok(())
}
fn charge(current: usize, additional: usize, limit: usize) -> Result<usize> {
    current
        .checked_add(additional)
        .filter(|&total| total <= limit)
        .ok_or(Error::Limit)
}
fn hex(id: &[u8; 16]) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn checkpoint(_point: &str) {
    #[cfg(test)]
    crate::durability::checkpoint(_point);
}

#[cfg(test)]
mod tests;
