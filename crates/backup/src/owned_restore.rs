//! Original database ownership through trusted application preparation and work.
use crate::directory::{descriptor_path, sync};
use crate::{Error, HEADER_SIZE, PreparedRestoreError, Report, Result, archive, inspect_bytes};
use emilybase_storage::StagedPrivateDirectory;
use emilybase_transactions::Database;
use rustix::fs::{AtFlags, Mode, OFlags};
use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;

/// Trusted native application wrapper that keeps the supplied original database
/// alive. Keep adapters for private schemas private: implementing this on a public
/// account type would expose mutable storage outside that type's invariants.
/// Factories must not detach or export that owner through errors or side channels.
pub trait PreparedDatabaseOwner {
    fn database(&mut self) -> &mut Database;
}
impl PreparedDatabaseOwner for Database {
    fn database(&mut self) -> &mut Database {
        self
    }
}

struct Journal {
    file: File,
    baseline: Metadata,
}
impl Journal {
    fn stamp(m: &Metadata) -> (u64, u64, u64, u32, u64, i64, i64, i64, i64) {
        (
            m.dev(),
            m.ino(),
            m.len(),
            m.mode(),
            m.nlink(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )
    }
    fn selected(&self, root: &File) -> Result<Metadata> {
        let owned = self.file.metadata()?;
        let visible = rustix::fs::statat(root, "redo.wal", AtFlags::SYMLINK_NOFOLLOW)
            .map_err(std::io::Error::from)?;
        if !owned.is_file()
            || owned.mode() & 0o7777 != 0o600
            || owned.nlink() != 1
            || (owned.dev(), owned.ino()) != (self.baseline.dev(), self.baseline.ino())
            || visible.st_mode & 0o170000 != 0o100000
            || (visible.st_dev, visible.st_ino) != (owned.dev(), owned.ino())
        {
            return Err(Error::PathChanged);
        }
        Ok(owned)
    }
    fn readback(&mut self, root: &File, expected: &[u8]) -> Result<Metadata> {
        let before = self.selected(root)?;
        if before.len() != expected.len() as u64 {
            return Err(Error::PathChanged);
        }
        self.file.seek(SeekFrom::Start(0))?;
        let mut scratch = [0; 8192];
        for bytes in expected.chunks(scratch.len()) {
            self.file.read_exact(&mut scratch[..bytes.len()])?;
            if scratch[..bytes.len()] != *bytes {
                return Err(Error::Format("prepared original WAL changed"));
            }
        }
        let after = self.selected(root)?;
        if Self::stamp(&before) != Self::stamp(&after) {
            return Err(Error::PathChanged);
        }
        Ok(after)
    }
    fn verify(&mut self, root: &File, expected: &[u8]) -> Result<()> {
        let now = self.readback(root, expected)?;
        if Self::stamp(&now) != Self::stamp(&self.baseline) {
            return Err(Error::PathChanged);
        }
        Ok(())
    }
    fn accept_prepared(&mut self, root: &File, expected: &[u8]) -> Result<()> {
        // Only exact acknowledged readback from this same original inode can
        // advance the baseline after intentional application preparation.
        self.baseline = self.readback(root, expected)?;
        Ok(())
    }
}
fn private(file: &File) -> Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_dir() || metadata.mode() & 0o7777 != 0o700 {
        return Err(Error::Path);
    }
    Ok(())
}
fn verify(
    owner: &mut impl PreparedDatabaseOwner,
    root: &File,
    parent: &File,
    journal: &mut Journal,
    expected: &[u8],
) -> Result<()> {
    private(parent)?;
    private(root)?;
    owner.database().check_directory_owner(root)?;
    journal.verify(root, expected)?;
    if owner.database().committed_wal()? != expected {
        return Err(Error::Format("prepared history changed during work"));
    }
    owner.database().check_directory_owner(root)?;
    journal.verify(root, expected)?;
    private(parent)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Boundary {
    Source,
    Prepared,
    Selected,
    Worked,
}

/// Prepare an existing immutable backup through a trusted owner factory, without
/// closing/reopening its actual database, then select one fresh private child and
/// run readonly coordination under that same owner. Preparation may append commits;
/// original identity/WAL inode and the full source prefix must remain. Compaction is
/// unsupported here. Work values wait for final original-file/history proofs; a
/// work Result is a nested value. Postselection failure is RetainedRestoreUnknown.
/// No private schema, current user authority or combined-root restore is inferred.
pub fn with_prepared_owner_bytes_at<O: PreparedDatabaseOwner, T, E>(
    bytes: &[u8],
    parent: &File,
    name: &str,
    prepare: impl FnOnce(Database) -> std::result::Result<O, E>,
    work: impl FnOnce(&mut O, &Report) -> T,
) -> std::result::Result<T, PreparedRestoreError<E>> {
    with_boundary(bytes, parent, name, prepare, work, |_| {})
}

fn with_boundary<O: PreparedDatabaseOwner, T, E>(
    bytes: &[u8],
    parent: &File,
    name: &str,
    prepare: impl FnOnce(Database) -> std::result::Result<O, E>,
    work: impl FnOnce(&mut O, &Report) -> T,
    mut boundary: impl FnMut(Boundary),
) -> std::result::Result<T, PreparedRestoreError<E>> {
    let backup = PreparedRestoreError::Backup;
    let source = inspect_bytes(bytes).map_err(backup)?;
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.len() > 255
        || name.contains('/')
        || name.as_bytes().contains(&0)
    {
        return Err(backup(Error::Path));
    }
    private(parent).map_err(backup)?;
    let stage = StagedPrivateDirectory::at(parent, name).map_err(|e| backup(e.into()))?;
    let root = stage
        .directory()
        .try_clone()
        .map_err(|e| backup(e.into()))?;
    let mut file = File::from(
        rustix::fs::openat(
            &root,
            "redo.wal",
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|e| backup(std::io::Error::from(e).into()))?,
    );
    let input = &bytes[HEADER_SIZE..];
    file.write_all(input).map_err(|e| backup(e.into()))?;
    sync(&file, "owned_restore_input_sync").map_err(|e| backup(e.into()))?;
    let baseline = file.metadata().map_err(|e| backup(e.into()))?;
    let mut journal = Journal { file, baseline };
    journal.verify(&root, input).map_err(backup)?;
    let database = Database::open_bound(descriptor_path(&root).join("."), Some(source.database_id))
        .map_err(|e| backup(e.into()))?;
    database
        .check_directory_owner(&root)
        .map_err(|e| backup(e.into()))?;
    boundary(Boundary::Source);
    stage.check().map_err(|e| backup(e.into()))?;
    journal.verify(&root, input).map_err(backup)?;
    let mut owner = prepare(database).map_err(PreparedRestoreError::Preparation)?;
    owner
        .database()
        .check_directory_owner(&root)
        .map_err(|e| backup(e.into()))?;
    let expected = owner
        .database()
        .committed_wal()
        .map_err(|e| backup(e.into()))?;
    if owner.database().database_id() != source.database_id || !expected.starts_with(input) {
        return Err(backup(Error::Format(
            "prepared owner or source prefix changed",
        )));
    }
    let report = archive::report(
        &emilybase_transactions::recover_image(&expected, Some(source.database_id))
            .map_err(|e| backup(e.into()))?,
    )
    .map_err(backup)?;
    journal.accept_prepared(&root, &expected).map_err(backup)?;
    sync(&journal.file, "owned_restore_prepared_sync").map_err(|e| backup(e.into()))?;
    owner
        .database()
        .checkpoint()
        .map_err(|e| backup(e.into()))?;
    boundary(Boundary::Prepared);
    verify(&mut owner, &root, parent, &mut journal, &expected).map_err(backup)?;
    stage.check().map_err(|e| backup(e.into()))?;
    sync(&root, "owned_restore_stage_sync").map_err(|e| backup(e.into()))?;
    let selected = match stage.publish() {
        Ok(selected) => selected,
        Err(error @ emilybase_storage::Error::PublicationUnknown(_)) => {
            return Err(backup(Error::RetainedRestoreUnknown(Box::new(
                error.into(),
            ))));
        }
        Err(error) => return Err(backup(error.into())),
    };
    boundary(Boundary::Selected);
    let check = |owner: &mut O, journal: &mut Journal| -> Result<()> {
        selected.check()?;
        verify(owner, &root, parent, journal, &expected)?;
        selected.check()?;
        Ok(())
    };
    check(&mut owner, &mut journal)
        .map_err(|e| backup(Error::RetainedRestoreUnknown(Box::new(e))))?;
    let value = work(&mut owner, &report);
    boundary(Boundary::Worked);
    let finish = (|| -> Result<()> {
        check(&mut owner, &mut journal)?;
        sync(&journal.file, "owned_restore_confirm_wal_sync")?;
        sync(parent, "owned_restore_confirm_parent_sync")?;
        check(&mut owner, &mut journal)
    })();
    finish.map_err(|e| backup(Error::RetainedRestoreUnknown(Box::new(e))))?;
    Ok(value)
}

#[cfg(all(test, target_os = "linux"))]
pub(crate) mod tests;
