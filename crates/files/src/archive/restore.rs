use super::publication::{private, unchanged};
use super::{FileArchiveReport, VerifiedFileArchive, verify_file_archive};
use crate::{Error, FileStore, Result, records};
use emilybase_object_storage::{Inventory, ObjectReader, ProjectDirectory, ProjectId};
use emilybase_storage::StagedPrivateDirectory;
use emilybase_transactions::Database;
use rustix::fs::{AtFlags, Mode, OFlags};
use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub(super) fn descriptor_path(file: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd())).join(".")
}
pub(super) struct Child {
    pub(super) directory: File,
    name: &'static str,
}
impl Child {
    pub(super) fn open(root: &File, name: &'static str) -> Result<Self> {
        let fd = rustix::fs::openat(
            root,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(std::io::Error::from)?;
        let child = Self {
            directory: fd.into(),
            name,
        };
        child.check(root)?;
        Ok(child)
    }
    pub(super) fn check(&self, root: &File) -> Result<()> {
        let owned = self.directory.metadata()?;
        let visible = rustix::fs::statat(root, self.name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(std::io::Error::from)?;
        if !owned.is_dir()
            || owned.mode() & 0o777 != 0o700
            || visible.st_mode & 0o170000 != 0o040000
            || visible.st_mode & 0o777 != 0o700
            || (visible.st_dev, visible.st_ino) != (owned.dev(), owned.ino())
        {
            return Err(Error::Destination);
        }
        Ok(())
    }
}
pub(super) struct PairGuard<'a> {
    root: File,
    metadata: Child,
    objects: Child,
    wal: File,
    baseline: Metadata,
    expected_wal: &'a [u8],
}
impl<'a> PairGuard<'a> {
    pub(super) fn new(root: File, expected_wal: &'a [u8]) -> Result<Self> {
        let metadata = Child::open(&root, "metadata")?;
        let objects = Child::open(&root, "objects")?;
        let fd = rustix::fs::openat(
            &metadata.directory,
            "redo.wal",
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(std::io::Error::from)?;
        let wal = File::from(fd);
        let baseline = private(&wal)?;
        if baseline.len() != expected_wal.len() as u64 {
            return Err(Error::Corrupt);
        }
        let mut guard = Self {
            root,
            metadata,
            objects,
            wal,
            baseline,
            expected_wal,
        };
        guard.check()?;
        Ok(guard)
    }
    pub(super) fn check(&mut self) -> Result<()> {
        let root = self.root.metadata()?;
        if !root.is_dir() || root.mode() & 0o777 != 0o700 {
            return Err(Error::Destination);
        }
        let mut seen = 0u8;
        for (count, entry) in rustix::fs::Dir::read_from(&self.root)
            .map_err(std::io::Error::from)?
            .enumerate()
        {
            if count >= 4 {
                return Err(Error::Corrupt);
            }
            let entry = entry.map_err(std::io::Error::from)?;
            let bit = match entry.file_name().to_bytes() {
                b"." | b".." => continue,
                b"metadata" => 1,
                b"objects" => 2,
                _ => return Err(Error::Corrupt),
            };
            if seen & bit != 0 {
                return Err(Error::Corrupt);
            }
            seen |= bit;
        }
        if seen != 3 {
            return Err(Error::Corrupt);
        }
        self.metadata.check(&self.root)?;
        self.objects.check(&self.root)?;
        unchanged(&self.wal, &self.baseline)?;
        self.wal_visible()?;
        self.wal.seek(SeekFrom::Start(0))?;
        let mut scratch = [0; 8192];
        let mut position = 0;
        while position < self.expected_wal.len() {
            let count = (self.expected_wal.len() - position).min(scratch.len());
            self.wal.read_exact(&mut scratch[..count])?;
            if scratch[..count] != self.expected_wal[position..position + count] {
                return Err(Error::Corrupt);
            }
            position += count;
        }
        unchanged(&self.wal, &self.baseline)?;
        self.wal_visible()?;
        self.metadata.check(&self.root)?;
        self.objects.check(&self.root)?;
        Ok(())
    }
    fn wal_visible(&self) -> Result<()> {
        let visible = rustix::fs::statat(
            &self.metadata.directory,
            "redo.wal",
            AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(std::io::Error::from)?;
        if visible.st_mode & 0o170000 != 0o100000
            || !matches!(visible.st_mode & 0o777, 0o600 | 0o400)
            || visible.st_nlink != 1
            || (visible.st_dev, visible.st_ino) != (self.baseline.dev(), self.baseline.ino())
        {
            return Err(Error::Destination);
        }
        Ok(())
    }
}
fn verify_pair(
    guard: &mut PairGuard<'_>,
    database: &mut Database,
    objects: &ProjectDirectory,
    readers: &mut [ObjectReader<'_>],
    inventory: &Inventory,
    expected: &VerifiedFileArchive<'_>,
) -> Result<()> {
    guard.check()?;
    for reader in readers {
        reader.verify()?;
    }
    if objects.inventory()? != *inventory
        || database.committed_wal()?.as_slice() != guard.expected_wal
    {
        return Err(Error::Corrupt);
    }
    let (quota, files) = records::metadata(database, expected.project())?;
    if quota != expected.quota() || files.as_slice() != expected.files() {
        return Err(Error::Corrupt);
    }
    guard.check()
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestoreBoundary {
    Metadata,
    Objects,
    Owned,
    Selected,
}

/// Verify the entire immutable pair before staging, restore both existing native
/// components privately, retain all original restored owners/files, then publish
/// one fresh common0700 root without replacement. Post-selection failure is
/// OutcomeUnknown; private nonempty stages and uncertain selected roots remain.
/// Native Linux operator paths only; no rescoping, account or current user grant.
pub fn restore_file_archive(
    bytes: &[u8],
    project: ProjectId,
    destination: impl AsRef<Path>,
) -> Result<FileArchiveReport> {
    restore_with(bytes, project, destination.as_ref(), |_| {})
}
fn restore_with(
    bytes: &[u8],
    project: ProjectId,
    destination: &Path,
    mut boundary: impl FnMut(RestoreBoundary),
) -> Result<FileArchiveReport> {
    restore_checked(bytes, project, destination, &mut boundary, || Ok(()))
}
pub(super) fn restore_checked(
    bytes: &[u8],
    project: ProjectId,
    destination: &Path,
    mut boundary: impl FnMut(RestoreBoundary),
    mut check_input: impl FnMut() -> Result<()>,
) -> Result<FileArchiveReport> {
    let view = verify_file_archive(bytes, project)?;
    let expected = FileArchiveReport::from_verified(&view);
    check_input()?;
    let stage = StagedPrivateDirectory::new(destination)?;
    let root = stage.directory().try_clone()?;
    let path = descriptor_path(&root);
    let metadata = emilybase_backup::restore_bytes_at(view.metadata_bytes(), &root, "metadata")?;
    if &metadata != view.metadata_report() {
        return Err(Error::Corrupt);
    }
    boundary(RestoreBoundary::Metadata);
    let objects =
        emilybase_object_storage::restore_archive_at(view.object_bytes, project, &root, "objects")?;
    if &objects != expected.objects() {
        return Err(Error::Corrupt);
    }
    boundary(RestoreBoundary::Objects);
    let mut guard = PairGuard::new(
        root,
        &view.metadata_bytes()[emilybase_backup::HEADER_SIZE..],
    )?;
    let database = Database::open(path.join("metadata"))?;
    let objects = ProjectDirectory::open(path.join("objects"), project)?;
    let mut store = FileStore::open(database, objects)?;
    let inventory = store.objects.inventory()?;
    if inventory.entries().len() != expected.objects().objects
        || inventory.payload_bytes() != expected.objects().payload_bytes
        || inventory.digest() != &expected.objects().digest
    {
        return Err(Error::Corrupt);
    }
    let mut readers = Vec::new();
    readers
        .try_reserve_exact(inventory.entries().len())
        .map_err(|_| Error::Allocation)?;
    for entry in inventory.entries() {
        let reader = store.objects.reader(entry.object())?;
        if reader.report() != entry.report() {
            return Err(Error::Corrupt);
        }
        readers.push(reader);
    }
    boundary(RestoreBoundary::Owned);
    check_input()?;
    stage.check()?;
    verify_pair(
        &mut guard,
        &mut store.database,
        &store.objects,
        &mut readers,
        &inventory,
        &view,
    )?;
    let selected = match stage.publish() {
        Ok(selected) => selected,
        Err(error @ emilybase_storage::Error::PublicationUnknown(_)) => {
            return Err(Error::OutcomeUnknown(Box::new(error.into())));
        }
        Err(error) => return Err(error.into()),
    };
    boundary(RestoreBoundary::Selected);
    let mut finish = || -> Result<()> {
        check_input()?;
        selected.check()?;
        verify_pair(
            &mut guard,
            &mut store.database,
            &store.objects,
            &mut readers,
            &inventory,
            &view,
        )?;
        check_input()?;
        selected.check()?;
        Ok(())
    };
    finish().map_err(|e| Error::OutcomeUnknown(Box::new(e)))?;
    Ok(expected)
}

#[cfg(test)]
mod tests;
