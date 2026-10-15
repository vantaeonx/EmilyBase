//! Native restored-registry coordination; account/root adoption is separate.
use super::{checkpoint, pending::Pending, prepare_decoded, sync};
use crate::{Error, ProjectStore, RegistryBackupReport, Result, metadata, registry_archive};
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;

struct Journal<'a> {
    directory: File,
    file: File,
    baseline: Metadata,
    project: String,
    expected: &'a [u8],
}
impl<'a> Journal<'a> {
    fn new(root: &File, project: &str, expected: &'a [u8]) -> Result<Self> {
        let directory = metadata::open_directory(&Self::path(root, project))?;
        let file = File::from(
            rustix::fs::openat(
                &directory,
                "redo.wal",
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::NONBLOCK
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(std::io::Error::from)?,
        );
        let baseline = file.metadata()?;
        let mut journal = Self {
            directory,
            file,
            baseline,
            project: project.into(),
            expected,
        };
        journal.check(root)?;
        Ok(journal)
    }
    fn path(root: &File, project: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(format!("/proc/self/fd/{}/.", root.as_raw_fd()))
            .join(project)
            .join("data")
    }
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
    fn selected(&self, root: &File) -> Result<()> {
        metadata::owned_directory(&Self::path(root, &self.project), &self.directory)?;
        private(&self.directory)?;
        let current = self.file.metadata()?;
        let visible = rustix::fs::statat(
            &self.directory,
            "redo.wal",
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(std::io::Error::from)?;
        if !current.is_file()
            || current.mode() & 0o7777 != 0o600
            || current.nlink() != 1
            || current.len() != self.expected.len() as u64
            || Self::stamp(&current) != Self::stamp(&self.baseline)
            || visible.st_mode & 0o170000 != 0o100000
            || (visible.st_dev, visible.st_ino) != (current.dev(), current.ino())
        {
            return Err(Error::Path);
        }
        Ok(())
    }
    fn check(&mut self, root: &File) -> Result<()> {
        self.selected(root)?;
        self.file.seek(SeekFrom::Start(0))?;
        let mut scratch = [0; 8192];
        for expected in self.expected.chunks(scratch.len()) {
            self.file.read_exact(&mut scratch[..expected.len()])?;
            if scratch[..expected.len()] != *expected {
                return Err(Error::RegistryFormat("prepared physical WAL changed"));
            }
        }
        self.selected(root)
    }
}

struct Selection<'a> {
    parent: File,
    root: File,
    name: String,
    ids: BTreeSet<String>,
    journals: Vec<Journal<'a>>,
}
impl Selection<'_> {
    fn contents(&mut self) -> Result<()> {
        private(&self.root)?;
        let mut names = BTreeSet::new();
        for entry in rustix::fs::Dir::read_from(&self.root).map_err(std::io::Error::from)? {
            let entry = entry.map_err(std::io::Error::from)?;
            let name = entry.file_name().to_str().map_err(|_| Error::Path)?;
            if matches!(name, "." | "..") {
                continue;
            }
            if names.len() >= crate::MAX_PROJECTS || !names.insert(name.to_owned()) {
                return Err(Error::Limit);
            }
        }
        if names != self.ids {
            return Err(Error::Metadata);
        }
        for journal in &mut self.journals {
            journal.check(&self.root)?;
        }
        Ok(())
    }
    fn check(&mut self) -> Result<()> {
        private(&self.parent)?;
        self.contents()?;
        let owned = self.root.metadata()?;
        let visible = rustix::fs::statat(
            &self.parent,
            &self.name,
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(std::io::Error::from)?;
        if visible.st_mode & 0o170000 != 0o040000
            || (visible.st_dev, visible.st_ino) != (owned.dev(), owned.ino())
        {
            return Err(Error::Path);
        }
        Ok(())
    }
}

fn private(parent: &File) -> Result<()> {
    let metadata = parent.metadata()?;
    if !metadata.is_dir() || metadata.mode() & 0o7777 != 0o700 {
        return Err(Error::Path);
    }
    Ok(())
}

/// Verify and prepare one native registry child, then run trusted work while its
/// original registry/project/data owners and all public WAL writers remain held.
/// Work sees copied report metadata after durable no-replace child selection.
/// Keep the enclosing parent private until every component has been prepared.
/// Final original-history checks withhold work's value on source failure. A work
/// Result is a nested value; callers flatten both layers. No mutable owner or
/// service/user authority is exported. Postselection failure preserves data and
/// returns RegistryRestoreUnknown. Nonempty failed stages are never adopted.
pub fn with_restored_registry_backup_bytes_at<T>(
    bytes: &[u8],
    parent: &File,
    name: &str,
    work: impl FnOnce(&RegistryBackupReport) -> T,
) -> Result<T> {
    let archive = registry_archive::decode(bytes)?;
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.len() > 255
        || name.contains('/')
        || name.as_bytes().contains(&0)
    {
        return Err(Error::Path);
    }
    private(parent)?;
    let mut pending = Pending::directory_under(parent, name)?;
    // Never recursively sweep a prepared component after a failed nested proof.
    pending.retain();
    prepare_decoded(&archive, &pending)?;
    let mut restored = ProjectStore::open_existing_relative(&pending.path())?;
    let selected = Cell::new(false);
    let mut journals = Vec::new();
    journals
        .try_reserve_exact(archive.entries.len())
        .map_err(|_| Error::Limit)?;
    for entry in &archive.entries {
        journals.push(Journal::new(
            &pending.owner,
            &entry.metadata.id,
            entry
                .database
                .get(emilybase_backup::HEADER_SIZE..)
                .ok_or(Error::RegistryFormat("prepared WAL bounds"))?,
        )?);
    }
    let selection = RefCell::new(Selection {
        parent: parent.try_clone()?,
        root: pending.owner.try_clone()?,
        name: name.to_owned(),
        ids: archive
            .report
            .projects
            .iter()
            .map(|p| p.id.clone())
            .collect(),
        journals,
    });
    let outcome = restored.capture_registry_checked_with(
        |image| {
            if image != bytes {
                return Err(Error::RegistryFormat(
                    "prepared registry differs from verified archive",
                ));
            }
            private(parent)?;
            selection.borrow_mut().contents()?;
            checkpoint("registry_restore_coordinated_owned");
            sync(&pending.owner, "registry_restore_coordinated_stage_sync")?;
            private(parent)?;
            selection.borrow_mut().contents()?;
            pending.publish()?;
            selected.set(true);
            checkpoint("registry_restore_coordinated_selected");
            pending.finish("registry_restore_coordinated_parent_sync")?;
            private(parent)?;
            selection.borrow_mut().check()?;
            checkpoint("registry_restore_coordinated_ready");
            let value = work(&archive.report);
            private(parent)?;
            pending.finish("registry_restore_coordinated_confirm_sync")?;
            private(parent)?;
            Ok(value)
        },
        || {
            // This final selection observation still runs under every original public
            // data writer, after all final history hashes, before any owner is dropped.
            if selected.get() {
                checkpoint("registry_restore_coordinated_verifying");
                selection.borrow_mut().check()?;
            }
            Ok(())
        },
    );
    if selected.get() {
        outcome.map_err(|error| Error::RegistryRestoreUnknown(Box::new(error)))
    } else {
        outcome
    }
}
