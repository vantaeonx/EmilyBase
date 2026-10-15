//! Native common-root operations under both original component owners.
use crate::archive::restore::{Child, check_pair_entries, descriptor_path};
use crate::store::PublishBoundary;
use crate::write_check::WriteCheck;
use crate::{Error, FileId, FileInfo, FileQuota, FileSnapshot, FileStore, FileUsage, Result};
use emilybase_object_storage::{Inventory, ObjectId, ProjectDirectory, ProjectId};
use emilybase_transactions::Database;
use rustix::fs::{AtFlags, Mode, OFlags};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::path::{Path, PathBuf};

struct Selection {
    parent: File,
    parent_path: PathBuf,
    name: OsString,
    root: File,
    metadata: Child,
    objects: Child,
    wal: OriginalFile,
    scope: OriginalFile,
}
struct OriginalFile {
    file: File,
    baseline: Metadata,
    name: &'static str,
    maximum: usize,
}
impl OriginalFile {
    fn open(parent: &File, name: &'static str, maximum: usize) -> Result<Self> {
        let file = File::from(
            rustix::fs::openat(
                parent,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(std::io::Error::from)?,
        );
        let baseline = file.metadata()?;
        let original = Self {
            file,
            baseline,
            name,
            maximum,
        };
        original.check(parent)?;
        Ok(original)
    }
    fn selected(&self, parent: &File) -> Result<Metadata> {
        let now = self.file.metadata()?;
        let visible = rustix::fs::statat(parent, self.name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(std::io::Error::from)?;
        if !now.is_file()
            || !matches!(now.mode() & 0o777, 0o400 | 0o600)
            || now.nlink() != 1
            || now.len() > self.maximum as u64
            || visible.st_mode & 0o170000 != 0o100000
            || !matches!(visible.st_mode & 0o777, 0o400 | 0o600)
            || visible.st_nlink != 1
            || (visible.st_dev, visible.st_ino) != (self.baseline.dev(), self.baseline.ino())
            || (now.dev(), now.ino()) != (self.baseline.dev(), self.baseline.ino())
        {
            return Err(Error::Destination);
        }
        Ok(now)
    }
    fn stamp(m: &Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
        (
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )
    }
    fn check(&self, parent: &File) -> Result<()> {
        if Self::stamp(&self.selected(parent)?) != Self::stamp(&self.baseline) {
            return Err(Error::Destination);
        }
        Ok(())
    }
    fn accept_committed(&mut self, parent: &File, expected: &[u8]) -> Result<()> {
        let before = self.selected(parent)?;
        if before.len() != expected.len() as u64 {
            return Err(Error::Corrupt);
        }
        self.file.seek(SeekFrom::Start(0))?;
        let mut scratch = [0; 8192];
        for bytes in expected.chunks(scratch.len()) {
            self.file.read_exact(&mut scratch[..bytes.len()])?;
            if scratch[..bytes.len()] != *bytes {
                return Err(Error::Corrupt);
            }
        }
        let after = self.selected(parent)?;
        if Self::stamp(&before) != Self::stamp(&after) {
            return Err(Error::Destination);
        }
        // Advance only this same retained original inode after exact acknowledged
        // readback. Another inode or merely valid alternative cannot be adopted.
        self.baseline = after;
        Ok(())
    }
}
impl Selection {
    fn open(path: &Path) -> Result<Self> {
        let name = path.file_name().ok_or(Error::Destination)?.to_os_string();
        if name.as_bytes().contains(&0) {
            return Err(Error::Destination);
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent_path = if parent.is_absolute() {
            parent.to_path_buf()
        } else {
            std::env::current_dir()?.join(parent)
        };
        let parent = File::from(
            rustix::fs::open(
                &parent_path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(std::io::Error::from)?,
        );
        let root = File::from(
            rustix::fs::openat(
                &parent,
                &name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(std::io::Error::from)?,
        );
        check_pair_entries(&root)?;
        let metadata = Child::open(&root, "metadata")?;
        let objects = Child::open(&root, "objects")?;
        let wal = OriginalFile::open(
            &metadata.directory,
            "redo.wal",
            emilybase_backup::MAX_BACKUP_BYTES - emilybase_backup::HEADER_SIZE,
        )?;
        let scope = OriginalFile::open(
            &objects.directory,
            ".emilybase-objects",
            emilybase_object_storage::HEADER_BYTES,
        )?;
        let selection = Self {
            parent,
            parent_path,
            name,
            root,
            metadata,
            objects,
            wal,
            scope,
        };
        selection.check()?;
        Ok(selection)
    }
    fn check(&self) -> Result<()> {
        self.check_topology()?;
        self.wal.check(&self.metadata.directory)
    }
    fn check_topology(&self) -> Result<()> {
        let owned = self.parent.metadata()?;
        let parent = std::fs::symlink_metadata(&self.parent_path)?;
        let root = self.root.metadata()?;
        let visible = rustix::fs::statat(&self.parent, &self.name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(std::io::Error::from)?;
        if !parent.is_dir()
            || (parent.dev(), parent.ino()) != (owned.dev(), owned.ino())
            || visible.st_mode & 0o170000 != 0o040000
            || visible.st_mode & 0o777 != 0o700
            || (visible.st_dev, visible.st_ino) != (root.dev(), root.ino())
        {
            return Err(Error::Destination);
        }
        check_pair_entries(&self.root)?;
        self.metadata.check(&self.root)?;
        self.objects.check(&self.root)?;
        self.scope.check(&self.objects.directory)
    }
}

/// A copied native observation, not user authority, a quota reservation or lease.
#[derive(Clone, PartialEq, Eq)]
pub struct NativeFileRootInfo {
    project: ProjectId,
    database: [u8; 16],
    revision: u64,
    quota: FileQuota,
    usage: FileUsage,
}
impl NativeFileRootInfo {
    pub const fn project(&self) -> ProjectId {
        self.project
    }
    pub const fn database_id(&self) -> &[u8; 16] {
        &self.database
    }
    pub const fn revision(&self) -> u64 {
        self.revision
    }
    pub const fn quota(&self) -> FileQuota {
        self.quota
    }
    pub const fn usage(&self) -> FileUsage {
        self.usage
    }
}
impl std::fmt::Debug for NativeFileRootInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeFileRootInfo")
            .field("revision", &self.revision)
            .field("usage", &self.usage)
            .finish_non_exhaustive()
    }
}

#[derive(PartialEq, Eq)]
struct State {
    wal_digest: [u8; 32],
    quota: FileQuota,
    files: Vec<FileInfo>,
    inventory: Inventory,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenBoundary {
    Selected,
    Metadata,
    Owned,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadBoundary {
    Admitted,
    Copied,
}

/// Original common root/parent/children plus both independently locked owners.
/// Trusted native Linux operator only. Read/capture operations check exact root
/// selection and acknowledged semantic state around work. No raw owner, SQL,
/// user permission or writable component is exposed. Reopen after read failure.
pub struct NativeFileRoot {
    // Component cache paths borrow the retained child descriptor paths below.
    // Rust drops fields in declaration order, ending owners before selections.
    store: FileStore,
    selection: Selection,
}
impl NativeFileRoot {
    pub fn open(path: impl AsRef<Path>, project: ProjectId) -> Result<Self> {
        Self::open_with(path.as_ref(), project, |_| {})
    }
    fn open_with(
        path: &Path,
        project: ProjectId,
        mut boundary: impl FnMut(OpenBoundary),
    ) -> Result<Self> {
        let selection = Selection::open(path)?;
        boundary(OpenBoundary::Selected);
        selection.check()?;
        let database = Database::open(descriptor_path(&selection.metadata.directory))?;
        database.check_directory_at(&selection.root, "metadata")?;
        boundary(OpenBoundary::Metadata);
        selection.check()?;
        let objects =
            ProjectDirectory::open(descriptor_path(&selection.objects.directory), project)?;
        let store = FileStore::open(database, objects)?;
        let mut root = Self { store, selection };
        boundary(OpenBoundary::Owned);
        root.state()?;
        Ok(root)
    }
    fn state(&mut self) -> Result<State> {
        if self.store.poisoned {
            return Err(Error::Poisoned);
        }
        self.selection.check()?;
        self.store
            .database
            .check_directory_at(&self.selection.root, "metadata")?;
        let wal = self.store.database.committed_wal()?;
        let wal_digest = Sha256::digest(&wal).into();
        let (quota, files, inventory) = self.store.validated()?;
        self.store
            .database
            .check_directory_at(&self.selection.root, "metadata")?;
        self.selection.check()?;
        Ok(State {
            wal_digest,
            quota,
            files,
            inventory,
        })
    }
    fn observe<T>(
        &mut self,
        operation: impl FnOnce(&mut FileStore, &State) -> Result<T>,
        mut boundary: impl FnMut(ReadBoundary),
    ) -> Result<T> {
        let result = (|| {
            let before = self.state()?;
            boundary(ReadBoundary::Admitted);
            if self.state()? != before {
                return Err(Error::Corrupt);
            }
            let value = operation(&mut self.store, &before)?;
            boundary(ReadBoundary::Copied);
            if self.state()? != before {
                return Err(Error::Corrupt);
            }
            Ok(value)
        })();
        if result.is_err() {
            self.store.poisoned = true;
        }
        result
    }
    pub fn inspect(&mut self) -> Result<NativeFileRootInfo> {
        self.observe(
            |store, state| {
                Ok(NativeFileRootInfo {
                    project: store.project(),
                    database: store.database.database_id(),
                    revision: store.database.last_transaction(),
                    quota: state.quota,
                    usage: FileUsage {
                        physical_objects: state.inventory.entries().len(),
                        payload_bytes: state.inventory.payload_bytes(),
                        references: state.files.len(),
                        orphans: state.inventory.entries().len() - state.files.len(),
                    },
                })
            },
            |_| {},
        )
    }
    /// Native catalog metadata, including display names; no payload/credentials.
    pub fn list(&mut self) -> Result<Vec<FileInfo>> {
        self.observe(|_, state| Ok(state.files.clone()), |_| {})
    }
    /// Reuse original coordinated capture while retaining common-root selection.
    /// Existing capture retains object descriptors across its own copy checks.
    /// Returned sensitive immutable bytes are not publication or user authority.
    pub fn capture(&mut self) -> Result<FileSnapshot> {
        self.observe(|store, _| store.capture(), |_| {})
    }
    /// Publish through the original blob-first/metadata-WAL transaction while
    /// guarding common-root selection before blob/commit and after ACK/readback.
    /// The supplied owner is native metadata, not proof of an account permission.
    /// Blob-stage or later uncertainty preserves data and requires explicit reopen.
    pub fn publish(
        &mut self,
        id: FileId,
        object: ObjectId,
        owner: [u8; 16],
        name: &str,
        payload: &[u8],
    ) -> Result<FileInfo> {
        self.publish_with(id, object, owner, name, payload, |_| {})
    }
    fn publish_with(
        &mut self,
        id: FileId,
        object: ObjectId,
        owner: [u8; 16],
        name: &str,
        payload: &[u8],
        boundary: impl FnMut(PublishBoundary),
    ) -> Result<FileInfo> {
        crate::records::validate_name(name)?;
        if payload.len() > emilybase_object_storage::MAX_PAYLOAD_BYTES {
            return Err(emilybase_object_storage::Error::Limit.into());
        }
        let before = match self.state() {
            Ok(state) => state,
            Err(error) => {
                self.store.poisoned = true;
                return Err(error);
            }
        };
        let mut source_failed = false;
        let selection = &mut self.selection;
        let result = self.store.publish_checked_with(
            id,
            object,
            owner,
            name,
            payload,
            (boundary, |at| {
                let checked = (|| match at {
                    WriteCheck::BeforeBlob | WriteCheck::BeforeCommit | WriteCheck::Verified => {
                        selection.check()
                    }
                    WriteCheck::Committed(database) => {
                        selection.check_topology()?;
                        database.check_directory_at(&selection.root, "metadata")?;
                        let wal = database.committed_wal()?;
                        selection
                            .wal
                            .accept_committed(&selection.metadata.directory, &wal)?;
                        selection.check()
                    }
                })();
                if checked.is_err() {
                    source_failed = true;
                }
                checked
            }),
        );
        if let Err(error) = result {
            if matches!(
                error,
                Error::Objects(emilybase_object_storage::Error::PublicationUnknown)
            ) {
                self.store.poisoned = true;
                return Err(Error::OutcomeUnknown(Box::new(error)));
            }
            if source_failed || self.store.poisoned {
                self.store.poisoned = true;
                return Err(error);
            }
            if !matches!(self.state(),Ok(after) if after==before) {
                self.store.poisoned = true;
                return Err(Error::Corrupt);
            }
            return Err(error);
        }
        let info = result?;
        let checked = (|| {
            let after = self.state()?;
            if after.quota != before.quota || !after.files.contains(&info) {
                return Err(Error::Corrupt);
            }
            Ok(info)
        })();
        if checked.is_err() {
            self.store.poisoned = true;
        }
        checked.map_err(|error| Error::OutcomeUnknown(Box::new(error)))
    }
}

#[cfg(test)]
mod publication_tests;
#[cfg(test)]
mod tests;
