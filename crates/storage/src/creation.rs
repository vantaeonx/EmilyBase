//! Private original page-file publication, independently of transaction commits.
use crate::{Error, Result};
use rustix::fs::{AtFlags, Mode, OFlags};
use std::ffi::OsString;
use std::fs::File;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub(crate) struct Pending {
    pub file: File,
    parent: File,
    parent_path: Option<PathBuf>,
    name: OsString,
    target: OsString,
    published: bool,
}

impl Pending {
    pub fn new(path: &Path) -> Result<Self> {
        let target = path.file_name().ok_or(Error::Path)?.to_os_string();
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
        Self::create(fd.into(), Some(parent_path), target)
    }

    pub fn at(parent: &File, target: &std::ffi::OsStr) -> Result<Self> {
        let name = Path::new(target);
        if name.components().count() != 1
            || name.file_name() != Some(target)
            || !parent.metadata()?.is_dir()
        {
            return Err(Error::Path);
        }
        Self::create(parent.try_clone()?, None, target.to_os_string())
    }

    fn create(parent: File, parent_path: Option<PathBuf>, target: OsString) -> Result<Self> {
        for _ in 0..32 {
            let mut nonce = [0; 16];
            getrandom::fill(&mut nonce).map_err(|_| Error::Randomness)?;
            let hex = nonce
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let name = OsString::from(format!(".emilybase-create-{hex}"));
            match rustix::fs::openat(
                &parent,
                &name,
                OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(fd) => {
                    let pending = Self {
                        file: fd.into(),
                        parent,
                        parent_path,
                        name,
                        target,
                        published: false,
                    };
                    pending.check()?;
                    return Ok(pending);
                }
                Err(rustix::io::Errno::EXIST) => (),
                Err(error) => return Err(std::io::Error::from(error).into()),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "temporary page-file collision limit",
        )
        .into())
    }

    fn parent_valid(&self) -> Result<()> {
        let Some(parent_path) = &self.parent_path else {
            return Ok(());
        };
        let visible = std::fs::symlink_metadata(parent_path).map_err(|_| Error::PathChanged)?;
        let owned = self.parent.metadata()?;
        if !visible.is_dir() || (visible.dev(), visible.ino()) != (owned.dev(), owned.ino()) {
            return Err(Error::PathChanged);
        }
        Ok(())
    }

    fn owns(&self, name: &std::ffi::OsStr) -> bool {
        let Ok(visible) = rustix::fs::statat(&self.parent, name, AtFlags::SYMLINK_NOFOLLOW) else {
            return false;
        };
        let Ok(owned) = self.file.metadata() else {
            return false;
        };
        (visible.st_dev, visible.st_ino) == (owned.dev(), owned.ino())
    }

    pub fn check(&self) -> Result<()> {
        self.parent_valid()?;
        if !self.owns(&self.name) {
            return Err(Error::PathChanged);
        }
        self.file_valid()?;
        Ok(())
    }

    fn file_valid(&self) -> Result<()> {
        let metadata = self.file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 || metadata.mode() & 0o777 != 0o600 {
            return Err(Error::Path);
        }
        Ok(())
    }

    pub fn publish(&mut self, published: impl FnOnce()) -> Result<()> {
        self.check()?;
        rename(&self.parent, &self.name, &self.target)?;
        self.published = true;
        published();
        sync(&self.parent, "parent_sync").map_err(Error::PublicationUnknown)?;
        if self.parent_valid().is_err() || !self.owns(&self.target) || self.file_valid().is_err() {
            return Err(Error::PublicationUnknown(std::io::Error::other(
                "published page-file destination changed",
            )));
        }
        Ok(())
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.published && self.owns(&self.name) {
            let _ = rustix::fs::unlinkat(&self.parent, &self.name, AtFlags::empty());
        }
    }
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
        "owned no-replace page-file publication requires Linux",
    )
    .into())
}

pub(crate) fn sync(file: &File, _phase: &'static str) -> std::io::Result<()> {
    #[cfg(all(test, target_os = "linux"))]
    crate::publication_tests::sync_failure(_phase, false)?;
    file.sync_all()?;
    #[cfg(all(test, target_os = "linux"))]
    crate::publication_tests::sync_failure(_phase, true)?;
    Ok(())
}
