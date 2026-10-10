use super::{FileArchiveReader, MAX_FILE_ARCHIVE_BYTES, VerifiedFileArchive, verify_file_archive};
use crate::{Error, FileQuota, FileSnapshot, Result};
use emilybase_object_storage::{ArchiveReport, ProjectId};
use rustix::fs::{AtFlags, Mode, OFlags};
use std::ffi::OsString;
use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::path::{Path, PathBuf};

/// Copied checked metadata, never a retained destination or current user grant.
#[derive(Clone, PartialEq, Eq)]
pub struct FileArchiveReport {
    project: ProjectId,
    metadata: emilybase_backup::Report,
    quota: FileQuota,
    references: usize,
    objects: ArchiveReport,
}
impl FileArchiveReport {
    pub const fn project(&self) -> ProjectId {
        self.project
    }
    pub const fn metadata(&self) -> &emilybase_backup::Report {
        &self.metadata
    }
    pub const fn quota(&self) -> FileQuota {
        self.quota
    }
    pub const fn references(&self) -> usize {
        self.references
    }
    pub const fn objects(&self) -> &ArchiveReport {
        &self.objects
    }
    fn from_verified(view: &VerifiedFileArchive<'_>) -> Self {
        Self {
            project: view.project(),
            metadata: view.metadata_report().clone(),
            quota: view.quota(),
            references: view.files().len(),
            objects: ArchiveReport {
                objects: view.objects().objects().len(),
                payload_bytes: view.objects().payload_bytes(),
                digest: *view.objects().digest(),
            },
        }
    }
}
impl std::fmt::Debug for FileArchiveReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileArchiveReport")
            .field("references", &self.references)
            .field("objects", &self.objects.objects)
            .field("payload_bytes", &self.objects.payload_bytes)
            .finish_non_exhaustive()
    }
}
struct Target {
    parent: File,
    path: PathBuf,
    name: OsString,
}
impl Target {
    fn new(path: &Path) -> Result<Self> {
        let name = path.file_name().ok_or(Error::Destination)?.to_os_string();
        if name.as_bytes().contains(&0) {
            return Err(Error::Destination);
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let path = if parent.is_absolute() {
            parent.to_path_buf()
        } else {
            std::env::current_dir()?.join(parent)
        };
        let fd = rustix::fs::open(
            &path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(std::io::Error::from)?;
        let target = Self {
            parent: fd.into(),
            path,
            name,
        };
        target.check()?;
        Ok(target)
    }
    fn check(&self) -> Result<()> {
        let visible = std::fs::symlink_metadata(&self.path)?;
        let owned = self.parent.metadata()?;
        if !visible.is_dir() || (visible.dev(), visible.ino()) != (owned.dev(), owned.ino()) {
            return Err(Error::Destination);
        }
        Ok(())
    }
    fn visible(&self, expected: &Metadata) -> Result<()> {
        let visible = rustix::fs::statat(&self.parent, &self.name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(std::io::Error::from)?;
        if (visible.st_dev, visible.st_ino) != (expected.dev(), expected.ino())
            || visible.st_mode & 0o170000 != 0o100000
            || !matches!(visible.st_mode & 0o777, 0o600 | 0o400)
            || visible.st_nlink != 1
        {
            return Err(Error::Destination);
        }
        self.check()
    }
}
fn private(file: &File) -> Result<Metadata> {
    let m = file.metadata()?;
    if !m.is_file()
        || !matches!(m.mode() & 0o777, 0o600 | 0o400)
        || m.nlink() != 1
        || m.len() > MAX_FILE_ARCHIVE_BYTES as u64
    {
        return Err(Error::Destination);
    }
    Ok(m)
}
fn unchanged(file: &File, before: &Metadata) -> Result<()> {
    let after = private(file)?;
    if (
        before.dev(),
        before.ino(),
        before.len(),
        before.mtime(),
        before.mtime_nsec(),
        before.ctime(),
        before.ctime_nsec(),
    ) != (
        after.dev(),
        after.ino(),
        after.len(),
        after.mtime(),
        after.mtime_nsec(),
        after.ctime(),
        after.ctime_nsec(),
    ) {
        return Err(Error::Destination);
    }
    Ok(())
}
fn inspect(
    target: &Target,
    file: &mut File,
    project: ProjectId,
    expected: Option<&FileSnapshot>,
) -> Result<FileArchiveReport> {
    inspect_with(target, file, project, expected, || {}, || {})
}
fn inspect_with(
    target: &Target,
    file: &mut File,
    project: ProjectId,
    expected: Option<&FileSnapshot>,
    read: impl FnOnce(),
    decoded: impl FnOnce(),
) -> Result<FileArchiveReport> {
    target.check()?;
    let before = private(file)?;
    target.visible(&before)?;
    let length = before.len() as usize;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| Error::Allocation)?;
    bytes.resize(length, 0);
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut bytes)?;
    let mut probe = [0; 1];
    loop {
        match file.read(&mut probe) {
            Ok(0) => break,
            Ok(_) => return Err(Error::Archive),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    read();
    unchanged(file, &before)?;
    target.visible(&before)?;
    let view = verify_file_archive(&bytes, project)?;
    if let Some(snapshot) = expected {
        let mut reader = FileArchiveReader::from_snapshot(snapshot)?;
        if reader.encoded_bytes() != bytes.len() {
            return Err(Error::Archive);
        }
        let mut scratch = [0; 8192];
        let mut position = 0;
        while position < bytes.len() {
            let count = (bytes.len() - position).min(scratch.len());
            reader.read_exact(&mut scratch[..count])?;
            if scratch[..count] != bytes[position..position + count] {
                return Err(Error::Archive);
            }
            position += count;
        }
    }
    let report = FileArchiveReport::from_verified(&view);
    decoded();
    unchanged(file, &before)?;
    target.visible(&before)?;
    Ok(report)
}

/// Native operator publication through original private no-replace staging,
/// fsync, exact readback and parent fsync. Choose output outside live inventories.
/// Retain selected actual inode/parent through complete semantic/canonical readback.
/// Late failure is OutcomeUnknown: inspect, never overwrite or blindly retry.
pub fn publish_file_archive(
    snapshot: &FileSnapshot,
    path: impl AsRef<Path>,
) -> Result<FileArchiveReport> {
    publish_with(snapshot, path.as_ref(), || {}, || {})
}
fn publish_with(
    snapshot: &FileSnapshot,
    path: &Path,
    prepared: impl FnOnce(),
    selected: impl FnOnce(),
) -> Result<FileArchiveReport> {
    let target = Target::new(path)?;
    let mut reader = FileArchiveReader::from_snapshot(snapshot)?;
    prepared();
    target.check()?;
    let length = reader.encoded_bytes();
    let mut file = match emilybase_storage::publish_private_reader_at_retained(
        &target.parent,
        &target.name,
        &mut reader,
        length,
        MAX_FILE_ARCHIVE_BYTES,
    ) {
        Ok(file) => file,
        Err(error @ emilybase_storage::Error::PublicationUnknown(_)) => {
            return Err(Error::OutcomeUnknown(Box::new(error.into())));
        }
        Err(error) => return Err(error.into()),
    };
    selected();
    let report = inspect(&target, &mut file, snapshot.project(), Some(snapshot))
        .map_err(|e| Error::OutcomeUnknown(Box::new(e)))?;
    Ok(report)
}
/// Readonly bounded inspection of a regular private no-follow file. Full decoding
/// may allocate an archive image and replayed metadata, not a global heap budget.
pub fn inspect_file_archive(
    path: impl AsRef<Path>,
    project: ProjectId,
) -> Result<FileArchiveReport> {
    let target = Target::new(path.as_ref())?;
    let fd = rustix::fs::openat(
        &target.parent,
        &target.name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    inspect(&target, &mut File::from(fd), project, None)
}

#[cfg(test)]
mod tests;
