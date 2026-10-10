use rustix::fs::{AtFlags, Mode, OFlags};
use std::ffi::OsString;
use std::fs::{File, TryLockError};
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
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

fn leaf(name: &std::ffi::OsStr) -> Result<()> {
    let bytes = name.as_bytes();
    if bytes.is_empty()
        || matches!(bytes, b"." | b"..")
        || bytes.contains(&0)
        || bytes.contains(&b'/')
    {
        return Err(Error::DirectoryChanged);
    }
    Ok(())
}
pub(crate) fn verify_at(owner: &File, parent: &File, name: &std::ffi::OsStr) -> Result<()> {
    leaf(name)?;
    let visible = rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| Error::DirectoryChanged)?;
    let owned = owner.metadata()?;
    if (visible.st_dev, visible.st_ino) != (owned.dev(), owned.ino())
        || !owned.is_dir()
        || visible.st_mode & 0o170000 != 0o040000
        || visible.st_mode & 0o777 != 0o700
        || owned.mode() & 0o777 != 0o700
    {
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
    parent_path: Option<PathBuf>,
    name: OsString,
    path: Option<PathBuf>,
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
        Self::create(parent, name, Some(parent_path), Some(path.to_path_buf()))
    }

    pub fn at(parent: &File, name: &std::ffi::OsStr) -> Result<Self> {
        leaf(name)?;
        if !parent.metadata()?.is_dir() {
            return Err(Error::DirectoryChanged);
        }
        Self::create(parent.try_clone()?, name.to_os_string(), None, None)
    }

    fn create(
        parent: File,
        name: OsString,
        parent_path: Option<PathBuf>,
        path: Option<PathBuf>,
    ) -> Result<Self> {
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
            path,
        };
        created.verify()?;
        Ok(created)
    }

    pub fn verify(&self) -> Result<()> {
        if let Some(path) = &self.parent_path {
            verify_path(path, &self.parent)?;
        }
        if let Some(path) = &self.path {
            verify_path(path, &self.owner)?;
        }
        verify_at(&self.owner, &self.parent, &self.name)
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
