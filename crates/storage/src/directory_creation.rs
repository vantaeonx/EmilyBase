//! Native owned0700 directory selection, without recursive cleanup authority.
use crate::{
    Error, Result,
    creation::{rename, sync},
};
use rustix::fs::{AtFlags, Mode, OFlags};
use std::ffi::OsString;
use std::fs::File;
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::path::{Path, PathBuf};

/// An exclusive random private stage in a retained operator-selected parent.
/// Callers must fsync and verify every child before publication. This does not
/// provide database/user authority or a lease against native administrators.
/// Drop removes only an unchanged empty owned stage; nonempty/foreign stages
/// are deliberately preserved for explicit inspection, never swept recursively.
pub struct StagedPrivateDirectory {
    directory: File,
    parent: File,
    parent_path: Option<PathBuf>,
    stage: OsString,
    target: OsString,
    published: bool,
}
/// Retained selected directory/parent identities, not a namespace lease.
pub struct PublishedPrivateDirectory {
    directory: File,
    parent: File,
    parent_path: Option<PathBuf>,
    target: OsString,
}
impl PublishedPrivateDirectory {
    pub const fn directory(&self) -> &File {
        &self.directory
    }
    pub fn check(&self) -> Result<()> {
        let parent = self.parent.metadata()?;
        if let Some(path) = &self.parent_path {
            let visible = std::fs::symlink_metadata(path).map_err(|_| Error::PathChanged)?;
            if !visible.is_dir() || (visible.dev(), visible.ino()) != (parent.dev(), parent.ino()) {
                return Err(Error::PathChanged);
            }
        }
        let owned = self.directory.metadata()?;
        let selected = rustix::fs::statat(&self.parent, &self.target, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(std::io::Error::from)?;
        if !parent.is_dir()
            || !owned.is_dir()
            || owned.mode() & 0o777 != 0o700
            || selected.st_mode & 0o170000 != 0o040000
            || selected.st_mode & 0o777 != 0o700
            || (selected.st_dev, selected.st_ino) != (owned.dev(), owned.ino())
        {
            return Err(Error::PathChanged);
        }
        Ok(())
    }
}
impl StagedPrivateDirectory {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let target = path.file_name().ok_or(Error::Path)?.to_os_string();
        if target.as_bytes().contains(&0) {
            return Err(Error::Path);
        }
        let parent_path = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent_path = if parent_path.is_absolute() {
            parent_path.to_path_buf()
        } else {
            std::env::current_dir()?.join(parent_path)
        };
        let fd = rustix::fs::open(
            &parent_path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(std::io::Error::from)?;
        let parent: File = fd.into();
        Self::create(parent, target, Some(parent_path))
    }
    /// Explicit native descriptor authority. One leaf only; moving the original
    /// parent never adopts a replacement at its old pathname. No path lease.
    pub fn at(parent: &File, target: impl AsRef<std::ffi::OsStr>) -> Result<Self> {
        let target = target.as_ref();
        let bytes = target.as_bytes();
        if bytes.is_empty()
            || matches!(bytes, b"." | b"..")
            || bytes.contains(&0)
            || bytes.contains(&b'/')
            || !parent.metadata()?.is_dir()
        {
            return Err(Error::Path);
        }
        Self::create(parent.try_clone()?, target.to_os_string(), None)
    }
    fn create(parent: File, target: OsString, parent_path: Option<PathBuf>) -> Result<Self> {
        for _ in 0..32 {
            let mut nonce = [0; 16];
            getrandom::fill(&mut nonce).map_err(|_| Error::Randomness)?;
            let stage = OsString::from(format!(
                ".emilybase-directory-{}",
                nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
            ));
            match rustix::fs::mkdirat(&parent, &stage, Mode::RUSR | Mode::WUSR | Mode::XUSR) {
                Ok(()) => (),
                Err(rustix::io::Errno::EXIST) => continue,
                Err(error) => return Err(std::io::Error::from(error).into()),
            }
            let fd = rustix::fs::openat(
                &parent,
                &stage,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(std::io::Error::from)?;
            let result = Self {
                directory: fd.into(),
                parent,
                parent_path,
                stage,
                target,
                published: false,
            };
            result.check()?;
            return Ok(result);
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "private directory stage collision limit",
        )
        .into())
    }
    pub const fn directory(&self) -> &File {
        &self.directory
    }
    fn parent_valid(&self) -> Result<()> {
        let retained = self.parent.metadata()?;
        if !retained.is_dir() {
            return Err(Error::PathChanged);
        }
        if let Some(path) = &self.parent_path {
            let visible = std::fs::symlink_metadata(path).map_err(|_| Error::PathChanged)?;
            if !visible.is_dir()
                || (visible.dev(), visible.ino()) != (retained.dev(), retained.ino())
            {
                return Err(Error::PathChanged);
            }
        }
        Ok(())
    }
    fn owns(&self, name: &std::ffi::OsStr) -> bool {
        let Ok(visible) = rustix::fs::statat(&self.parent, name, AtFlags::SYMLINK_NOFOLLOW) else {
            return false;
        };
        let Ok(owned) = self.directory.metadata() else {
            return false;
        };
        visible.st_mode & 0o170000 == 0o040000
            && (visible.st_dev, visible.st_ino) == (owned.dev(), owned.ino())
    }
    fn private(&self) -> Result<()> {
        let m = self.directory.metadata()?;
        if !m.is_dir() || m.mode() & 0o777 != 0o700 {
            return Err(Error::Path);
        }
        Ok(())
    }
    pub fn check(&self) -> Result<()> {
        self.parent_valid()?;
        self.private()?;
        if !self.owns(&self.stage) {
            return Err(Error::PathChanged);
        }
        Ok(())
    }
    /// Fsync the complete caller-verified directory, select without replacement,
    /// fsync the retained parent and return the exact selected descriptor.
    pub fn publish(self) -> Result<PublishedPrivateDirectory> {
        self.publish_with(|| {})
    }
    fn publish_with(mut self, selected: impl FnOnce()) -> Result<PublishedPrivateDirectory> {
        self.check()?;
        let retained = PublishedPrivateDirectory {
            directory: self.directory.try_clone()?,
            parent: self.parent.try_clone()?,
            parent_path: self.parent_path.clone(),
            target: self.target.clone(),
        };
        sync(&self.directory, "directory_sync")?;
        self.check()?;
        rename(&self.parent, &self.stage, &self.target)?;
        self.published = true;
        selected();
        sync(&self.parent, "parent_sync").map_err(Error::PublicationUnknown)?;
        if retained.check().is_err() {
            return Err(Error::PublicationUnknown(std::io::Error::other(
                "published private directory changed",
            )));
        }
        Ok(retained)
    }
}
impl Drop for StagedPrivateDirectory {
    fn drop(&mut self) {
        if !self.published && self.owns(&self.stage) {
            // REMOVEDIR refuses nonempty entries; no payload is recursively removed.
            let _ = rustix::fs::unlinkat(&self.parent, &self.stage, AtFlags::REMOVEDIR);
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
