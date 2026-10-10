//! Verify the complete input before creating a fresh native private directory.
use crate::{
    ArchiveReport, Error, MAX_ARCHIVE_BYTES, ProjectDirectory, ProjectId, Result, VerifiedArchive,
    verify_archive,
};
use std::io::{Seek, SeekFrom};
use std::path::Path;

/// Restore a complete immutable archive into one fresh operator-selected name.
/// Same project/IDs/bytes only. No overwrite, rescoping or AccountRoot integration.
pub fn restore_archive(
    bytes: &[u8],
    project: ProjectId,
    destination: impl AsRef<Path>,
) -> Result<ArchiveReport> {
    let view = verify_archive(bytes, project)?;
    restore_checked(&view, destination.as_ref(), || {}, || Ok(()), || {})
}
/// Explicit native directory descriptor and one fresh leaf, without resolving a
/// parent pathname. Original component readback/fsync/no-replace rules remain.
pub fn restore_archive_at(
    bytes: &[u8],
    project: ProjectId,
    parent: &std::fs::File,
    name: impl AsRef<std::ffi::OsStr>,
) -> Result<ArchiveReport> {
    let view = verify_archive(bytes, project)?;
    let stage =
        emilybase_storage::StagedPrivateDirectory::at(parent, name).map_err(Error::Publication)?;
    restore_staged(&view, stage, || {}, || Ok(()), || {})
}
/// Bounded private source inspection, complete reconstruction/readback and
/// no-replace directory publication. Partial private stages are preserved on
/// failure; a selected uncertain result requires explicit directory inspection.
pub fn restore_archive_file(
    path: impl AsRef<Path>,
    project: ProjectId,
    destination: impl AsRef<Path>,
) -> Result<ArchiveReport> {
    restore_file_with(path.as_ref(), project, destination.as_ref(), || {}, || {})
}
fn restore_file_with(
    path: &Path,
    project: ProjectId,
    destination: &Path,
    populated: impl FnOnce(),
    selected: impl FnOnce(),
) -> Result<ArchiveReport> {
    let mut file = crate::inspect::open_private(path)?;
    let (bytes, before) = crate::inspect::read_image(&mut file, MAX_ARCHIVE_BYTES)?;
    let view = verify_archive(&bytes, project)?;
    crate::inspect::recheck(&file, &before, MAX_ARCHIVE_BYTES)?;
    crate::inspect::check_visible(path, &before)?;
    restore_checked(
        &view,
        destination,
        populated,
        || {
            crate::inspect::recheck(&file, &before, MAX_ARCHIVE_BYTES)?;
            file.seek(SeekFrom::Start(0))?;
            let (current, after) = crate::inspect::read_image(&mut file, MAX_ARCHIVE_BYTES)?;
            if current != bytes {
                return Err(Error::Archive);
            }
            crate::inspect::check_visible(path, &after)
        },
        selected,
    )
}
fn report(view: &VerifiedArchive<'_>) -> ArchiveReport {
    ArchiveReport {
        objects: view.objects().len(),
        payload_bytes: view.payload_bytes(),
        digest: *view.digest(),
    }
}
fn matches(owner: &ProjectDirectory, expected: &ArchiveReport) -> Result<bool> {
    let inventory = owner.inventory()?;
    Ok(inventory.entries().len() == expected.objects
        && inventory.payload_bytes() == expected.payload_bytes
        && inventory.digest() == &expected.digest)
}
fn restore_checked(
    view: &VerifiedArchive<'_>,
    destination: &Path,
    populated: impl FnOnce(),
    check_input: impl FnOnce() -> Result<()>,
    selected: impl FnOnce(),
) -> Result<ArchiveReport> {
    let stage =
        emilybase_storage::StagedPrivateDirectory::new(destination).map_err(Error::Publication)?;
    restore_staged(view, stage, populated, check_input, selected)
}
fn restore_staged(
    view: &VerifiedArchive<'_>,
    stage: emilybase_storage::StagedPrivateDirectory,
    populated: impl FnOnce(),
    check_input: impl FnOnce() -> Result<()>,
    selected: impl FnOnce(),
) -> Result<ArchiveReport> {
    let expected = report(view);
    let prepare = || -> Result<ProjectDirectory> {
        let mut owner = ProjectDirectory::initialize_descriptor(stage.directory(), view.project())?;
        for object in view.objects() {
            owner.put(object.object(), object.payload())?;
        }
        populated();
        if !matches(&owner, &expected)? {
            return Err(Error::InventoryChanged);
        }
        check_input()?;
        stage.check().map_err(Error::Publication)?;
        Ok(owner)
    };
    let owner = prepare().map_err(|error| Error::RestoreStage(Box::new(error)))?;
    let published = match stage.publish() {
        Ok(published) => published,
        Err(emilybase_storage::Error::PublicationUnknown(_)) => {
            return Err(Error::PublicationUnknown);
        }
        Err(error) => return Err(Error::Publication(error)),
    };
    selected();
    if published.check().is_err()
        || !matches(&owner, &expected).unwrap_or(false)
        || published.check().is_err()
    {
        return Err(Error::PublicationUnknown);
    }
    Ok(expected)
}

#[cfg(test)]
mod crash_tests;
#[cfg(test)]
mod tests;
