use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_wal::{SNAPSHOT_WAL_VERSION, recover};

use super::*;

fn row(id: i64, text: &str) -> Vec<Value> {
    vec![Value::Integer(id), Value::Text(text.into())]
}

fn seeded(path: &Path, compacted: bool) -> Database {
    let mut db = Database::create(path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(Schema {
        name: "items".into(),
        columns: vec![
            Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
            Column {
                name: "text".into(),
                data_type: DataType::Text,
                nullable: false,
            },
        ],
        primary_key: 0,
    })
    .unwrap();
    tx.insert("items", row(0, "initial")).unwrap();
    tx.insert("items", row(1, &"a".repeat(3000))).unwrap();
    tx.insert("items", row(2, &"b".repeat(3000))).unwrap();
    tx.commit().unwrap();
    if compacted {
        db.compact().unwrap();
    }
    for revision in 0..8 {
        let mut tx = db.begin().unwrap();
        tx.update(
            "items",
            &Key::Integer(0),
            row(0, &format!("revision {revision}")),
        )
        .unwrap();
        tx.commit().unwrap();
    }
    db
}

fn barrier() {
    println!("READY");
    std::io::stdout().flush().unwrap();
    let _ = std::io::stdin().read(&mut [0; 1]);
}

#[test]
#[ignore = "subprocess helper, invoked by its parent test"]
fn compaction_worker() {
    let path = PathBuf::from(std::env::var_os("EMILYBASE_COMPACTION_PATH").unwrap());
    let phase = std::env::var("EMILYBASE_COMPACTION_PHASE").unwrap();
    let mut db = Database::open(path).unwrap();
    db.compact_with(
        || {
            if phase == "staged" {
                barrier();
            }
        },
        || {
            if phase == "renamed" {
                barrier();
            }
        },
        |directory| {
            directory.sync_all()?;
            if phase == "durable" {
                barrier();
            }
            Ok(())
        },
    )
    .unwrap();
    if phase == "returned" {
        barrier();
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
fn process_kill_at_every_compaction_publication_boundary_preserves_acknowledged_state() {
    let _guard = crate::PROCESS_TESTS.lock().unwrap();
    for compacted in [false, true] {
        for phase in ["staged", "renamed", "durable", "returned"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("db");
            let db = seeded(&path, compacted);
            let id = db.database_id();
            let transaction = db.last_transaction();
            let expected_pages = db.view().unwrap().pages().cloned().collect::<Vec<_>>();
            let expected_rows = db.view().unwrap().scan("items", 100).unwrap();
            let original = fs::read(path.join("redo.wal")).unwrap();
            drop(db);
            let mut worker = Worker(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "compaction::tests::compaction_worker",
                        "--nocapture",
                        "--ignored",
                    ])
                    .env("EMILYBASE_COMPACTION_PATH", &path)
                    .env("EMILYBASE_COMPACTION_PHASE", phase)
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
            let selected = fs::read(path.join("redo.wal")).unwrap();
            if phase == "staged" {
                assert_eq!(selected, original);
            } else {
                assert_eq!(
                    recover(&selected, Some(id)).unwrap().format_version,
                    SNAPSHOT_WAL_VERSION
                );
            }
            let mut db = Database::open_bound(&path, Some(id)).unwrap();
            assert_eq!(db.last_transaction(), transaction);
            assert_eq!(
                db.view().unwrap().scan("items", 100).unwrap(),
                expected_rows
            );
            assert_eq!(
                db.view().unwrap().pages().cloned().collect::<Vec<_>>(),
                expected_pages
            );
            // Leftover staging is ignored on open and replaced only by a later explicit compaction.
            if phase == "staged" {
                assert!(path.join("redo-next.wal").is_file());
            }
            db.compact().unwrap();
            assert!(!path.join("redo-next.wal").exists());
            let mut tx = db.begin().unwrap();
            tx.insert("items", row(3, "after interrupted compaction"))
                .unwrap();
            assert_eq!(tx.commit().unwrap(), transaction + 1);
            drop(db);
            let db = Database::open(path).unwrap();
            assert_eq!(db.view().unwrap().row_count(), 4);
            assert_eq!(db.last_transaction(), transaction + 1);
        }
    }
}

#[test]
fn failed_parent_sync_after_rename_preserves_complete_baseline_and_poisons_old_owner() {
    let _guard = crate::PROCESS_TESTS.lock().unwrap();
    for compacted in [false, true] {
        for sync_before_error in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("db");
            let mut db = seeded(&path, compacted);
            let id = db.database_id();
            let transaction = db.last_transaction();
            let expected = db.view().unwrap().scan("items", 100).unwrap();
            let result = db.compact_with(
                || {},
                || {},
                |directory| {
                    if sync_before_error {
                        directory.sync_all()?;
                    }
                    Err(std::io::Error::other("synthetic directory sync failure"))
                },
            );
            assert!(matches!(result, Err(Error::MaintenanceUnknown(_))));
            assert!(matches!(db.view(), Err(Error::Poisoned)));
            assert!(matches!(db.begin(), Err(Error::Poisoned)));
            assert!(matches!(db.compact(), Err(Error::Poisoned)));
            assert!(matches!(db.committed_wal(), Err(Error::Poisoned)));
            assert!(matches!(
                Database::open(&path),
                Err(Error::Wal(emilybase_wal::Error::Busy))
            ));
            drop(db);
            let mut db = Database::open_bound(&path, Some(id)).unwrap();
            assert_eq!(db.last_transaction(), transaction);
            assert_eq!(db.view().unwrap().scan("items", 100).unwrap(), expected);
            let mut tx = db.begin().unwrap();
            tx.insert("items", row(3, "after outcome inspection"))
                .unwrap();
            assert_eq!(tx.commit().unwrap(), transaction + 1);
        }
    }
}

#[test]
fn directory_owner_remains_exclusive_before_and_after_replacement() {
    let _guard = crate::PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = seeded(&path, false);
    let excluded = || {
        assert!(matches!(
            Database::open(&path),
            Err(Error::Wal(emilybase_wal::Error::Busy))
        ));
    };
    db.compact_with(excluded, excluded, std::fs::File::sync_all)
        .unwrap();
    excluded();
    drop(db);
    assert!(Database::open(path).is_ok());
}
