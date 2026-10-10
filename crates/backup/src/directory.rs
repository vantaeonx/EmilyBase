//! Descriptor-relative destination operations. Local ancestor paths are trusted.
use std::ffi::OsString;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use rustix::fs::{AtFlags, Mode, OFlags};

use crate::{Error, Result};

pub(crate) struct Destination {
    pub parent: File,
    pub name: OsString,
    path: Option<PathBuf>,
}

impl Destination {
    pub fn open(target: &Path) -> Result<Self> {
        let name = target.file_name().ok_or(Error::Path)?.to_os_string();
        let parent = target
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        // Anchor relative operator input once; later cwd changes cannot redirect it.
        let path = if parent.is_absolute() {
            parent.to_path_buf()
        } else {
            std::env::current_dir()?.join(parent)
        };
        let parent = open_directory(&path)?;
        let destination = Self {
            parent,
            name,
            path: Some(path),
        };
        destination.check()?;
        Ok(destination)
    }

    pub fn check(&self) -> Result<()> {
        let expected = self.parent.metadata()?;
        if !expected.is_dir() {
            return Err(Error::PathChanged);
        }
        if let Some(path) = &self.path {
            let actual = std::fs::symlink_metadata(path).map_err(|_| Error::PathChanged)?;
            if !actual.is_dir() || actual.dev() != expected.dev() || actual.ino() != expected.ino()
            {
                return Err(Error::PathChanged);
            }
        }
        Ok(())
    }
    pub fn at(parent: &File, name: &std::ffi::OsStr) -> Result<Self> {
        use std::os::unix::ffi::OsStrExt;
        let bytes = name.as_bytes();
        if bytes.is_empty()
            || matches!(bytes, b"." | b"..")
            || bytes.contains(&0)
            || bytes.contains(&b'/')
            || !parent.metadata()?.is_dir()
        {
            return Err(Error::Path);
        }
        let result = Self {
            parent: parent.try_clone()?,
            name: name.to_os_string(),
            path: None,
        };
        result.check()?;
        Ok(result)
    }

    pub fn owns(&self, name: &std::ffi::OsStr, file: &File) -> bool {
        owns(&self.parent, name, file)
    }

    pub fn publish(&self, source: &std::ffi::OsStr) -> Result<()> {
        self.check()?;
        rename(&self.parent, source, &self.name)
    }

    pub fn finish(&self, selected: &File) -> std::io::Result<()> {
        sync(&self.parent, "parent_sync")?;
        if self.check().is_err() || !self.owns(&self.name, selected) {
            return Err(std::io::Error::other("published destination changed"));
        }
        Ok(())
    }

    pub fn entry_path(&self, name: &std::ffi::OsStr) -> PathBuf {
        descriptor_path(&self.parent).join(name)
    }
}

pub(crate) fn open_directory(path: &Path) -> Result<File> {
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    Ok(fd.into())
}

pub(crate) fn descriptor_path(file: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

pub(crate) fn owns(parent: &File, name: &std::ffi::OsStr, file: &File) -> bool {
    let Ok(actual) = rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) else {
        return false;
    };
    let Ok(expected) = file.metadata() else {
        return false;
    };
    actual.st_dev == expected.dev() && actual.st_ino == expected.ino()
}

#[cfg(target_os = "linux")]
fn rename(parent: &File, source: &std::ffi::OsStr, target: &std::ffi::OsStr) -> Result<()> {
    rustix::fs::renameat_with(
        parent,
        source,
        parent,
        target,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(std::io::Error::from)?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn rename(_parent: &File, _source: &std::ffi::OsStr, _target: &std::ffi::OsStr) -> Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "descriptor-relative no-replace backup publication requires Linux",
    )
    .into())
}

pub(crate) fn sync(file: &File, _phase: &'static str) -> std::io::Result<()> {
    #[cfg(all(test, target_os = "linux"))]
    crate::ownership_tests::sync_failure(_phase, false)?;
    file.sync_all()?;
    #[cfg(all(test, target_os = "linux"))]
    crate::ownership_tests::sync_failure(_phase, true)?;
    Ok(())
}
