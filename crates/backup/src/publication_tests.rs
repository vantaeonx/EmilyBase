use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_transactions::Database;

use crate::{create, files, inspect, restore};

fn barrier() {
    println!("READY");
    std::io::stdout().flush().unwrap();
    let _ = std::io::stdin().read(&mut [0u8; 1]);
}

#[test]
#[ignore = "subprocess helper, invoked by its parent test"]
fn publication_worker() {
    let input = PathBuf::from(std::env::var_os("EMILYBASE_PUBLICATION_INPUT").unwrap());
    let target = PathBuf::from(std::env::var_os("EMILYBASE_PUBLICATION_TARGET").unwrap());
    let kind = std::env::var("EMILYBASE_PUBLICATION_KIND").unwrap();
    let phase = std::env::var("EMILYBASE_PUBLICATION_PHASE").unwrap();
    let synced = || {
        if phase == "synced" {
            barrier();
        }
    };
    let published = || {
        if phase == "published" {
            barrier();
        }
    };
    if kind == "backup" {
        let mut db = Database::open(input).unwrap();
        files::create_with(&mut db, &target, synced, published).unwrap();
    } else {
        restore::restore_with(&input, &target, synced, published).unwrap();
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
fn failure_opening_parent_after_restore_publication_reports_unknown_durability() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let archive = dir.path().join("snapshot.backup");
    let parent = dir.path().join("outputs");
    let moved = dir.path().join("moved-outputs");
    std::fs::create_dir(&parent).unwrap();
    let mut db = Database::create(source).unwrap();
    let expected = create(&mut db, &archive).unwrap();
    drop(db);
    let result = restore::restore_with(
        &archive,
        &parent.join("restored"),
        || {},
        || std::fs::rename(&parent, &moved).unwrap(),
    );
    assert!(matches!(result, Err(crate::Error::PublicationUnknown(_))));
    let restored = Database::open(moved.join("restored")).unwrap();
    assert_eq!(restored.database_id(), expected.database_id);
    assert_eq!(restored.last_transaction(), expected.last_transaction);
    assert_eq!(inspect(archive).unwrap(), expected);
}

#[test]
fn forced_termination_never_publishes_a_partial_backup_or_restore() {
    for kind in ["backup", "restore"] {
        for phase in ["synced", "published"] {
            let dir = tempfile::tempdir().unwrap();
            let source = dir.path().join("source");
            let archive = dir.path().join("source.backup");
            let target = dir.path().join("output");
            let mut db = Database::create(&source).unwrap();
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
            for id in [1, 2] {
                tx.insert("items", vec![Value::Integer(id)]).unwrap();
            }
            tx.commit().unwrap();
            let expected = create(&mut db, &archive).unwrap();
            let source_before = std::fs::read(source.join("redo.wal")).unwrap();
            let archive_before = std::fs::read(&archive).unwrap();
            drop(db);
            let input = if kind == "backup" { &source } else { &archive };
            let mut worker = Worker(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "publication_tests::publication_worker",
                        "--nocapture",
                        "--ignored",
                    ])
                    .env("EMILYBASE_PUBLICATION_INPUT", input)
                    .env("EMILYBASE_PUBLICATION_TARGET", &target)
                    .env("EMILYBASE_PUBLICATION_KIND", kind)
                    .env("EMILYBASE_PUBLICATION_PHASE", phase)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
            let stdout = worker.0.stdout.take().unwrap();
            let (sender, receiver) = mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
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
            if phase == "synced" {
                assert!(!target.exists());
            } else if kind == "backup" {
                assert_eq!(inspect(&target).unwrap(), expected);
            } else {
                let mut restored = Database::open(&target).unwrap();
                assert_eq!(restored.view().unwrap().row_count(), 2);
                assert_eq!(restored.database_id(), expected.database_id);
                let mut tx = restored.begin().unwrap();
                tx.insert("items", vec![Value::Integer(3)]).unwrap();
                tx.commit().unwrap();
            }
            assert_eq!(
                std::fs::read(source.join("redo.wal")).unwrap(),
                source_before
            );
            assert_eq!(std::fs::read(&archive).unwrap(), archive_before);
            // A retry uses a new target; stale private staging paths are not adopted.
            let retry = dir.path().join("retry");
            if kind == "backup" {
                let mut db = Database::open(&source).unwrap();
                assert_eq!(create(&mut db, &retry).unwrap(), expected);
            } else {
                assert_eq!(restore::restore(&archive, &retry).unwrap(), expected);
            }
        }
    }
}
