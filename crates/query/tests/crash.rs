use emilybase_catalog::{Key, Value};
use emilybase_query::{execute, query};
use emilybase_transactions::Database;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

static PROCESS_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn killed_sql_writer_preserves_every_received_ack_as_one_atomic_script() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    for compact in [false, true] {
        for threshold in [5, 20, 60] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("db");
            let mut db = Database::create(&path).unwrap();
            execute(
                &mut db,
                "CREATE TABLE t(id INT PRIMARY KEY,value INT);INSERT INTO t VALUES(0,0)",
                &[],
            )
            .unwrap();
            if compact {
                db.compact().unwrap();
            }
            let base = db.last_transaction();
            drop(db);
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "sql_writer_helper", "--nocapture"])
                .env("EMILYBASE_SQL_CRASH_PATH", &path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let stdout = child.stdout.take().unwrap();
            let (sender, receiver) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            });
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut acknowledgments = Vec::new();
            while acknowledgments.len() < threshold {
                let line = receiver
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .unwrap()
                    .unwrap();
                if let Some(id) = line.strip_prefix("SQL_ACK ") {
                    acknowledgments.push(id.parse::<u64>().unwrap());
                }
            }
            child.kill().unwrap();
            assert!(!child.wait().unwrap().success());
            drop(receiver);
            reader.join().unwrap();
            for (index, id) in acknowledgments.iter().enumerate() {
                assert_eq!(*id, base + index as u64 + 1);
            }
            let mut db = Database::open(&path).unwrap();
            let committed = db.last_transaction() - base;
            assert!(committed >= threshold as u64);
            assert_eq!(db.view().unwrap().row_count(), committed as usize + 1);
            assert_eq!(
                db.view()
                    .unwrap()
                    .get("t", &Key::Integer(0))
                    .unwrap()
                    .unwrap()[1],
                Value::Integer(committed as i64)
            );
            for id in 1..=committed {
                assert_eq!(
                    db.view()
                        .unwrap()
                        .get("t", &Key::Integer(id as i64))
                        .unwrap()
                        .unwrap(),
                    &vec![Value::Integer(id as i64), Value::Integer(id as i64)]
                );
            }
            let report = execute(&mut db, "INSERT INTO t VALUES(-1,-1)", &[]).unwrap();
            assert_eq!(report.transaction, base + committed + 1);
            drop(db);
            let db = Database::open(&path).unwrap();
            assert_eq!(
                query(db.view().unwrap(), "SELECT value FROM t WHERE id=-1", &[])
                    .unwrap()
                    .rows,
                [vec![Value::Integer(-1)]]
            );
        }
    }
}

#[test]
fn killed_process_after_sql_rollback_cannot_publish_discarded_rows() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    execute(
        &mut db,
        "CREATE TABLE t(id INT PRIMARY KEY,value INT);INSERT INTO t VALUES(0,9)",
        &[],
    )
    .unwrap();
    let transaction = db.last_transaction();
    drop(db);
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "sql_rollback_helper", "--nocapture"])
        .env("EMILYBASE_SQL_CRASH_PATH", &path)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    loop {
        let mut line = String::new();
        assert!(stdout.read_line(&mut line).unwrap() > 0);
        if line.starts_with("SQL_ROLLBACK") {
            break;
        }
    }
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    let db = Database::open(&path).unwrap();
    assert_eq!(db.last_transaction(), transaction);
    assert_eq!(
        query(db.view().unwrap(), "SELECT * FROM t", &[])
            .unwrap()
            .rows,
        [vec![Value::Integer(0), Value::Integer(9)]]
    );
}

#[test]
#[ignore = "subprocess helper, invoked by its parent kill test"]
fn sql_writer_helper() {
    let path = std::env::var_os("EMILYBASE_SQL_CRASH_PATH").unwrap();
    let mut db = Database::open(path).unwrap();
    for id in 1..5000i64 {
        let report = execute(&mut db,"INSERT INTO t VALUES($1,$1);UPDATE t SET value=$1 WHERE id=0;SELECT id FROM t WHERE id=$1",&[Value::Integer(id)]).unwrap();
        assert_eq!(report.results[2].rows, [vec![Value::Integer(id)]]);
        println!("SQL_ACK {}", report.transaction);
        std::io::stdout().flush().unwrap();
    }
}

#[test]
#[ignore = "subprocess helper, invoked by its parent kill test"]
fn sql_rollback_helper() {
    let path = std::env::var_os("EMILYBASE_SQL_CRASH_PATH").unwrap();
    let mut db = Database::open(path).unwrap();
    let report = execute(
        &mut db,
        "BEGIN;INSERT INTO t VALUES(1,1);UPDATE t SET value=1 WHERE id=0;SELECT * FROM t;ROLLBACK",
        &[],
    )
    .unwrap();
    assert!(!report.committed);
    assert_eq!(report.results[2].rows.len(), 2);
    println!("SQL_ROLLBACK");
    std::io::stdout().flush().unwrap();
    std::thread::sleep(Duration::from_secs(30));
}
