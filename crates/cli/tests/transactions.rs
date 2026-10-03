use std::path::Path;
use std::process::{Command, Output};

use serde_json::json;

fn run(path: &Path, command: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg(command)
        .arg(path)
        .args(args)
        .output()
        .unwrap()
}

fn ok(path: &Path, command: &str, args: &[&str]) -> String {
    let output = run(path, command, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn schema() -> String {
    json!({"name":"items","columns":[
        {"name":"id","data_type":"integer","nullable":false},
        {"name":"text","data_type":"text","nullable":false}
    ],"primary_key":0})
    .to_string()
}

fn row(id: i64, text: &str) -> serde_json::Value {
    json!([{"type":"integer","value":id},{"type":"text","value":text}])
}

#[test]
fn actual_cli_uses_managed_transactions_for_table_and_row_commands() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    ok(&path, "db-init", &["--durable"]);
    assert!(path.join("redo.wal").is_file());
    ok(&path, "table-create", &[&schema()]);
    ok(
        &path,
        "row-insert",
        &["items", &row(1, "first").to_string()],
    );
    let key = r#"{"type":"integer","value":1}"#;
    ok(
        &path,
        "row-update",
        &["items", key, &row(1, "updated").to_string()],
    );
    let stored: serde_json::Value =
        serde_json::from_str(&ok(&path, "row-get", &["items", key])).unwrap();
    assert_eq!(stored, row(1, "updated"));
    assert!(ok(&path, "checkpoint", &[]).contains("journal retained"));
    let schema_list: serde_json::Value =
        serde_json::from_str(&ok(&path, "table-list", &[])).unwrap();
    assert_eq!(schema_list.as_array().unwrap().len(), 1);
    ok(&path, "row-delete", &["items", key]);
    assert_eq!(ok(&path, "row-get", &["items", key]).trim(), "null");
    ok(&path, "table-drop", &["items"]);
    assert_eq!(ok(&path, "table-list", &[]).trim(), "[]");
}

#[test]
fn cli_batch_commit_rollback_and_error_are_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    ok(&path, "db-init", &["--durable"]);
    ok(&path, "table-create", &[&schema()]);
    let operations = json!([
        {"op":"insert","table":"items","row":row(1,"one")},
        {"op":"insert","table":"items","row":row(2,"two")}
    ])
    .to_string();
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    assert_eq!(
        ok(&path, "tx", &[&operations, "--rollback"]).trim(),
        "rolled back"
    );
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    assert!(ok(&path, "tx", &[&operations]).contains("transaction=3"));
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    let invalid = json!([
        {"op":"insert","table":"items","row":row(3,"must roll back")},
        {"op":"insert","table":"items","row":row(1,"duplicate")}
    ])
    .to_string();
    assert!(!run(&path, "tx", &[&invalid]).status.success());
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    let rows: serde_json::Value = serde_json::from_str(&ok(&path, "row-scan", &["items"])).unwrap();
    assert_eq!(rows, json!([row(1, "one"), row(2, "two")]));
}

#[test]
fn batch_inputs_are_bounded_and_errors_do_not_echo_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    ok(&path, "db-init", &["--durable"]);
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    let marker = "synthetic-sensitive-marker";
    for input in [
        format!(r#"[{{"op":"invented","value":"{marker}"}}]"#),
        format!(r#"[{{"op":"drop_table","table":"items","extra":"{marker}"}}]"#),
        " ".repeat(16385),
    ] {
        let output = run(&path, "tx", &[&input]);
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains(marker));
        assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    }
    let legacy = dir.path().join("legacy.emily");
    ok(&legacy, "db-init", &[]);
    assert!(!run(&legacy, "tx", &["[]"]).status.success());
    assert!(!run(&legacy, "checkpoint", &[]).status.success());
}

#[test]
fn operation_count_is_checked_before_touching_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    ok(&path, "db-init", &["--durable"]);
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    let operations = vec![json!({"op":"drop_table","table":"items"}); 257];
    let input = serde_json::to_string(&operations).unwrap();
    assert!(input.len() < 16384);
    let output = run(&path, "tx", &[&input]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("too many transaction operations"));
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
}

#[test]
fn batch_executes_all_operation_kinds_and_keeps_recreated_table_isolated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    ok(&path, "db-init", &["--durable"]);
    let typed_schema: serde_json::Value = serde_json::from_str(&schema()).unwrap();
    let key = json!({"type":"integer","value":1});
    let operations = json!([
        {"op":"create_table","schema":typed_schema},
        {"op":"insert","table":"items","row":row(1,"initial")},
        {"op":"update","table":"items","key":key,"row":row(1,"updated")},
        {"op":"delete","table":"items","key":key},
        {"op":"drop_table","table":"items"},
        {"op":"create_table","schema":typed_schema},
        {"op":"insert","table":"items","row":row(3,"fresh table")}
    ])
    .to_string();
    assert!(ok(&path, "tx", &[&operations]).contains("transaction=2"));
    let rows: serde_json::Value = serde_json::from_str(&ok(&path, "row-scan", &["items"])).unwrap();
    assert_eq!(rows, json!([row(3, "fresh table")]));
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    let invalid = json!([{
        "op":"update","table":"items",
        "key":{"type":"integer","value":3},"row":row(4,"changed primary key")
    }])
    .to_string();
    assert!(!run(&path, "tx", &[&invalid]).status.success());
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    let new_schema = schema().replace("items", "another");
    assert_eq!(
        ok(&path, "table-create", &[&new_schema]).trim(),
        "table_id=3"
    );
}
