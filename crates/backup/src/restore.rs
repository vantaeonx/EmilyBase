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

/// Restore an immutable caller-owned image without writing an intermediate archive.
/// Input bytes are sensitive; this does not authenticate their origin.
pub fn restore_bytes(bytes: &[u8], target: impl AsRef<Path>) -> Result<Report> {
    match restore_prepared_bytes(bytes, target, |_| Ok::<(), std::convert::Infallible>(())) {
        Ok(report) => Ok(report),
        Err(PreparedRestoreError::Backup(error)) => Err(error),
        Err(PreparedRestoreError::Preparation(never)) => match never {},
    }
}

/// Native owned-parent restore of one checked backup to a single fresh leaf.
/// Does not resolve/rebind a pathname for the supplied directory descriptor.
pub fn restore_bytes_at(
    bytes: &[u8],
    parent: &File,
    name: impl AsRef<std::ffi::OsStr>,
) -> Result<Report> {
    let report = inspect_bytes(bytes)?;
    let (mut pending, source) = stage_to(bytes, Destination::at(parent, name.as_ref())?, report)?;
    finish(&mut pending, &source, || {}, || {})
}

/// Trusted application preparation runs against a private, descriptor-anchored
/// directory before publication. Returned counts describe the prepared journal.
/// The callback must close all database owners before returning. It must not
/// expose its staging path or treat it as a published application store.
pub fn restore_prepared<E>(
    backup: impl AsRef<Path>,
    target: impl AsRef<Path>,
    prepare: impl FnOnce(&Path) -> std::result::Result<(), E>,
) -> std::result::Result<Report, PreparedRestoreError<E>> {
    restore_prepared_with(backup.as_ref(), target.as_ref(), prepare, || {}, || {})
}

/// Bounded byte-image entry point using the same owned preparation/publication.
/// The callback has the same private staging and owner-release obligations.
pub fn restore_prepared_bytes<E>(
    bytes: &[u8],
    target: impl AsRef<Path>,
    prepare: impl FnOnce(&Path) -> std::result::Result<(), E>,
) -> std::result::Result<Report, PreparedRestoreError<E>> {
    restore_prepared_bytes_with(bytes, target.as_ref(), prepare, || {}, || {})
}

#[derive(Debug, thiserror::Error)]
pub enum PreparedRestoreError<E> {
    #[error("prepared backup restore failed")]
    Backup(#[source] Error),
    #[error("private restore preparation failed")]
    Preparation(#[source] E),
}

pub(crate) fn restore_with(
    backup: &Path,
    target: &Path,
    synced: impl FnOnce(),
    published: impl FnOnce(),
) -> Result<Report> {
    match restore_prepared_with(
        backup,
        target,
        |_| Ok::<(), std::convert::Infallible>(()),
        synced,
        published,
    ) {
        Ok(report) => Ok(report),
        Err(PreparedRestoreError::Backup(error)) => Err(error),
        Err(PreparedRestoreError::Preparation(never)) => match never {},
    }
}

pub(crate) fn restore_prepared_with<E>(
    backup: &Path,
    target: &Path,
    prepare: impl FnOnce(&Path) -> std::result::Result<(), E>,
    synced: impl FnOnce(),
    published: impl FnOnce(),
) -> std::result::Result<Report, PreparedRestoreError<E>> {
    let bytes = files::read(backup).map_err(PreparedRestoreError::Backup)?;
    restore_prepared_bytes_with(&bytes, target, prepare, synced, published)
}

pub(crate) fn restore_prepared_bytes_with<E>(
    bytes: &[u8],
    target: &Path,
    prepare: impl FnOnce(&Path) -> std::result::Result<(), E>,
    synced: impl FnOnce(),
    published: impl FnOnce(),
) -> std::result::Result<Report, PreparedRestoreError<E>> {
    let (mut pending, source) = stage(bytes, target).map_err(PreparedRestoreError::Backup)?;
    pending.check().map_err(PreparedRestoreError::Backup)?;
    // No application credential/data is reachable under the final name yet.
    let path = descriptor_path(&pending.owner).join(".");
    prepare(&path).map_err(PreparedRestoreError::Preparation)?;
    finish(&mut pending, &source, synced, published).map_err(PreparedRestoreError::Backup)
}

fn stage(bytes: &[u8], target: &Path) -> Result<(PendingDirectory, Report)> {
    let report = inspect_bytes(bytes)?;
    stage_to(bytes, Destination::open(target)?, report)
}
fn stage_to(
    bytes: &[u8],
    destination: Destination,
    report: Report,
) -> Result<(PendingDirectory, Report)> {
    let pending = PendingDirectory::from_destination(destination)?;
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
    let path = descriptor_path(&pending.owner).join(".");
    let mut database = Database::open_bound(&path, Some(report.database_id))?;
    if database.committed_wal()? != bytes[HEADER_SIZE..] {
        return Err(Error::Format(
            "restored journal differs from verified archive",
        ));
    }
    drop(database);
    Ok((pending, report))
}

fn finish(
    pending: &mut PendingDirectory,
    source: &Report,
    synced: impl FnOnce(),
    published: impl FnOnce(),
) -> Result<Report> {
    pending.check()?;
    let path = descriptor_path(&pending.owner).join(".");
    // Reopen the selected directory, not an application-returned database handle.
    // Refuse a changed identity and replay every prepared commit before exposure.
    let mut database = Database::open_bound(&path, Some(source.database_id))?;
    let wal = database.committed_wal()?;
    let report = crate::archive::report(&emilybase_transactions::recover_image(
        &wal,
        Some(source.database_id),
    )?)?;
    drop(wal);
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
    fn from_destination(destination: Destination) -> Result<Self> {
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

    fn check(&self) -> Result<()> {
        self.destination.check()?;
        if !self.destination.owns(&self.name, &self.owner) {
            return Err(Error::PathChanged);
        }
        Ok(())
    }

    fn publish(&mut self, published: impl FnOnce()) -> Result<()> {
        self.check()?;
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
