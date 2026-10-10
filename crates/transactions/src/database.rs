#[cfg(test)]
use std::fs;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use emilybase_database::Snapshot;
use emilybase_storage::Pager;
use emilybase_wal::{DatabaseId, Wal};

use crate::{Error, Result, Transaction, replay::replay};

pub struct Database {
    pub(crate) wal: Wal,
    pub(crate) snapshot: Snapshot,
    pub(crate) poisoned: bool,
    pub(crate) path: PathBuf,
    pub(crate) ownership: File,
    pub(crate) cache_startup: crate::PrimaryCacheWarmup,
}

impl Database {
    /// An interrupted initialization remains detectable, never silently retried.
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        Self::create_with(path.as_ref(), || {}, || {})
    }

    /// Explicit native Linux parent descriptor and one fresh leaf. The original
    /// parent/child remain authoritative across moves; no old path is adopted.
    /// The returned owner's internal cache path is anchored to its own retained
    /// descriptor, not the caller's parent handle. No user authority is granted.
    pub fn create_at(parent: &File, name: impl AsRef<std::ffi::OsStr>) -> Result<Self> {
        Self::create_at_with(parent, name.as_ref(), || {}, || {})
    }

    /// Observe that one native parent/leaf still selects this exact original
    /// directory and WAL. Readonly, with no owner descriptor exported or lease.
    pub fn check_directory_at(
        &self,
        parent: &File,
        name: impl AsRef<std::ffi::OsStr>,
    ) -> Result<()> {
        self.ready()?;
        crate::ownership::verify_at(&self.ownership, parent, name.as_ref())?;
        crate::journal_replacement::source_selected(&self.ownership, &self.wal)
    }

    pub(crate) fn create_at_with(
        parent: &File,
        name: &std::ffi::OsStr,
        owned: impl FnOnce(),
        initialized: impl FnOnce(),
    ) -> Result<Self> {
        let mut id = [0; 16];
        getrandom::fill(&mut id).map_err(|_| Error::Randomness)?;
        let created = crate::ownership::Created::at(parent, name)?;
        let path = PathBuf::from(format!("/proc/self/fd/{}", created.owner.as_raw_fd())).join(".");
        Self::create_owned(created, path, id, owned, initialized)
    }

    pub(crate) fn create_with(
        path: &Path,
        owned: impl FnOnce(),
        initialized: impl FnOnce(),
    ) -> Result<Self> {
        let path = crate::ownership::absolute(path)?;
        let mut id = [0; 16];
        getrandom::fill(&mut id).map_err(|_| Error::Randomness)?;
        let created = crate::ownership::Created::new(&path)?;
        Self::create_owned(created, path, id, owned, initialized)
    }

    fn create_owned(
        created: crate::ownership::Created,
        path: PathBuf,
        id: DatabaseId,
        owned: impl FnOnce(),
        initialized: impl FnOnce(),
    ) -> Result<Self> {
        owned();
        created.verify()?;
        let file = crate::ownership::wal_file(&created.owner, true)?;
        let probe = file.try_clone()?;
        let mut wal = Wal::create_from_file(file, id)?;
        let snapshot = Snapshot::empty()?;
        let pages = snapshot.pages().cloned().collect::<Vec<_>>();
        wal.append(&pages)?;
        initialized();
        created.finish(&wal, &probe)?;
        let ownership = created.owner;
        Ok(Self {
            wal,
            snapshot,
            poisoned: false,
            path,
            ownership,
            cache_startup: crate::PrimaryCacheWarmup::default(),
        })
    }

    /// The checkpoint is disposable; recover exclusively from validated commits.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_bound(path, None)
    }

    pub fn open_bound(path: impl AsRef<Path>, expected_id: Option<DatabaseId>) -> Result<Self> {
        Self::open_with(path.as_ref(), expected_id, || {}, || {})
    }

    pub(crate) fn open_with(
        path: &Path,
        expected_id: Option<DatabaseId>,
        owned: impl FnOnce(),
        recovered: impl FnOnce(),
    ) -> Result<Self> {
        let path = crate::ownership::absolute(path)?;
        let path = path.as_path();
        let ownership = crate::ownership::lock_directory(path)?;
        owned();
        crate::ownership::verify_path(path, &ownership)?;
        let (wal, recovery) =
            Wal::open_from_file(crate::ownership::wal_file(&ownership, false)?, expected_id)?;
        let snapshot = replay(recovery)?;
        recovered();
        crate::ownership::verify_path(path, &ownership)?;
        crate::journal_replacement::source_selected(&ownership, &wal)?;
        let mut database = Self {
            wal,
            snapshot,
            poisoned: false,
            path: path.to_path_buf(),
            ownership,
            cache_startup: crate::PrimaryCacheWarmup::default(),
        };
        database.cache_startup = database.warm_primary_index_caches()?;
        Ok(database)
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

    /// Export a stable acknowledged prefix, excluding any abandoned WAL tail.
    pub fn committed_wal(&mut self) -> Result<Vec<u8>> {
        self.ready()?;
        let result = (|| {
            let bytes = self.wal.committed_bytes()?;
            let recovered = crate::recover_image(&bytes, Some(self.database_id()))?;
            if recovered.last_transaction != self.last_transaction()
                || !recovered.snapshot.pages().eq(self.snapshot.pages())
            {
                return Err(Error::History("committed snapshot changed externally"));
            }
            Ok(bytes)
        })();
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    /// Materialize a synced, atomically replaced cache. WAL is deliberately retained.
    pub fn checkpoint(&mut self) -> Result<()> {
        self.checkpoint_with(|| {}, || {})
    }

    fn checkpoint_with(&mut self, synced: impl FnOnce(), published: impl FnOnce()) -> Result<()> {
        self.ready()?;
        let temporary = "checkpoint-next.emily";
        match rustix::fs::unlinkat(&self.ownership, temporary, rustix::fs::AtFlags::empty()) {
            Ok(()) => (),
            Err(rustix::io::Errno::NOENT) => (),
            Err(error) => return Err(std::io::Error::from(error).into()),
        }
        let pages = self.snapshot.pages().cloned().collect::<Vec<_>>();
        let pager = Pager::create_with_pages_at(&self.ownership, temporary, &pages)?;
        synced();
        rustix::fs::renameat(
            &self.ownership,
            temporary,
            &self.ownership,
            "checkpoint.emily",
        )
        .map_err(std::io::Error::from)?;
        published();
        self.ownership.sync_all()?;
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

#[cfg(test)]
#[path = "initialization_tests.rs"]
pub(crate) mod initialization_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use emilybase_catalog::{Column, DataType, Schema, Value};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;

    fn barrier() {
        println!("READY");
        std::io::stdout().flush().unwrap();
        let _ = std::io::stdin().read(&mut [0u8; 1]);
    }

    #[test]
    #[ignore = "subprocess helper, invoked by its parent test"]
    fn checkpoint_worker() {
        let Ok(path) = std::env::var("EMILYBASE_CHECKPOINT_TEST_PATH") else {
            return;
        };
        let phase = std::env::var("EMILYBASE_CHECKPOINT_TEST_PHASE").unwrap();
        let mut db = Database::open(path).unwrap();
        if phase == "synced" {
            db.checkpoint_with(barrier, || {}).unwrap();
        } else {
            db.checkpoint_with(|| {}, barrier).unwrap();
        }
    }

    struct Worker(Child);
    impl Drop for Worker {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn checkpoint_process_kill_before_and_after_rename_preserves_commits() {
        let _guard = crate::PROCESS_TESTS.lock().unwrap();
        for phase in ["synced", "renamed"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("db");
            let mut db = Database::create(&path).unwrap();
            let mut tx = db.begin().unwrap();
            tx.create_table(Schema {
                name: "items".into(),
                columns: vec![Column {
                    name: "id".into(),
                    data_type: DataType::Integer,
                    nullable: false,
                }],
                primary_key: 0,
            })
            .unwrap();
            tx.insert("items", vec![Value::Integer(1)]).unwrap();
            tx.commit().unwrap();
            db.checkpoint().unwrap();
            let mut tx = db.begin().unwrap();
            tx.insert("items", vec![Value::Integer(2)]).unwrap();
            tx.commit().unwrap();
            let committed_wal = fs::read(path.join("redo.wal")).unwrap();
            drop(db);
            let mut worker = Worker(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "database::tests::checkpoint_worker",
                        "--nocapture",
                        "--ignored",
                    ])
                    .env("EMILYBASE_CHECKPOINT_TEST_PATH", &path)
                    .env("EMILYBASE_CHECKPOINT_TEST_PHASE", phase)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
            let stdout = worker.0.stdout.take().unwrap();
            let (sender, receiver) = mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout)
                    .lines()
                    .map_while(std::result::Result::ok)
                {
                    if line == "READY" {
                        let _ = sender.send(());
                        break;
                    }
                }
            });
            receiver.recv_timeout(Duration::from_secs(10)).unwrap();
            worker.0.kill().unwrap();
            assert!(!worker.0.wait().unwrap().success());
            reader.join().unwrap();
            assert_eq!(fs::read(path.join("redo.wal")).unwrap(), committed_wal);
            let mut db = Database::open(&path).unwrap();
            assert_eq!(db.view().unwrap().row_count(), 2);
            db.checkpoint().unwrap();
            let cache = emilybase_database::Database::open(path.join("checkpoint.emily")).unwrap();
            assert_eq!(
                cache.scan("items", 100).unwrap(),
                db.view().unwrap().scan("items", 100).unwrap()
            );
        }
    }
}
