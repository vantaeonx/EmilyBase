use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Event, EventKind};
use emilybase_transactions::Database;
use emilybase_wal::Wal;

fn schema() -> Schema {
    Schema {
        name: "items".into(),
        columns: vec![Column {
            name: "id".into(),
            data_type: DataType::Integer,
            nullable: false,
        }],
        primary_key: 0,
    }
}

fn barrier() {
    println!("READY");
    std::io::stdout().flush().unwrap();
    let _ = std::io::stdin().read(&mut [0u8; 1]);
}

#[test]
#[ignore = "subprocess helper, invoked by its parent test"]
fn table_crash_worker() {
    let Ok(path) = std::env::var("EMILYBASE_TABLE_CRASH_PATH") else {
        return;
    };
    let phase = std::env::var("EMILYBASE_TABLE_CRASH_PHASE").unwrap();
    let mut db = Database::open(&path).unwrap();
    if phase == "stream" {
        for id in 1..=1000 {
            let mut tx = db.begin().unwrap();
            tx.insert("items", vec![Value::Integer(id)]).unwrap();
            tx.commit().unwrap();
            println!("ACK:{id}");
            std::io::stdout().flush().unwrap();
        }
        return;
    }
    if phase == "wal_pages" {
        let mut staged = db.view().unwrap().clone();
        for id in [1, 2] {
            staged
                .apply(Event {
                    table_id: 1,
                    kind: EventKind::Insert(vec![Value::Integer(id)]),
                })
                .unwrap();
        }
        let id = db.database_id();
        drop(db);
        let (mut wal, _) =
            Wal::open(std::path::Path::new(&path).join("redo.wal"), Some(id)).unwrap();
        let mut pending = wal
            .begin(&staged.pages().cloned().collect::<Vec<_>>())
            .unwrap();
        pending.sync_uncommitted().unwrap();
        barrier();
    } else {
        let mut tx = db.begin().unwrap();
        for id in [1, 2] {
            tx.insert("items", vec![Value::Integer(id)]).unwrap();
        }
        if phase == "committed" {
            tx.commit().unwrap();
            barrier();
        } else {
            barrier();
            tx.rollback();
        }
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
fn killed_table_writer_preserves_complete_batches_and_discards_pending_work() {
    for phase in ["staged", "wal_pages", "committed"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut db = Database::create(&path).unwrap();
        let mut tx = db.begin().unwrap();
        tx.create_table(schema()).unwrap();
        tx.insert("items", vec![Value::Integer(0)]).unwrap();
        tx.commit().unwrap();
        db.checkpoint().unwrap();
        drop(db);
        let mut worker = Worker(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "table_crash_worker", "--nocapture", "--ignored"])
                .env("EMILYBASE_TABLE_CRASH_PATH", &path)
                .env("EMILYBASE_TABLE_CRASH_PHASE", phase)
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
        let mut db = Database::open(&path).unwrap();
        let expected = if phase == "committed" { 3 } else { 1 };
        assert_eq!(db.view().unwrap().row_count(), expected, "phase {phase}");
        assert!(
            db.view()
                .unwrap()
                .get("items", &Key::Integer(0))
                .unwrap()
                .is_some()
        );
        for id in [1, 2] {
            assert_eq!(
                db.view()
                    .unwrap()
                    .get("items", &Key::Integer(id))
                    .unwrap()
                    .is_some(),
                phase == "committed"
            );
        }
        // A new commit removes an ignored tail and must itself survive reopen.
        let mut tx = db.begin().unwrap();
        tx.insert("items", vec![Value::Integer(3)]).unwrap();
        tx.commit().unwrap();
        drop(db);
        assert_eq!(
            Database::open(path).unwrap().view().unwrap().row_count(),
            expected + 1
        );
    }
}

#[test]
fn killing_a_continuously_writing_process_preserves_every_observed_acknowledgment() {
    for kill_after in [1, 5, 20, 50] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut db = Database::create(&path).unwrap();
        let mut tx = db.begin().unwrap();
        tx.create_table(schema()).unwrap();
        tx.insert("items", vec![Value::Integer(0)]).unwrap();
        tx.commit().unwrap();
        drop(db);
        let mut worker = Worker(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "table_crash_worker", "--nocapture", "--ignored"])
                .env("EMILYBASE_TABLE_CRASH_PATH", &path)
                .env("EMILYBASE_TABLE_CRASH_PHASE", "stream")
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stdout = worker.0.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut acknowledged = Vec::new();
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(id) = line
                    .strip_prefix("ACK:")
                    .and_then(|s| s.parse::<i64>().ok())
                {
                    acknowledged.push(id);
                    if id == kill_after {
                        let _ = sender.send(());
                    }
                }
            }
            acknowledged
        });
        receiver.recv_timeout(Duration::from_secs(10)).unwrap();
        worker.0.kill().unwrap();
        assert!(!worker.0.wait().unwrap().success());
        let acknowledged = reader.join().unwrap();
        let db = Database::open(path).unwrap();
        let snapshot = db.view().unwrap();
        assert!(acknowledged.len() >= kill_after as usize);
        for id in acknowledged {
            assert_eq!(
                snapshot.get("items", &Key::Integer(id)).unwrap(),
                Some(&vec![Value::Integer(id)])
            );
        }
        // A commit can be durable before its stdout acknowledgment is observed.
        // Such complete commits are valid; an uncommitted prefix is never replayed.
        let last_id = db.last_transaction() as i64 - 2;
        assert_eq!(snapshot.row_count(), last_id as usize + 1);
        assert_eq!(
            snapshot.scan("items", 10000).unwrap(),
            (0..=last_id)
                .map(|id| vec![Value::Integer(id)])
                .collect::<Vec<_>>()
        );
    }
}
