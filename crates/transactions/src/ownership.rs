use rustix::fs::{AtFlags, Mode, OFlags};
use std::ffi::OsString;
use std::fs::{File, TryLockError};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;

use crate::{Error, Result};

/// The directory inode remains stable when the journal is atomically replaced.
/// Keep this owner alive until journal handles are released. Ancestors are trusted.
pub(crate) fn lock_directory(path: &Path) -> Result<File> {
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    lock(fd.into())
}

fn lock(file: File) -> Result<File> {
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(Error::Wal(emilybase_wal::Error::Busy)),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}

pub(crate) fn absolute(path: &Path) -> Result<PathBuf> {
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    })
}

pub(crate) fn verify_path(path: &Path, file: &File) -> Result<()> {
    let visible = std::fs::symlink_metadata(path).map_err(|_| Error::DirectoryChanged)?;
    let owned = file.metadata()?;
    if !visible.is_dir() || (visible.dev(), visible.ino()) != (owned.dev(), owned.ino()) {
        return Err(Error::DirectoryChanged);
    }
    Ok(())
}

pub(crate) fn wal_file(directory: &File, create: bool) -> Result<File> {
    let flags = OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let flags = if create {
        flags | OFlags::CREATE | OFlags::EXCL
    } else {
        flags
    };
    let fd = rustix::fs::openat(directory, "redo.wal", flags, Mode::RUSR | Mode::WUSR)
        .map_err(std::io::Error::from)?;
    Ok(fd.into())
}

pub(crate) struct Created {
    pub owner: File,
    parent: File,
    parent_path: PathBuf,
    name: OsString,
    path: PathBuf,
}

impl Created {
    pub fn new(path: &Path) -> Result<Self> {
        let name = path
            .file_name()
            .ok_or(Error::DirectoryChanged)?
            .to_os_string();
        let parent_path = path.parent().ok_or(Error::DirectoryChanged)?.to_path_buf();
        let fd = rustix::fs::open(
            &parent_path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(std::io::Error::from)?;
        let parent: File = fd.into();
        rustix::fs::mkdirat(&parent, &name, Mode::RWXU).map_err(std::io::Error::from)?;
        let fd = rustix::fs::openat(
            &parent,
            &name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(std::io::Error::from)?;
        let owner = lock(fd.into())?;
        let created = Self {
            owner,
            parent,
            parent_path,
            name,
            path: path.to_path_buf(),
        };
        created.verify()?;
        Ok(created)
    }

    pub fn verify(&self) -> Result<()> {
        verify_path(&self.parent_path, &self.parent)?;
        verify_path(&self.path, &self.owner)?;
        let visible = rustix::fs::statat(&self.parent, &self.name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| Error::DirectoryChanged)?;
        let owned = self.owner.metadata()?;
        if (visible.st_dev, visible.st_ino) != (owned.dev(), owned.ino())
            || owned.mode() & 0o777 != 0o700
        {
            return Err(Error::DirectoryChanged);
        }
        Ok(())
    }

    pub fn finish(&self, wal: &emilybase_wal::Wal, probe: &File) -> Result<()> {
        let finish = || -> Result<()> {
            self.verify()?;
            crate::journal_replacement::source_selected(&self.owner, wal)?;
            if probe.metadata()?.mode() & 0o777 != 0o600 {
                return Err(Error::JournalOwnership);
            }
            sync(&self.owner, "directory_sync")?;
            sync(&self.parent, "parent_sync")?;
            self.verify()?;
            crate::journal_replacement::source_selected(&self.owner, wal)?;
            if probe.metadata()?.mode() & 0o777 != 0o600 {
                return Err(Error::JournalOwnership);
            }
            Ok(())
        };
        finish().map_err(|error| Error::InitializationUnknown(std::io::Error::other(error)))
    }
}

fn sync(file: &File, _phase: &'static str) -> std::io::Result<()> {
    #[cfg(test)]
    crate::database::initialization_tests::sync_failure(_phase, false)?;
    file.sync_all()?;
    #[cfg(test)]
    crate::database::initialization_tests::sync_failure(_phase, true)?;
    Ok(())
}
