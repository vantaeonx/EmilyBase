use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use emilybase_transactions::Database;
use rustix::fs::{Mode, OFlags};

use crate::directory::{Destination, descriptor_path, sync};
use crate::{Error, HEADER_SIZE, Report, Result, files, inspect_bytes, publish};

/// Validate, replay and sync privately; publish a complete directory atomically.
pub fn restore(backup: impl AsRef<Path>, target: impl AsRef<Path>) -> Result<Report> {
    restore_with(backup.as_ref(), target.as_ref(), || {}, || {})
}

pub(crate) fn restore_with(
    backup: &Path,
    target: &Path,
    synced: impl FnOnce(),
    published: impl FnOnce(),
) -> Result<Report> {
    let bytes = files::read(backup)?;
    let report = inspect_bytes(&bytes)?;
    let mut pending = PendingDirectory::new(target)?;
    let fd = rustix::fs::openat(
        &pending.owner,
        "redo.wal",
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(std::io::Error::from)?;
    let mut file = File::from(fd);
    file.write_all(&bytes[HEADER_SIZE..])?;
    sync(&file, "restore_wal_sync")?;
    drop(file);
    // The legacy path-based engine is anchored through our live directory handle.
    let path = descriptor_path(&pending.owner).join(".");
    let mut database = Database::open_bound(&path, Some(report.database_id))?;
    if database.committed_wal()? != bytes[HEADER_SIZE..] {
        return Err(Error::Format(
            "restored journal differs from verified archive",
        ));
    }
    database.checkpoint()?;
    drop(database);
    sync(&pending.owner, "restore_directory_sync")?;
    synced();
    pending.publish(published)?;
    Ok(report)
}

struct PendingDirectory {
    destination: Destination,
    owner: File,
    name: OsString,
    published: bool,
}

impl PendingDirectory {
    fn new(target: &Path) -> Result<Self> {
        let destination = Destination::open(target)?;
        for _ in 0..32 {
            let name = publish::temporary_name()?;
            match rustix::fs::mkdirat(&destination.parent, &name, Mode::RWXU) {
                Ok(()) => {
                    let fd = rustix::fs::openat(
                        &destination.parent,
                        &name,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(std::io::Error::from)?;
                    return Ok(Self {
                        destination,
                        owner: fd.into(),
                        name,
                        published: false,
                    });
                }
                Err(rustix::io::Errno::EXIST) => (),
                Err(error) => return Err(std::io::Error::from(error).into()),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "temporary directory collision limit",
        )
        .into())
    }

    fn publish(&mut self, published: impl FnOnce()) -> Result<()> {
        self.destination.check()?;
        if !self.destination.owns(&self.name, &self.owner) {
            return Err(Error::PathChanged);
        }
        self.destination.publish(&self.name)?;
        self.published = true;
        published();
        self.destination
            .finish(&self.owner)
            .map_err(Error::PublicationUnknown)
    }
}

impl Drop for PendingDirectory {
    fn drop(&mut self) {
        if !self.published && self.destination.owns(&self.name, &self.owner) {
            let _ = std::fs::remove_dir_all(self.destination.entry_path(&self.name));
        }
    }
}
