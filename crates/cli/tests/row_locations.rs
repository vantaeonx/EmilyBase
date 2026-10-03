use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Output};

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
fn initialized(path: &Path) {
    ok(path, "db-init", &["--durable"]);
    ok(
        path,
        "sql",
        &[
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (7,'synthetic private payload')",
        ],
    );
}
const KEY: &str = r#"{"type":"integer","value":7}"#;

#[test]
fn actual_cli_resolves_current_locations_after_compaction_and_refuses_stale_and_foreign_images() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a");
    let foreign = dir.path().join("b");
    initialized(&path);
    initialized(&foreign);
    let location = ok(&path, "row-location", &["t", KEY]);
    let value: Value = serde_json::from_str(&location).unwrap();
    assert_eq!(value["database_id"].as_array().unwrap().len(), 16);
    assert_eq!(value["row"]["fingerprint"].as_array().unwrap().len(), 32);
    assert_eq!(
        serde_json::from_str::<Value>(&ok(&path, "row-resolve", &["t", KEY, &location])).unwrap(),
        json!([{"type":"integer","value":7},{"type":"text","value":"synthetic private payload"}])
    );
    ok(&path, "checkpoint", &[]);
    ok(&path, "compact", &[]);
    assert_eq!(ok(&path, "row-location", &["t", KEY]), location);
    assert!(
        run(&foreign, "row-resolve", &["t", KEY, &location])
            .status
            .code()
            .is_some_and(|code| code != 0)
    );
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    let mut forged = value;
    forged["row"]["page_id"] = json!(u64::MAX);
    assert!(
        !run(&path, "row-resolve", &["t", KEY, &forged.to_string()])
            .status
            .success()
    );
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    ok(&path, "sql", &["UPDATE t SET v='replacement' WHERE id=7"]);
    let stale = run(&path, "row-resolve", &["t", KEY, &location]);
    assert!(!stale.status.success());
    assert!(stale.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&stale.stderr).contains("synthetic private payload"));
    assert_ne!(ok(&path, "row-location", &["t", KEY]), location);
    ok(&path, "sql", &["DELETE FROM t WHERE id=7"]);
    assert_eq!(ok(&path, "row-location", &["t", KEY]).trim(), "null");
    assert!(
        !run(&path, "row-resolve", &["t", KEY, &location])
            .status
            .success()
    );
}

#[test]
fn actual_cli_rejects_malformed_extra_and_oversized_location_json_without_echoing_input() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    initialized(&path);
    let wal = std::fs::read(path.join("redo.wal")).unwrap();
    let mut extra: Value = serde_json::from_str(&ok(&path, "row-location", &["t", KEY])).unwrap();
    extra["synthetic_private_extra"] = json!("synthetic private marker");
    for bad in [
        "synthetic private marker".into(),
        extra.to_string(),
        "x".repeat(16385),
    ] {
        let output = run(&path, "row-resolve", &["t", KEY, &bad]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!error.contains("synthetic private marker"));
        assert!(!error.contains("synthetic_private_extra"));
        assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), wal);
    }
}
