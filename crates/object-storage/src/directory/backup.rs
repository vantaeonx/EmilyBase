//! Complete immutable capture followed by original owned archive publication.
use super::*;
use crate::{ArchiveReader, ArchiveReport, MAX_ARCHIVE_BYTES};
use std::ffi::OsString;
use std::io::{Seek, SeekFrom};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

struct Target {
    parent: File,
    path: PathBuf,
    name: OsString,
}
impl Target {
    fn new(path: &Path, source: &File) -> Result<Self> {
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
        let owned = target.parent.metadata()?;
        let source = source.metadata()?;
        if (owned.dev(), owned.ino()) == (source.dev(), source.ino()) {
            return Err(Error::Destination);
        }
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
    fn inspect(&self, file: &mut File, project: ProjectId) -> Result<ArchiveReport> {
        file.seek(SeekFrom::Start(0))?;
        let (report, metadata) = crate::archive::inspect_open_archive(file, project)?;
        check_visible_os(&self.parent, &self.name, &metadata)?;
        self.check()?;
        Ok(report)
    }
}
fn check_visible_os(directory: &File, name: &std::ffi::OsStr, owned: &Metadata) -> Result<()> {
    let visible = rustix::fs::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(std::io::Error::from)?;
    if (visible.st_dev, visible.st_ino) != (owned.dev(), owned.ino())
        || visible.st_mode & 0o170000 != 0o100000
        || !matches!(visible.st_mode & 0o777, 0o600 | 0o400)
        || visible.st_nlink != 1
    {
        return Err(Error::File);
    }
    Ok(())
}

impl ProjectDirectory {
    /// Publish a complete checked archive at one fresh operator-selected path.
    /// Retain the destination parent before capture; never place an archive in
    /// the source directory. No overwrite, implicit retry or root integration.
    /// PublicationUnknown requires explicit inspection of the selected target.
    pub fn backup_to(&self, path: impl AsRef<Path>) -> Result<ArchiveReport> {
        self.backup_with(path.as_ref(), || {}, || {})
    }
    fn backup_with(
        &self,
        path: &Path,
        captured: impl FnOnce(),
        selected: impl FnOnce(),
    ) -> Result<ArchiveReport> {
        let target = Target::new(path, &self.directory)?;
        let snapshot = self.capture()?;
        let mut image = ArchiveReader::from_snapshot(&snapshot)?;
        let inventory = snapshot.inventory();
        let expected = ArchiveReport {
            objects: inventory.entries().len(),
            payload_bytes: inventory.payload_bytes(),
            digest: *inventory.digest(),
        };
        captured();
        if &self.inventory()? != inventory {
            return Err(Error::InventoryChanged);
        }
        target.check()?;
        let exact_bytes = image.encoded_bytes();
        let mut selected_file = match emilybase_storage::publish_private_reader_at_retained(
            &target.parent,
            &target.name,
            &mut image,
            exact_bytes,
            MAX_ARCHIVE_BYTES,
        ) {
            Ok(file) => file,
            Err(emilybase_storage::Error::PublicationUnknown(_)) => {
                return Err(Error::PublicationUnknown);
            }
            Err(error) => return Err(Error::Publication(error)),
        };
        selected();
        match target.inspect(&mut selected_file, self.project) {
            Ok(report) if report == expected => {
                self.check().map_err(|_| Error::PublicationUnknown)?;
                Ok(report)
            }
            _ => Err(Error::PublicationUnknown),
        }
    }
}

#[cfg(test)]
mod crash_tests;
#[cfg(test)]
mod tests;
