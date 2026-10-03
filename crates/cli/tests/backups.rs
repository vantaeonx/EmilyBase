use std::path::Path;
use std::process::{Command, Output};

use serde_json::json;

fn run(command: &str, paths: &[&Path], args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg(command)
        .args(paths)
        .args(args)
        .output()
        .unwrap()
}

fn ok(command: &str, paths: &[&Path], args: &[&str]) -> String {
    let output = run(command, paths, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn seed(path: &Path) {
    ok("db-init", &[path], &["--durable"]);
    let schema = json!({"name":"items","columns":[
        {"name":"id","data_type":"integer","nullable":false},
        {"name":"text","data_type":"text","nullable":false}
    ],"primary_key":0})
    .to_string();
    ok("table-create", &[path], &[&schema]);
    ok(
        "row-insert",
        &[path],
        &[
            "items",
            &json!([
                {"type":"integer","value":7},
                {"type":"text","value":"synthetic-private-value"}
            ])
            .to_string(),
        ],
    );
}

#[test]
fn actual_cli_verifies_restores_and_continues_writing_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("исходная база");
    let backup = dir.path().join("копия с пробелами.backup");
    let target = dir.path().join("восстановленная база");
    seed(&source);
    let created = ok("backup", &[&source, &backup], &[]);
    assert!(created.contains("tables=1 rows=1 transaction=3"));
    assert!(!created.contains("synthetic-private-value"));
    assert_eq!(ok("backup-verify", &[&backup], &[]), created);
    assert_eq!(ok("restore", &[&backup, &target], &[]), created);
    assert_eq!(
        ok("row-scan", &[&source], &["items"]),
        ok("row-scan", &[&target], &["items"])
    );
    ok(
        "row-insert",
        &[&target],
        &[
            "items",
            &json!([
                {"type":"integer","value":8},{"type":"text","value":"new restored commit"}
            ])
            .to_string(),
        ],
    );
    let rows: serde_json::Value =
        serde_json::from_str(&ok("row-scan", &[&target], &["items"])).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert_eq!(ok("backup-verify", &[&backup], &[]), created);
}

#[test]
fn cli_refuses_overwrite_and_bad_archives_without_partial_destinations() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let backup = dir.path().join("snapshot.backup");
    seed(&source);
    ok("backup", &[&source, &backup], &[]);
    let before = std::fs::read(&backup).unwrap();
    assert!(!run("backup", &[&source, &backup], &[]).status.success());
    assert_eq!(std::fs::read(&backup).unwrap(), before);
    let existing = dir.path().join("existing");
    std::fs::create_dir(&existing).unwrap();
    assert!(!run("restore", &[&backup, &existing], &[]).status.success());
    assert_eq!(std::fs::read_dir(existing).unwrap().count(), 0);
    let mut damaged = before;
    damaged[emilybase_backup::HEADER_SIZE + 100] ^= 1;
    std::fs::write(&backup, damaged).unwrap();
    let target = dir.path().join("must-not-exist");
    let output = run("restore", &[&backup, &target], &[]);
    assert!(!output.status.success());
    assert!(!target.exists());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-private-value"));
    assert!(!run("backup-verify", &[&backup], &[]).status.success());
}

#[test]
fn backup_cli_requires_managed_ownership_and_rejects_legacy_files() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    seed(&source);
    let db = emilybase_transactions::Database::open(&source).unwrap();
    let backup = dir.path().join("blocked.backup");
    assert!(!run("backup", &[&source, &backup], &[]).status.success());
    assert!(!backup.exists());
    drop(db);
    let legacy = dir.path().join("legacy.emily");
    ok("db-init", &[&legacy], &[]);
    assert!(!run("backup", &[&legacy, &backup], &[]).status.success());
    assert!(!backup.exists());
}
