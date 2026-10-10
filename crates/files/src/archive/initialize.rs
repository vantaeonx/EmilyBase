//! Create a complete new native pair under one original private directory stage.
use super::FileArchiveReport;
use super::restore::{Child, PairGuard, descriptor_path};
use crate::{Error, FileQuota, FileSnapshot, FileStore, Result};
use emilybase_object_storage::{ProjectDirectory, ProjectId};
use emilybase_storage::StagedPrivateDirectory;
use emilybase_transactions::Database;
use rustix::fs::Mode;
use std::fs::File;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum InitializeBoundary {
    Metadata,
    Objects,
    Catalog,
    Owned,
    Selected,
}

/// Durably select one fresh0700 metadata/object root after original-engine
/// initialization, exact readback and complete graph checks under retained native
/// owners. No overwrite, synthetic backup bootstrap, rescoping or user authority.
/// Nonempty failed private stages remain; selected uncertainty is OutcomeUnknown.
pub fn initialize_file_root(
    destination: impl AsRef<Path>,
    project: ProjectId,
    quota: FileQuota,
) -> Result<FileArchiveReport> {
    initialize_with(destination.as_ref(), project, quota, |_| {})
}
fn verify_initial(
    guard: &mut PairGuard<'_>,
    store: &mut FileStore,
    expected: &FileSnapshot,
    root: &File,
) -> Result<()> {
    guard.check()?;
    store.database.check_directory_at(root, "metadata")?;
    let current = store.capture()?;
    if current.project() != expected.project()
        || current.quota() != expected.quota()
        || current.metadata_bytes() != expected.metadata_bytes()
        || current.metadata_report() != expected.metadata_report()
        || !current.files().is_empty()
        || !current.objects().objects().is_empty()
    {
        return Err(Error::Corrupt);
    }
    store.database.check_directory_at(root, "metadata")?;
    guard.check()
}
fn initialize_with(
    destination: &Path,
    project: ProjectId,
    quota: FileQuota,
    mut boundary: impl FnMut(InitializeBoundary),
) -> Result<FileArchiveReport> {
    let stage = StagedPrivateDirectory::new(destination)?;
    let root = stage.directory().try_clone()?;
    let database = Database::create_at(&root, "metadata")?;
    database.check_directory_at(&root, "metadata")?;
    let metadata = Child::open(&root, "metadata")?;
    boundary(InitializeBoundary::Metadata);
    metadata.check(&root)?;
    rustix::fs::mkdirat(&root, "objects", Mode::RWXU).map_err(std::io::Error::from)?;
    let object_child = Child::open(&root, "objects")?;
    // Open an independent file description of the original retained child. Do
    // not clone its description into a public owner with a shared flock lifetime.
    let objects = ProjectDirectory::initialize(descriptor_path(&object_child.directory), project)?;
    object_child.check(&root)?;
    boundary(InitializeBoundary::Objects);
    let mut store = FileStore::initialize(database, objects, quota)?;
    boundary(InitializeBoundary::Catalog);
    metadata.check(&root)?;
    object_child.check(&root)?;
    let expected = store.capture()?;
    let report = FileArchiveReport::from_snapshot(&expected);
    let mut guard = PairGuard::new(
        root,
        &expected.metadata_bytes()[emilybase_backup::HEADER_SIZE..],
    )?;
    boundary(InitializeBoundary::Owned);
    stage.check()?;
    metadata.check(stage.directory())?;
    object_child.check(stage.directory())?;
    verify_initial(&mut guard, &mut store, &expected, stage.directory())?;
    let selected = match stage.publish() {
        Ok(selected) => selected,
        Err(error @ emilybase_storage::Error::PublicationUnknown(_)) => {
            return Err(Error::OutcomeUnknown(Box::new(error.into())));
        }
        Err(error) => return Err(error.into()),
    };
    boundary(InitializeBoundary::Selected);
    let mut finish = || -> Result<()> {
        selected.check()?;
        metadata.check(selected.directory())?;
        object_child.check(selected.directory())?;
        verify_initial(&mut guard, &mut store, &expected, selected.directory())?;
        selected.check()?;
        Ok(())
    };
    finish().map_err(|error| Error::OutcomeUnknown(Box::new(error)))?;
    Ok(report)
}

#[cfg(test)]
mod tests;
