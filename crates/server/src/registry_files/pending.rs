//! Identity-bound staging for archive files and registry root publication.
use super::{parent, sync};
use crate::{Error, Result, metadata};
use std::ffi::OsString;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

struct Parent {
    owner: File,
    path: PathBuf,
    target: OsString,
}
impl Parent {
    fn new(target: &Path) -> Result<Self> {
        let name = target.file_name().ok_or(Error::Path)?.to_os_string();
        let path = if parent(target).is_absolute() {
            parent(target).to_path_buf()
        } else {
            std::env::current_dir()?.join(parent(target))
        };
        let fd = rustix::fs::open(
            &path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(std::io::Error::from)?;
        let parent = Self {
            owner: fd.into(),
            path,
            target: name,
        };
        parent.check()?;
        Ok(parent)
    }
    fn check(&self) -> Result<()> {
        let current = std::fs::symlink_metadata(&self.path).map_err(|_| Error::Path)?;
        let actual = self.owner.metadata()?;
        if !current.is_dir() || (current.dev(), current.ino()) != (actual.dev(), actual.ino()) {
            return Err(Error::Path);
        }
        Ok(())
    }
    fn owns(&self, name: &std::ffi::OsStr, owner: &File) -> bool {
        let Ok(current) =
            rustix::fs::statat(&self.owner, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW)
        else {
            return false;
        };
        let Ok(actual) = owner.metadata() else {
            return false;
        };
        (current.st_dev, current.st_ino) == (actual.dev(), actual.ino())
    }
}

pub(super) struct Pending {
    parent: Parent,
    name: OsString,
    pub owner: File,
    directory: bool,
    published: bool,
}
impl Pending {
    pub fn file(target: &Path, prefix: &str) -> Result<Self> {
        let parent = Parent::new(target)?;
        let temporary = tempfile::Builder::new()
            .prefix(prefix)
            .tempfile_in(descriptor_path(&parent.owner))?;
        // Disable pathname-based cleanup before exposing any test/publication boundary.
        let (owner, path) = temporary.keep().map_err(|error| error.error)?;
        let name = path.file_name().ok_or(Error::Path)?.to_os_string();
        Ok(Self {
            parent,
            name,
            owner,
            directory: false,
            published: false,
        })
    }
    pub fn directory(target: &Path) -> Result<Self> {
        let parent = Parent::new(target)?;
        let temporary = tempfile::Builder::new()
            .prefix(".emilybase-registry-restore-")
            .tempdir_in(descriptor_path(&parent.owner))?;
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))?;
        let owner = metadata::open_directory(temporary.path())?;
        let path = temporary.keep();
        let name = path.file_name().ok_or(Error::Path)?.to_os_string();
        Ok(Self {
            parent,
            name,
            owner,
            directory: true,
            published: false,
        })
    }
    pub fn path(&self) -> PathBuf {
        // Final dot denotes the owned real directory, not the proc descriptor symlink.
        descriptor_path(&self.owner).join(".")
    }
    fn check(&self) -> Result<()> {
        self.parent.check()?;
        if !self.parent.owns(&self.name, &self.owner) {
            return Err(Error::Path);
        }
        Ok(())
    }
    pub fn publish(&mut self) -> Result<()> {
        self.check()?;
        rustix::fs::renameat_with(
            &self.parent.owner,
            &self.name,
            &self.parent.owner,
            &self.parent.target,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(std::io::Error::from)?;
        self.published = true;
        Ok(())
    }
    pub fn finish(&self, phase: &str) -> Result<()> {
        sync(&self.parent.owner, phase).map_err(Error::PublicationUnknown)?;
        if self.parent.check().is_err() || !self.parent.owns(&self.parent.target, &self.owner) {
            return Err(Error::PublicationUnknown(std::io::Error::other(
                "selected publication destination changed",
            )));
        }
        Ok(())
    }
}
impl Drop for Pending {
    fn drop(&mut self) {
        if self.published || !self.parent.owns(&self.name, &self.owner) {
            return;
        }
        if self.directory {
            let _ = std::fs::remove_dir_all(descriptor_path(&self.parent.owner).join(&self.name));
        } else {
            let _ =
                rustix::fs::unlinkat(&self.parent.owner, &self.name, rustix::fs::AtFlags::empty());
        }
    }
}
fn descriptor_path(owner: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", owner.as_raw_fd()))
}
