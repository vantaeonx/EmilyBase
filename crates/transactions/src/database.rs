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
        let temporary = self.path.join("checkpoint-next.emily");
        match fs::remove_file(&temporary) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        let pages = self.snapshot.pages().cloned().collect::<Vec<_>>();
        let pager = Pager::create_with_pages(&temporary, &pages)?;
        synced();
        fs::rename(&temporary, self.path.join("checkpoint.emily"))?;
        published();
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
