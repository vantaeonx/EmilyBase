//! Private, verified, no-clobber registry archive publication and offline restore.
use crate::registry_archive::{self, MAX_REGISTRY_BACKUP_BYTES};
use crate::{Error, ProjectStore, RegistryBackupReport, Result, metadata};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

mod pending;

pub fn inspect_registry_backup(path: impl AsRef<Path>) -> Result<RegistryBackupReport> {
    crate::inspect_registry_backup_bytes(&read(path.as_ref())?)
}

pub(crate) fn read(path: &Path) -> Result<Vec<u8>> {
    read_bounded(path, MAX_REGISTRY_BACKUP_BYTES)
}

pub(crate) fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    let mut file: File = fd.into();
    read_file(&mut file, limit)
}

fn read_file(file: &mut File, limit: usize) -> Result<Vec<u8>> {
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.permissions().mode() & 0o077 != 0 {
        return Err(Error::Path);
    }
    if metadata.len() > limit as u64 {
        return Err(Error::Limit);
    }
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(0))?;
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(Error::Limit);
    }
    Ok(bytes)
}

#[derive(Clone, Copy)]
pub(crate) enum FileArchive {
    Registry,
    AccountBundle,
}
impl FileArchive {
    fn points(self) -> [&'static str; 5] {
        match self {
            Self::Registry => [
                "registry_backup_file_sync",
                "registry_backup_file_synced",
                "registry_backup_renamed",
                "registry_backup_parent_sync",
                "registry_backup_parent_synced",
            ],
            Self::AccountBundle => [
                "bundle_backup_file_sync",
                "bundle_backup_file_synced",
                "bundle_backup_renamed",
                "bundle_backup_parent_sync",
                "bundle_backup_parent_synced",
            ],
        }
    }
    fn limit(self) -> usize {
        match self {
            Self::Registry => MAX_REGISTRY_BACKUP_BYTES,
            Self::AccountBundle => crate::MAX_ACCOUNT_BUNDLE_BYTES,
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            Self::Registry => ".emilybase-registry-backup-",
            Self::AccountBundle => ".emilybase-account-bundle-",
        }
    }
    fn mismatch(self) -> Error {
        match self {
            Self::Registry => Error::RegistryFormat("staged archive differs from source"),
            Self::AccountBundle => Error::BundleFormat("staged archive differs from source"),
        }
    }
}
pub(crate) fn publish(bytes: &[u8], target: &Path) -> Result<RegistryBackupReport> {
    publish_checked(
        bytes,
        target,
        crate::inspect_registry_backup_bytes,
        FileArchive::Registry,
    )
}

/// Shared descriptor-owned publication. The selected parser determines the report
/// and bounded format; no payload or credential is logged by this path.
pub(crate) fn publish_checked<T: PartialEq>(
    bytes: &[u8],
    target: &Path,
    inspect: fn(&[u8]) -> Result<T>,
    format: FileArchive,
) -> Result<T> {
    if bytes.len() > format.limit() {
        return Err(Error::Limit);
    }
    let report = inspect(bytes)?;
    let points = format.points();
    let mut pending = pending::Pending::file(target, format.prefix())?;
    pending.owner.write_all(bytes)?;
    sync(&pending.owner, points[0])?;
    checkpoint(points[1]);
    let written = read_file(&mut pending.owner, format.limit())?;
    if written != bytes || inspect(&written)? != report {
        return Err(format.mismatch());
    }
    pending.publish()?;
    checkpoint(points[2]);
    pending.finish(points[3])?;
    checkpoint(points[4]);
    Ok(report)
}

/// Validate all projects before writing, then publish a complete independent registry.
/// Existing files/directories/symlinks are never replaced. Master keys are external.
pub fn restore_registry_backup(
    backup: impl AsRef<Path>,
    target: impl AsRef<Path>,
) -> Result<RegistryBackupReport> {
    let bytes = read(backup.as_ref())?;
    restore_bytes(&bytes, target.as_ref())
}

fn restore_bytes(bytes: &[u8], target: &Path) -> Result<RegistryBackupReport> {
    let archive = registry_archive::decode(bytes)?;
    let mut pending = pending::Pending::directory(target)?;
    for (entry, report) in archive.entries.iter().zip(&archive.report.projects) {
        let project = pending.path().join(&entry.metadata.id);
        std::fs::DirBuilder::new().mode(0o700).create(&project)?;
        metadata::write_new(&project.join("project.json"), &entry.metadata)?;
        let data = project.join("data");
        std::fs::DirBuilder::new().mode(0o700).create(&data)?;
        let mut wal = File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(data.join("redo.wal"))?;
        wal.write_all(&entry.database[emilybase_backup::HEADER_SIZE..])?;
        sync(&wal, "registry_restore_wal_sync")?;
        drop(wal);
        checkpoint("registry_restore_wal_synced");
        let mut database =
            emilybase_transactions::Database::open_bound(&data, Some(report.database_id))?;
        if database.committed_wal()? != entry.database[emilybase_backup::HEADER_SIZE..] {
            return Err(Error::RegistryFormat("restored WAL differs from archive"));
        }
        database.checkpoint()?;
        drop(database);
        sync(&File::open(&data)?, "registry_restore_data_sync")?;
        sync(&File::open(&project)?, "registry_restore_project_sync")?;
        checkpoint("registry_restore_project_synced");
    }
    {
        let mut restored = ProjectStore::open(pending.path())?;
        if restored.backup_image()? != bytes {
            return Err(Error::RegistryFormat(
                "restored registry differs from archive",
            ));
        }
    }
    sync(&pending.owner, "registry_restore_stage_sync")?;
    checkpoint("registry_restore_stage_synced");
    pending.publish()?;
    checkpoint("registry_restore_renamed");
    pending.finish("registry_restore_parent_sync")?;
    checkpoint("registry_restore_parent_synced");
    Ok(archive.report)
}

pub(crate) fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}
fn sync(file: &File, _boundary: &str) -> std::io::Result<()> {
    #[cfg(test)]
    crate::durability::fail(_boundary)?;
    file.sync_all()?;
    #[cfg(test)]
    crate::durability::fail(&format!("{_boundary}_after"))?;
    Ok(())
}
fn checkpoint(_boundary: &str) {
    #[cfg(test)]
    crate::durability::checkpoint(_boundary);
}

#[cfg(test)]
mod tests;
