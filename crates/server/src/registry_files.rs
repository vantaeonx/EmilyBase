//! Private, verified, no-clobber registry archive publication and offline restore.
use crate::registry_archive::{self, MAX_REGISTRY_BACKUP_BYTES};
use crate::{Error, ProjectStore, RegistryBackupReport, Result, metadata};
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

pub fn inspect_registry_backup(path: impl AsRef<Path>) -> Result<RegistryBackupReport> {
    crate::inspect_registry_backup_bytes(&read(path.as_ref())?)
}

pub(crate) fn read(path: &Path) -> Result<Vec<u8>> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    let file: File = fd.into();
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.permissions().mode() & 0o077 != 0 {
        return Err(Error::Path);
    }
    if metadata.len() > MAX_REGISTRY_BACKUP_BYTES as u64 {
        return Err(Error::Limit);
    }
    let mut bytes = Vec::new();
    file.take(MAX_REGISTRY_BACKUP_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_REGISTRY_BACKUP_BYTES {
        return Err(Error::Limit);
    }
    Ok(bytes)
}

pub(crate) fn publish(bytes: &[u8], target: &Path) -> Result<RegistryBackupReport> {
    let report = crate::inspect_registry_backup_bytes(bytes)?;
    let mut pending = tempfile::Builder::new()
        .prefix(".emilybase-registry-backup-")
        .tempfile_in(parent(target))?;
    pending.write_all(bytes)?;
    sync(pending.as_file(), "registry_backup_file_sync")?;
    checkpoint("registry_backup_file_synced");
    let written = read(pending.path())?;
    if written != bytes || crate::inspect_registry_backup_bytes(&written)? != report {
        return Err(Error::RegistryFormat("staged archive differs from source"));
    }
    rename(pending.path(), target)?;
    checkpoint("registry_backup_renamed");
    // The temporary pathname is gone. Its drop cannot unlink the selected target.
    drop(pending);
    File::open(parent(target))
        .and_then(|file| sync(&file, "registry_backup_parent_sync"))
        .map_err(Error::PublicationUnknown)?;
    checkpoint("registry_backup_parent_synced");
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
    let pending = tempfile::Builder::new()
        .prefix(".emilybase-registry-restore-")
        .tempdir_in(parent(target))?;
    std::fs::set_permissions(pending.path(), std::fs::Permissions::from_mode(0o700))?;
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
    sync(&File::open(pending.path())?, "registry_restore_stage_sync")?;
    checkpoint("registry_restore_stage_synced");
    rename(pending.path(), target)?;
    checkpoint("registry_restore_renamed");
    // A renamed private staging directory must never be deleted on uncertainty.
    let _old = pending.keep();
    File::open(parent(target))
        .and_then(|file| sync(&file, "registry_restore_parent_sync"))
        .map_err(Error::PublicationUnknown)?;
    checkpoint("registry_restore_parent_synced");
    Ok(archive.report)
}

pub(crate) fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}
fn rename(source: &Path, target: &Path) -> Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        source,
        rustix::fs::CWD,
        target,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(std::io::Error::from)?;
    Ok(())
}
fn sync(file: &File, _boundary: &str) -> std::io::Result<()> {
    #[cfg(test)]
    crate::durability::fail(_boundary)?;
    file.sync_all()
}
fn checkpoint(_boundary: &str) {
    #[cfg(test)]
    crate::durability::checkpoint(_boundary);
}
