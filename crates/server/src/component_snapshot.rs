//! Explicit native component capture; no filesystem roster adoption or user API.
use crate::{AccountFileRootManifest, AccountRoot, Error, FileRootBinding, Result};
use emilybase_files::{FileSnapshot, NativeFileRoot, NativeFileRootInfo};
use std::collections::BTreeSet;

/// Sensitive account-bundle subcomponent. Its roster/reset report does not
/// describe a whole root: only the enclosing v2 manifest carries file bindings.
pub struct AccountComponentSnapshot {
    image: Vec<u8>,
    report: crate::AccountBundleRootReport,
}
impl AccountComponentSnapshot {
    pub(crate) fn new(image: Vec<u8>, report: crate::AccountBundleRootReport) -> Self {
        Self { image, report }
    }
    pub fn bundle_bytes(&self) -> &[u8] {
        &self.image
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.image
    }
    pub fn report(&self) -> &crate::AccountBundleRootReport {
        &self.report
    }
}

pub const MAX_COMPONENT_FILE_STORES: usize = 4;
/// Actual physical objects across all components, including every orphan.
pub const MAX_COMPONENT_OBJECTS: usize = 128;
/// Retained encoded image bytes, excluding models, descriptors and copy scratch.
pub const MAX_COMPONENT_IMAGE_BYTES: usize = 128 * 1024 * 1024;

/// Sensitive immutable component copies. Not a serialized common archive, a
/// current permission, filesystem authority, publication or restored destination.
pub struct AccountFileSnapshot {
    accounts: AccountComponentSnapshot,
    files: Vec<FileSnapshot>,
    manifest: AccountFileRootManifest,
    manifest_bytes: Vec<u8>,
    copied_bytes: usize,
    physical_objects: usize,
}
impl AccountFileSnapshot {
    pub fn accounts(&self) -> &AccountComponentSnapshot {
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
    pub(crate) fn from_owned(
        accounts: AccountComponentSnapshot,
        files: Vec<FileSnapshot>,
        admitted: &[NativeFileRootInfo],
        manifest: AccountFileRootManifest,
    ) -> Result<Self> {
        if accounts.report().reset_at != manifest.reset_at()
            || accounts
                .report()
                .private_accounts
                .iter()
                .map(|p| p.project.as_str())
                .ne(manifest.private_projects().iter().map(String::as_str))
            || files.len() != manifest.file_stores().len()
        {
            return Err(Error::ComponentSnapshot("account roster changed"));
        }
        check_database_ids(&accounts, admitted)?;
        let manifest_bytes = manifest.to_bytes()?;
        let mut copied_bytes = charge(
            accounts.bundle_bytes().len(),
            manifest_bytes.len(),
            MAX_COMPONENT_IMAGE_BYTES,
        )?;
        let mut physical_objects = 0;
        for file in &files {
            copied_bytes = charge_file(copied_bytes, file, MAX_COMPONENT_IMAGE_BYTES)?;
            physical_objects = charge(
                physical_objects,
                file.objects().objects().len(),
                MAX_COMPONENT_OBJECTS,
            )?;
        }
        Ok(Self {
            accounts,
            files,
            manifest,
            manifest_bytes,
            copied_bytes,
            physical_objects,
        })
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
/// Common archive/restore use separate APIs and grant no current user authority.
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
    accounts.capture_physical_with(|snapshot| {
        if snapshot.manifest() != &original_manifest {
            return Err(Error::ComponentSnapshot("account roster changed"));
        }
        let snapshot = snapshot.into_component();
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
        check_file(&snapshot, expected)?;
        result.copied_bytes = charge_file(result.copied_bytes, &snapshot, image_limit)?;
        result.files.push(snapshot);
        capture_files(remaining, &admitted[1..], result, work, image_limit)
    })??;
    Ok(())
}
fn check_file(snapshot: &FileSnapshot, expected: &NativeFileRootInfo) -> Result<()> {
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
    Ok(())
}
fn charge_file(current: usize, snapshot: &FileSnapshot, image_limit: usize) -> Result<usize> {
    let mut copied_bytes = charge(current, snapshot.metadata_bytes().len(), image_limit)?;
    for object in snapshot.objects().objects() {
        copied_bytes = charge(
            copied_bytes,
            emilybase_object_storage::HEADER_BYTES,
            image_limit,
        )?;
        copied_bytes = charge(copied_bytes, object.payload().len(), image_limit)?;
    }
    Ok(copied_bytes)
}

fn check_database_ids(
    accounts: &AccountComponentSnapshot,
    files: &[NativeFileRootInfo],
) -> Result<()> {
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
// Original native owners and all physical payload readers enclose the account
// capture, publication and each subsequent source proof. Recursion is capped at4.
pub(crate) fn with_owned_files<T>(
    roots: &mut [NativeFileRoot],
    manifest: &AccountFileRootManifest,
    work: impl FnOnce(Vec<FileSnapshot>, Vec<NativeFileRootInfo>) -> Result<T>,
) -> Result<T> {
    if roots.len() > MAX_COMPONENT_FILE_STORES || roots.len() != manifest.file_stores().len() {
        return Err(Error::Limit);
    }
    let mut admitted = Vec::with_capacity(roots.len());
    let mut objects = 0;
    for (root, binding) in roots.iter_mut().zip(manifest.file_stores()) {
        let info = root.inspect()?;
        if info.project().to_string() != binding.project()
            || hex(info.database_id()) != binding.database_id()
        {
            return Err(Error::ComponentSnapshot("original file binding changed"));
        }
        objects = charge(
            objects,
            info.usage().physical_objects,
            MAX_COMPONENT_OBJECTS,
        )?;
        admitted.push(info);
    }
    checkpoint("components_admitted");
    retain_owned(roots, &admitted, Vec::new(), 0, |files| {
        work(files, admitted.clone())
    })
}
fn retain_owned<T>(
    roots: &mut [NativeFileRoot],
    admitted: &[NativeFileRootInfo],
    mut files: Vec<FileSnapshot>,
    copied: usize,
    work: impl FnOnce(Vec<FileSnapshot>) -> Result<T>,
) -> Result<T> {
    let Some((root, rest)) = roots.split_first_mut() else {
        checkpoint("components_retained");
        return work(files);
    };
    root.with_snapshot(|snapshot| {
        check_file(&snapshot, &admitted[0])?;
        let copied = charge_file(copied, &snapshot, MAX_COMPONENT_IMAGE_BYTES)?;
        files.push(snapshot);
        retain_owned(rest, &admitted[1..], files, copied, work)
    })?
}
pub(crate) fn unique_ids(ids: impl Iterator<Item = [u8; 16]>) -> Result<()> {
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
pub(crate) fn charge(current: usize, additional: usize, limit: usize) -> Result<usize> {
    current
        .checked_add(additional)
        .filter(|&total| total <= limit)
        .ok_or(Error::Limit)
}
pub(crate) fn hex(id: &[u8; 16]) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn checkpoint(_point: &str) {
    #[cfg(test)]
    crate::durability::checkpoint(_point);
}

#[cfg(test)]
mod tests;
