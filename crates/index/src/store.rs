use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::{BPlusTree, IndexSnapshot, MAX_SNAPSHOT_BYTES, SnapshotDelta};

const ACTIVE: &str = "tree.ebif";
type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Index(#[from] crate::Error),
    #[error("index filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("index directory already owned")]
    Busy,
    #[error("index path must be private, regular and not a symlink")]
    PrivatePath,
    #[error("index owner requires reopen")]
    Poisoned,
    #[error("index publication outcome requires reopen and revision inspection")]
    PublicationUnknown(#[source] std::io::Error),
    #[error("persisted index differs from the owned snapshot")]
    Changed,
}

/// Synchronous standalone snapshot publisher. It does not commit table/WAL state.
pub struct IndexStore {
    root: PathBuf,
    owner: File,
    current: IndexSnapshot,
    poisoned: bool,
}

impl IndexStore {
    /// Stage, validate, sync and publish a private directory without replacing any path.
    pub fn create(path: impl AsRef<Path>, tree: &BPlusTree) -> Result<Self> {
        let path = path.as_ref();
        if path.file_name().is_none() {
            return Err(StoreError::PrivatePath);
        }
        let current = IndexSnapshot {
            revision: 1,
            tree: tree.clone(),
        };
        let bytes = current.encode()?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent_owner = File::open(parent)?;
        let staging = tempfile::Builder::new()
            .prefix(".emilybase-index-")
            .tempdir_in(parent)?;
        std::fs::set_permissions(staging.path(), std::fs::Permissions::from_mode(0o700))?;
        let owner = directory(staging.path())?;
        lock(&owner)?;
        let pending_path = staging.path().join(ACTIVE);
        let mut pending = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&pending_path)?;
        pending.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        pending.write_all(&bytes)?;
        sync(&pending, "create_file_sync")?;
        #[cfg(test)]
        crate::store_tests::checkpoint("create_file_synced");
        if read_snapshot(&pending_path)? != current {
            return Err(StoreError::Changed);
        }
        sync(&owner, "create_directory_sync")?;
        #[cfg(test)]
        crate::store_tests::checkpoint("create_directory_synced");
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            staging.path(),
            rustix::fs::CWD,
            path,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(std::io::Error::from)?;
        #[cfg(test)]
        crate::store_tests::checkpoint("create_renamed");
        sync(&parent_owner, "create_parent_sync").map_err(StoreError::PublicationUnknown)?;
        #[cfg(test)]
        crate::store_tests::checkpoint("create_parent_synced");
        Ok(Self {
            root: path.to_path_buf(),
            owner,
            current,
            poisoned: false,
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let root = path.as_ref().to_path_buf();
        let owner = directory(&root)?;
        lock(&owner)?;
        let current = read_snapshot(&root.join(ACTIVE))?;
        Ok(Self {
            root,
            owner,
            current,
            poisoned: false,
        })
    }

    pub fn snapshot(&self) -> Result<&IndexSnapshot> {
        self.ready()?;
        Ok(&self.current)
    }

    pub fn replace(&mut self, tree: &BPlusTree) -> Result<u64> {
        self.ready()?;
        let delta = self.current.delta_to(tree)?;
        self.apply(&delta)
    }

    /// Whole-snapshot publication holds directory ownership across active-file replacement.
    pub fn apply(&mut self, delta: &SnapshotDelta) -> Result<u64> {
        self.ready()?;
        let next = delta.apply(&self.current)?;
        let actual = self.read_owned();
        match actual {
            Ok(actual) if actual == self.current => (),
            Ok(_) => {
                self.poisoned = true;
                return Err(StoreError::Changed);
            }
            Err(error) => {
                self.poisoned = true;
                return Err(error);
            }
        }
        let bytes = next.encode()?;
        let mut pending = tempfile::Builder::new()
            .prefix(".index-stage-")
            .tempfile_in(&self.root)?;
        pending
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
        pending.write_all(&bytes)?;
        sync(pending.as_file(), "replace_file_sync")?;
        #[cfg(test)]
        crate::store_tests::checkpoint("replace_file_synced");
        if read_snapshot(pending.path())? != next {
            return Err(StoreError::Changed);
        }
        pending
            .persist(self.root.join(ACTIVE))
            .map_err(|failure| failure.error)?;
        #[cfg(test)]
        crate::store_tests::checkpoint("replace_renamed");
        if let Err(error) = sync(&self.owner, "replace_directory_sync") {
            self.poisoned = true;
            return Err(StoreError::PublicationUnknown(error));
        }
        #[cfg(test)]
        crate::store_tests::checkpoint("replace_directory_synced");
        self.current = next;
        Ok(self.current.revision)
    }

    fn read_owned(&self) -> Result<IndexSnapshot> {
        let directory = directory(&self.root)?;
        let actual = directory.metadata()?;
        let expected = self.owner.metadata()?;
        if actual.dev() != expected.dev() || actual.ino() != expected.ino() {
            return Err(StoreError::Changed);
        }
        read_snapshot(&self.root.join(ACTIVE))
    }

    fn ready(&self) -> Result<()> {
        if self.poisoned {
            Err(StoreError::Poisoned)
        } else {
            Ok(())
        }
    }
}

fn directory(path: &Path) -> Result<File> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o777 != 0o700
    {
        return Err(StoreError::PrivatePath);
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::DIRECTORY).bits() as i32)
        .open(path)?;
    Ok(file)
}
fn read_snapshot(path: &Path) -> Result<IndexSnapshot> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(StoreError::PrivatePath);
    }
    if metadata.len() > MAX_SNAPSHOT_BYTES as u64 {
        return Err(crate::Error::Limit.into());
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_SNAPSHOT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(IndexSnapshot::decode(&bytes)?)
}
fn lock(file: &File) -> Result<()> {
    match file.try_lock() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(StoreError::Busy),
        Err(TryLockError::Error(error)) => Err(StoreError::Io(error)),
    }
}
fn sync(file: &File, _boundary: &str) -> std::io::Result<()> {
    #[cfg(test)]
    crate::store_tests::fail(_boundary, false)?;
    file.sync_all()?;
    #[cfg(test)]
    crate::store_tests::fail(_boundary, true)?;
    Ok(())
}
