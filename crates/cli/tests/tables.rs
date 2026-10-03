use std::path::Path;
use std::process::{Command, Output};

use emilybase_catalog::{Row, Schema, Value};
use emilybase_database::Database;

const SCHEMA: &str = r#"{"name":"items","columns":[{"name":"id","data_type":"integer","nullable":false},{"name":"title","data_type":"text","nullable":true}],"primary_key":0}"#;
const ROW: &str = r#"[{"type":"integer","value":7},{"type":"text","value":"Привет"}]"#;
const KEY: &str = r#"{"type":"integer","value":7}"#;

fn invoke(command: &str, path: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg(command)
        .arg(path)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn typed_cli_lifecycle_uses_real_persistent_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("typed.emily");
    assert!(invoke("db-init", &path, &[]).status.success());
    assert!(invoke("table-create", &path, &[SCHEMA]).status.success());
    let listed = invoke("table-list", &path, &[]);
    let schemas: Vec<Schema> = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(schemas[0].name, "items");
    assert!(
        invoke("row-insert", &path, &["items", ROW])
            .status
            .success()
    );
    assert!(
        !invoke("row-insert", &path, &["items", ROW])
            .status
            .success()
    );
    let output = invoke("row-get", &path, &["items", KEY]);
    let row: Row = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(row, vec![Value::Integer(7), Value::Text("Привет".into())]);
    let updated = r#"[{"type":"integer","value":7},{"type":"null"}]"#;
    assert!(
        invoke("row-update", &path, &["items", KEY, updated])
            .status
            .success()
    );
    let scan = invoke("row-scan", &path, &["items", "--limit", "1"]);
    let rows: Vec<Row> = serde_json::from_slice(&scan.stdout).unwrap();
    assert_eq!(rows[0], vec![Value::Integer(7), Value::Null]);
    assert!(
        invoke("row-delete", &path, &["items", KEY])
            .status
            .success()
    );
    assert_eq!(invoke("row-get", &path, &["items", KEY]).stdout, b"null\n");
    assert!(invoke("table-drop", &path, &["items"]).status.success());
    assert_eq!(invoke("table-list", &path, &[]).stdout, b"[]\n");
}

#[test]
fn raw_commands_cannot_mutate_managed_table_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("protected.emily");
    assert!(invoke("db-init", &path, &[]).status.success());
    let before = std::fs::read(&path).unwrap();
    for (command, arguments) in [
        ("append", vec!["raw"]),
        ("replace", vec!["1", "0", "raw"]),
        ("delete", vec!["1", "0"]),
    ] {
        let output = invoke(command, &path, &arguments);
        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("raw mutation is disabled")
        );
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
    Database::open(path).unwrap();
}

#[test]
fn invalid_json_and_oversized_inputs_do_not_leak_or_write_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.emily");
    assert!(invoke("db-init", &path, &[]).status.success());
    assert!(invoke("table-create", &path, &[SCHEMA]).status.success());
    let before = std::fs::read(&path).unwrap();
    let payload = "synthetic-private-value";
    let output = invoke("row-insert", &path, &["items", payload]);
    assert!(!output.status.success());
    assert!(!String::from_utf8(output.stderr).unwrap().contains(payload));
    let oversized = "x".repeat(16385);
    assert!(
        !invoke("row-insert", &path, &["items", &oversized])
            .status
            .success()
    );
    assert!(
        !invoke("row-scan", &path, &["items", "--limit", "10001"])
            .status
            .success()
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn table_commands_respect_another_process_owner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("owned.emily");
    let owner = Database::create(&path).unwrap();
    let output = invoke("table-create", &path, &[SCHEMA]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("already locked")
    );
    drop(owner);
    assert!(invoke("table-create", &path, &[SCHEMA]).status.success());
}
