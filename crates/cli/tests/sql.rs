use std::process::{Command, Output};

fn run(path: &std::path::Path, sql: &str, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg("sql")
        .arg(path)
        .arg(sql)
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn actual_cli_executes_binds_explains_rolls_back_and_refuses_legacy_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic db");
    emilybase_transactions::Database::create(&path).unwrap();
    let result = run(
        &path,
        "CREATE TABLE t(id INT PRIMARY KEY,title TEXT); INSERT INTO t VALUES(1,$1); SELECT * FROM t",
        &[
            "--parameters",
            "[{\"type\":\"text\",\"value\":\"'; DROP TABLE t; --\"}]",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["committed"], true);
    assert_eq!(
        report["results"][2]["rows"][0][1]["value"],
        "'; DROP TABLE t; --"
    );
    let result = run(&path, "SELECT * FROM t WHERE id=1", &["--explain"]);
    assert!(result.status.success());
    let plan: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(plan["access"], "primary_key");
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    let result = run(&path, "BEGIN;DELETE FROM t;ROLLBACK", &[]);
    assert!(result.status.success());
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    let result = run(
        &path,
        "INSERT INTO t VALUES(2,$1);SELECT missing FROM t",
        &[
            "--parameters",
            "[{\"type\":\"text\",\"value\":\"synthetic-secret\"}]",
        ],
    );
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("synthetic-secret"));
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    let legacy = dir.path().join("legacy.emily");
    emilybase_database::Database::create(&legacy).unwrap();
    let original = std::fs::read(&legacy).unwrap();
    assert!(
        !run(&legacy, "CREATE TABLE t(id INT PRIMARY KEY)", &[])
            .status
            .success()
    );
    assert_eq!(std::fs::read(&legacy).unwrap(), original);
}
