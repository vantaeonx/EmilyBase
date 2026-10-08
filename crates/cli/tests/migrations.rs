use emilybase_catalog::{Key, Value};
use emilybase_transactions::Database;
use serde_json::Value as Json;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
static CASES: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn migrate(path: &Path, version: u32, label: &str, sql: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg("migrate")
        .arg(path)
        .arg(version.to_string())
        .arg(label)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Oversized input may be refused before the writer finishes its pipe.
    let _ = child.stdin.take().unwrap().write_all(sql);
    child.wait_with_output().unwrap()
}
fn list(path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg("migrations")
        .arg(path)
        .output()
        .unwrap()
}
fn json(output: &Output) -> Json {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn actual_cli_applies_and_retries_without_echoing_sql_on_both_wal_versions() {
    let _serial = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let sql=b"CREATE TABLE t(id INT PRIMARY KEY,value TEXT); INSERT INTO t VALUES(1,'synthetic-sensitive-content')";
    for format in [1, 2] {
        let path = dir.path().join(format!("database-{format}"));
        let mut database = Database::create(&path).unwrap();
        if format == 2 {
            database.compact().unwrap();
        }
        drop(database);
        let before = std::fs::read(path.join("redo.wal")).unwrap();
        assert_eq!(json(&list(&path)), serde_json::json!([]));
        assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
        let output = migrate(&path, 1, "initial", sql);
        let applied = json(&output);
        assert_eq!(applied["already_applied"], false);
        assert_eq!(applied["receipt"]["version"], 1);
        assert_eq!(applied["receipt"]["transaction"], 2);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("synthetic-sensitive"));
        let before = std::fs::read(path.join("redo.wal")).unwrap();
        let repeated = json(&migrate(&path, 1, "initial", sql));
        assert_eq!(repeated["already_applied"], true);
        assert_eq!(repeated["receipt"], applied["receipt"]);
        assert_eq!(
            json(&list(&path)),
            serde_json::json!([applied["receipt"].clone()])
        );
        assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
        let result = json(&migrate(
            &path,
            2,
            "next",
            b"UPDATE t SET value='next' WHERE id=1",
        ));
        assert_eq!(result["receipt"]["transaction"], 3);
        let database = Database::open(&path).unwrap();
        assert_eq!(
            database
                .view()
                .unwrap()
                .get("t", &Key::Integer(1))
                .unwrap()
                .unwrap()[1],
            Value::Text("next".into())
        );
    }
}

#[test]
fn invalid_stdin_is_refused_before_destination_recovery_and_never_echoed() {
    let _serial = CASES.lock().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("protected");
    std::fs::create_dir(&path).unwrap();
    let marker = path.join("redo.wal");
    std::fs::write(&marker, b"synthetic-protected-marker").unwrap();
    for (version, label, sql) in [
        (0, "initial", b"CREATE TABLE t(id INT PRIMARY KEY)".to_vec()),
        (
            1,
            "bad/label",
            b"CREATE TABLE t(id INT PRIMARY KEY)".to_vec(),
        ),
        (1, "initial", b"synthetic-sensitive-content".to_vec()),
        (
            1,
            "initial",
            b"DROP TABLE _emilybase_migrations_v1".to_vec(),
        ),
        (1, "initial", b"SELECT * FROM t".to_vec()),
        (1, "initial", vec![b'x'; 16_385]),
        (1, "initial", vec![0xff, 0, 0xf0]),
    ] {
        let output = migrate(&path, version, label, &sql);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(!error.contains("synthetic-sensitive"));
        assert!(!error.contains("marker"));
        assert!(!error.contains(path.to_str().unwrap()));
        assert_eq!(
            std::fs::read(&marker).unwrap(),
            b"synthetic-protected-marker"
        );
    }
}

#[test]
fn changed_skipped_failed_and_busy_commands_never_publish_receipts_or_partial_data() {
    let _serial = CASES.lock().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("db");
    drop(Database::create(&path).unwrap());
    let first = b"CREATE TABLE t(id INT PRIMARY KEY); INSERT INTO t VALUES(1)";
    json(&migrate(&path, 1, "initial", first));
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    for (version, label, sql) in [
        (1, "initial", b"DROP TABLE t".as_slice()),
        (1, "renamed", first.as_slice()),
        (3, "skipped", b"DROP TABLE t".as_slice()),
        (
            2,
            "failed",
            b"CREATE TABLE next(id INT PRIMARY KEY); INSERT INTO t VALUES(1)".as_slice(),
        ),
    ] {
        let output = migrate(&path, version, label, sql);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    }
    let database = Database::open(&path).unwrap();
    assert!(database.view().unwrap().schema("next").is_err());
    let busy = migrate(&path, 2, "busy", b"DROP TABLE t");
    assert!(!busy.status.success());
    assert!(busy.stdout.is_empty());
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    drop(database);
    assert_eq!(json(&list(&path)).as_array().unwrap().len(), 1);
}
