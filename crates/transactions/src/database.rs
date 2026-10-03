use std::fs::{self, DirBuilder, File};
use std::path::{Path, PathBuf};

use emilybase_database::Snapshot;
use emilybase_storage::Pager;
use emilybase_wal::{DatabaseId, Wal};

use crate::{Error, Result, Transaction, replay::replay};

pub struct Database {
    pub(crate) wal: Wal,
    pub(crate) snapshot: Snapshot,
    pub(crate) poisoned: bool,
    path: PathBuf,
}

impl Database {
    /// A interrupted initialization remains detectable, never silently retried.
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut id = [0; 16];
        getrandom::fill(&mut id).map_err(|_| Error::Randomness)?;
        let mut builder = DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path)?;
        let mut wal = Wal::create(path.join("redo.wal"), id)?;
        let snapshot = Snapshot::empty()?;
        let pages = snapshot.pages().cloned().collect::<Vec<_>>();
        wal.append(&pages)?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        File::open(parent)?.sync_all()?;
        Ok(Self {
            wal,
            snapshot,
            poisoned: false,
            path: path.to_path_buf(),
        })
    }

    /// The checkpoint is disposable; recover exclusively from validated commits.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_bound(path, None)
    }

    pub fn open_bound(path: impl AsRef<Path>, expected_id: Option<DatabaseId>) -> Result<Self> {
        let path = path.as_ref();
        let (wal, recovery) = Wal::open(path.join("redo.wal"), expected_id)?;
        let snapshot = replay(recovery)?;
        Ok(Self {
            wal,
            snapshot,
            poisoned: false,
            path: path.to_path_buf(),
        })
    }

    pub fn view(&self) -> Result<&Snapshot> {
        self.ready()?;
        Ok(&self.snapshot)
    }

    pub fn begin(&mut self) -> Result<Transaction<'_>> {
        self.ready()?;
        let staged = self.snapshot.clone();
        Ok(Transaction {
            database: self,
            staged,
            aborted: false,
            events: 0,
        })
    }

    pub fn database_id(&self) -> DatabaseId {
        self.wal.database_id()
    }

    pub fn last_transaction(&self) -> u64 {
        self.wal.last_transaction()
    }

    /// Materialize a synced, atomically replaced cache. WAL is deliberately retained.
    pub fn checkpoint(&mut self) -> Result<()> {
        self.ready()?;
        let temporary = self.path.join("checkpoint-next.emily");
        match fs::remove_file(&temporary) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        let pages = self.snapshot.pages().cloned().collect::<Vec<_>>();
        let pager = Pager::create_with_pages(&temporary, &pages)?;
        fs::rename(&temporary, self.path.join("checkpoint.emily"))?;
        File::open(&self.path)?.sync_all()?;
        drop(pager);
        Ok(())
    }

    pub(crate) fn ready(&self) -> Result<()> {
        if self.poisoned {
            Err(Error::Poisoned)
        } else {
            Ok(())
        }
    }
}
