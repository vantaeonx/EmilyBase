use emilybase_catalog::{Key, Value};
use emilybase_query::{execute, stage};
use emilybase_transactions::Database;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn killed_composed_writer_recovers_both_schema_and_receipts_only_after_ack() {
    for version in [1, 2] {
        for phase in ["prefix", "sql", "receipt", "error", "ack"] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("db");
            let mut database = Database::create(&path).unwrap();
            execute(
                &mut database,
                "CREATE TABLE t(id INT PRIMARY KEY,value INT); INSERT INTO t VALUES(0,0); \
                 CREATE TABLE receipts(id INT PRIMARY KEY)",
                &[],
            )
            .unwrap();
            if version == 2 {
                database.compact().unwrap();
            }
            let base = database.last_transaction();
            let before = database.committed_wal().unwrap();
            drop(database);
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "staging_writer_helper",
                    "--nocapture",
                ])
                .env("EMILYBASE_STAGING_PATH", &path)
                .env("EMILYBASE_STAGING_PHASE", phase)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let stdout = child.stdout.take().unwrap();
            let (sender, receiver) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    if let Ok(line) = line {
                        if let Some(marker) = line.strip_prefix("STAGED_READY ") {
                            let _ = sender.send(marker.to_owned());
                            break;
                        }
                    } else {
                        break;
                    }
                }
            });
            let marker = receiver.recv_timeout(Duration::from_secs(20));
            // Always reap the child before asserting the bounded handshake.
            let killed = child.kill();
            let status = child.wait().unwrap();
            reader.join().unwrap();
            assert!(killed.is_ok(), "{version}/{phase}");
            assert!(!status.success(), "{version}/{phase}");
            assert_eq!(marker.unwrap(), phase);
            let mut database = Database::open(&path).unwrap();
            let committed = phase == "ack";
            assert_eq!(database.last_transaction(), base + u64::from(committed));
            let snapshot = database.view().unwrap();
            assert_eq!(
                snapshot.get("t", &Key::Integer(0)).unwrap().unwrap()[1],
                Value::Integer(if committed { 9 } else { 0 })
            );
            assert_eq!(
                snapshot.get("t", &Key::Integer(1)).unwrap().is_some(),
                committed
            );
            assert_eq!(snapshot.schema("next").is_ok(), committed);
            assert_eq!(
                snapshot.scan("receipts", 10).unwrap().len(),
                if committed { 2 } else { 0 }
            );
            if !committed {
                assert_eq!(database.committed_wal().unwrap(), before);
            } else {
                assert!(snapshot.get("next", &Key::Integer(7)).unwrap().is_some());
            }
            // Recovered ownership is usable for a new independent transaction.
            let (mut transaction, _) = stage(
                database.begin().unwrap(),
                "UPDATE t SET value=10 WHERE id=0",
                &[],
            )
            .unwrap()
            .into_parts();
            transaction
                .insert("receipts", vec![Value::Integer(3)])
                .unwrap();
            assert_eq!(
                transaction.commit().unwrap(),
                base + u64::from(committed) + 1
            );
            drop(database);
            let database = Database::open(&path).unwrap();
            assert_eq!(
                database
                    .view()
                    .unwrap()
                    .get("t", &Key::Integer(0))
                    .unwrap()
                    .unwrap()[1],
                Value::Integer(10)
            );
        }
    }
}

#[test]
#[ignore = "subprocess helper, invoked by its parent kill test"]
fn staging_writer_helper() {
    let path = std::env::var_os("EMILYBASE_STAGING_PATH").unwrap();
    let phase = std::env::var("EMILYBASE_STAGING_PHASE").unwrap();
    let mut database = Database::open(path).unwrap();
    let mut transaction = database.begin().unwrap();
    transaction
        .insert("receipts", vec![Value::Integer(1)])
        .unwrap();
    if phase == "prefix" {
        ready(&phase);
    }
    let staged = stage(
        transaction,
        "UPDATE t SET value=9 WHERE id=0; INSERT INTO t VALUES(1,9); \
         CREATE TABLE next(id INT PRIMARY KEY); INSERT INTO next VALUES(7); SELECT * FROM next",
        &[],
    )
    .unwrap();
    assert_eq!(staged.results()[4].rows, vec![vec![Value::Integer(7)]]);
    if phase == "sql" {
        ready(&phase);
    }
    let (mut transaction, _) = staged.into_parts();
    transaction
        .insert("receipts", vec![Value::Integer(2)])
        .unwrap();
    if phase == "receipt" {
        ready(&phase);
    }
    if phase == "error" {
        assert!(stage(transaction, "SELECT missing FROM next", &[]).is_err());
        ready(&phase);
    } else {
        assert_eq!(phase, "ack");
        transaction.commit().unwrap();
        ready(&phase);
    }
}

fn ready(phase: &str) -> ! {
    println!("STAGED_READY {phase}");
    std::io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}
