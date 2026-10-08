use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_transactions::Database;
use serde_json::json;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
static CASES: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn export(path: &Path, table: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg("table-export")
        .arg(path)
        .arg(table)
        .output()
        .unwrap()
}
fn import(path: &Path, bytes: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg("table-import")
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(bytes).unwrap();
    drop(stdin);
    child.wait_with_output().unwrap()
}
fn source(path: &Path) {
    let mut db = Database::create(path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(Schema {
        name: "t".into(),
        primary_key: 0,
        columns: vec![
            Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
            Column {
                name: "value".into(),
                data_type: DataType::Text,
                nullable: false,
            },
        ],
    })
    .unwrap();
    for id in [2, 1] {
        tx.insert(
            "t",
            vec![
                Value::Integer(id),
                Value::Text("synthetic-Привет-界\0'; DROP TABLE t; --".into()),
            ],
        )
        .unwrap();
    }
    tx.commit().unwrap();
}
#[test]
fn actual_cli_roundtrip_is_one_durable_commit_and_existing_table_refuses_on_both_wals() {
    let _guard = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    for compact in [false, true] {
        let path = dir.path().join(format!("источник-{compact}"));
        source(&path);
        if compact {
            Database::open(&path).unwrap().compact().unwrap();
        }
        let before = fs::read(path.join("redo.wal")).unwrap();
        let result = export(&path, "t");
        assert!(result.status.success());
        assert!(result.stderr.is_empty());
        assert_eq!(fs::read(path.join("redo.wal")).unwrap(), before);
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(value["format"], "emilybase-table");
        assert_eq!(value["version"], 1);
        assert_eq!(value["rows"][0][0]["value"], 1);
        let target = dir.path().join(format!("копия-{compact}"));
        let mut db = Database::create(&target).unwrap();
        if compact {
            db.compact().unwrap();
        }
        drop(db);
        let output = import(&target, &result.stdout);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["transaction"], 2);
        assert_eq!(report["transfer"]["rows"], 2);
        assert_eq!(report["transfer"]["columns"], 2);
        let printed = String::from_utf8(output.stdout).unwrap();
        assert!(!printed.contains("synthetic-"));
        assert!(!printed.contains(target.to_str().unwrap()));
        assert_eq!(export(&target, "t").stdout, result.stdout);
        let before = fs::read(target.join("redo.wal")).unwrap();
        let repeated = import(&target, &result.stdout);
        assert!(!repeated.status.success());
        assert!(repeated.stdout.is_empty());
        assert_eq!(fs::read(target.join("redo.wal")).unwrap(), before);
        assert_eq!(
            Database::open(&target).unwrap().view().unwrap().row_count(),
            2
        );
    }
}
#[test]
fn malformed_stdin_never_opens_destination_and_errors_do_not_echo_contents() {
    let _guard = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("absent");
    let root = dir.path().join("source");
    source(&root);
    let original = export(&root, "t").stdout;
    let mut duplicate: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let row = duplicate["rows"][0].clone();
    duplicate["rows"].as_array_mut().unwrap().push(row);
    let mut unknown: serde_json::Value = serde_json::from_slice(&original).unwrap();
    unknown["synthetic-private-extra"] = json!("synthetic-private-content");
    // The selected invalid destination marker distinguishes pre-open document
    // validation from any attempted existing-database recovery.
    fs::create_dir(&missing).unwrap();
    let marker = missing.join("redo.wal");
    fs::write(&marker, b"synthetic-protected-marker").unwrap();
    for input in [
        b"synthetic-private-content".to_vec(),
        serde_json::to_vec(&duplicate).unwrap(),
        serde_json::to_vec(&unknown).unwrap(),
        original[..original.len() / 2].to_vec(),
    ] {
        let out = import(&missing, &input);
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        let error = String::from_utf8(out.stderr).unwrap();
        let expected = emilybase_transfer::decode_table(&input).unwrap_err();
        assert_eq!(error.trim(), format!("error: {expected}"));
        assert!(!error.contains("synthetic-private"));
        assert!(!error.contains(missing.to_str().unwrap()));
        assert_eq!(fs::read(&marker).unwrap(), b"synthetic-protected-marker");
    }
    let locked = Database::open(&root).unwrap();
    let before = fs::read(root.join("redo.wal")).unwrap();
    assert!(!import(&root, b"{}").status.success());
    assert_eq!(fs::read(root.join("redo.wal")).unwrap(), before);
    drop(locked);
}
#[test]
fn export_is_complete_or_empty_on_refusal_and_legacy_files_remain_exact() {
    let _guard = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source");
    source(&path);
    let before = fs::read(path.join("redo.wal")).unwrap();
    let result = export(&path, "unknown");
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), before);
    let legacy = dir.path().join("legacy.emily");
    emilybase_database::Database::create(&legacy).unwrap();
    let before = fs::read(&legacy).unwrap();
    assert!(!export(&legacy, "t").status.success());
    assert!(!import(&legacy, &export(&path, "t").stdout).status.success());
    assert_eq!(fs::read(legacy).unwrap(), before);
}
