//! Private descriptor-owned replacement of the mandatory journal.
use crate::{Error, Result};
use emilybase_wal::Wal;
use rustix::fs::{AtFlags, Mode, OFlags};
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::os::unix::fs::MetadataExt;

pub(crate) const STAGING: &str = "redo-next.wal";
const SELECTED: &str = "redo.wal";

pub(crate) struct Pending {
    pub file: File,
    parent: File,
    published: bool,
}

pub(crate) fn source_selected(parent: &File, source: &Wal) -> Result<()> {
    let fd = rustix::fs::openat(
        parent,
        SELECTED,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| Error::JournalOwnership)?;
    let selected: File = fd.into();
    let metadata = selected.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 || !source.owns_file(&selected)? {
        return Err(Error::JournalOwnership);
    }
    Ok(())
}

impl Pending {
    pub fn new(parent: &File) -> Result<Self> {
        // Explicit maintenance may discard only the reserved old staging entry.
        // An active operation's cleanup below checks the inode it actually created.
        match rustix::fs::unlinkat(parent, STAGING, AtFlags::empty()) {
            Ok(()) | Err(rustix::io::Errno::NOENT) => (),
            Err(error) => return Err(std::io::Error::from(error).into()),
        }
        let parent = parent.try_clone()?;
        let fd = rustix::fs::openat(
            &parent,
            STAGING,
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(std::io::Error::from)?;
        let pending = Self {
            file: fd.into(),
            parent,
            published: false,
        };
        pending.check()?;
        Ok(pending)
    }

    fn owns(&self, name: &str) -> bool {
        let Ok(visible) = rustix::fs::statat(&self.parent, name, AtFlags::SYMLINK_NOFOLLOW) else {
            return false;
        };
        let Ok(owned) = self.file.metadata() else {
            return false;
        };
        (visible.st_dev, visible.st_ino) == (owned.dev(), owned.ino())
    }

    fn admitted(&self) -> bool {
        self.file.metadata().is_ok_and(|metadata| {
            metadata.is_file() && metadata.nlink() == 1 && metadata.mode() & 0o777 == 0o600
        })
    }

    pub fn check(&self) -> Result<()> {
        if !self.owns(STAGING) || !self.admitted() {
            return Err(Error::JournalOwnership);
        }
        Ok(())
    }

    pub fn publish(&mut self) -> Result<()> {
        self.check()?;
        rustix::fs::renameat(&self.parent, STAGING, &self.parent, SELECTED)
            .map_err(std::io::Error::from)?;
        self.published = true;
        Ok(())
    }

    fn selected(&self) -> bool {
        self.owns(SELECTED) && self.admitted()
    }

    pub fn selected_image(&self, expected: &[u8]) -> std::io::Result<bool> {
        if !self.selected() || self.file.metadata()?.len() != expected.len() as u64 {
            return Ok(false);
        }
        let mut buffer = [0; emilybase_storage::PAGE_SIZE];
        for (index, chunk) in expected.chunks(buffer.len()).enumerate() {
            self.file.read_exact_at(
                &mut buffer[..chunk.len()],
                (index * emilybase_storage::PAGE_SIZE) as u64,
            )?;
            if &buffer[..chunk.len()] != chunk {
                return Ok(false);
            }
        }
        Ok(self.selected() && self.file.metadata()?.len() == expected.len() as u64)
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.published && self.owns(STAGING) {
            let _ = rustix::fs::unlinkat(&self.parent, STAGING, AtFlags::empty());
        }
    }
}
