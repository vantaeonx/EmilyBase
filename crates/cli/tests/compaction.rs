use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::json;

static PROCESS_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    ],"primary_key":0});
    let operations = json!([
        {"op":"create_table","schema":schema},
        {"op":"insert","table":"items","row":[
            {"type":"integer","value":7},{"type":"text","value":"synthetic-private-marker"}
        ]}
    ])
    .to_string();
    ok("tx", &[path], &[&operations]);
}

#[test]
fn actual_cli_compacts_without_changing_boundary_and_restores_both_archive_versions() {
    let _guard = PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("база с пробелами");
    let old = dir.path().join("до.backup");
    let new = dir.path().join("после.backup");
    seed(&source);
    let rows = ok("row-scan", &[&source], &["items"]);
    let old_report = ok("backup", &[&source, &old], &[]);
    assert!(old_report.contains("transaction=2"));
    let old_bytes = fs::read(&old).unwrap();
    let output = ok("compact", &[&source], &[]);
    assert!(output.contains("transaction=2"));
    assert!(!output.contains("synthetic-private-marker"));
    assert_eq!(
        &fs::read(source.join("redo.wal")).unwrap()[8..10],
        &2u16.to_le_bytes()
    );
    assert_eq!(ok("row-scan", &[&source], &["items"]), rows);
    let new_report = ok("backup", &[&source, &new], &[]);
    assert!(new_report.contains("transaction=2"));
    assert_eq!(&fs::read(&new).unwrap()[12..14], &2u16.to_le_bytes());
    for (index, archive) in [&old, &new].into_iter().enumerate() {
        let target = dir.path().join(format!("restored-{index}"));
        ok("backup-verify", &[archive], &[]);
        ok("restore", &[archive, &target], &[]);
        assert_eq!(ok("row-scan", &[&target], &["items"]), rows);
        ok("compact", &[&target], &[]);
        ok(
            "row-insert",
            &[&target],
            &[
                "items",
                &json!([
                    {"type":"integer","value":8},{"type":"text","value":"after restore"}
                ])
                .to_string(),
            ],
        );
        let current: serde_json::Value =
            serde_json::from_str(&ok("row-scan", &[&target], &["items"])).unwrap();
        assert_eq!(current.as_array().unwrap().len(), 2);
        ok("compact", &[&target], &[]);
    }
    assert_eq!(fs::read(&old).unwrap(), old_bytes);
    assert_eq!(ok("backup-verify", &[&old], &[]), old_report);
    assert_eq!(ok("backup-verify", &[&new], &[]), new_report);
    assert_eq!(ok("row-scan", &[&source], &["items"]), rows);
}

#[test]
fn compact_cli_refuses_busy_legacy_and_damaged_sources_without_leaking_rows() {
    let _guard = PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    seed(&source);
    let before = fs::read(source.join("redo.wal")).unwrap();
    let owner = emilybase_transactions::Database::open(&source).unwrap();
    let busy = run("compact", &[&source], &[]);
    assert!(!busy.status.success());
    assert!(!String::from_utf8_lossy(&busy.stderr).contains("synthetic-private-marker"));
    assert_eq!(fs::read(source.join("redo.wal")).unwrap(), before);
    drop(owner);
    let legacy = dir.path().join("legacy.emily");
    ok("db-init", &[&legacy], &[]);
    let legacy_bytes = fs::read(&legacy).unwrap();
    assert!(!run("compact", &[&legacy], &[]).status.success());
    assert_eq!(fs::read(legacy).unwrap(), legacy_bytes);
    let mut damaged = before;
    damaged[100] ^= 1;
    fs::write(source.join("redo.wal"), &damaged).unwrap();
    let output = run("compact", &[&source], &[]);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-private-marker"));
    assert_eq!(fs::read(source.join("redo.wal")).unwrap(), damaged);
    assert!(!source.join("redo-next.wal").exists());
}

#[test]
fn opening_and_checkpointing_never_silently_upgrade_the_wal() {
    let _guard = PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    seed(&source);
    let before = fs::read(source.join("redo.wal")).unwrap();
    ok("table-list", &[&source], &[]);
    ok("checkpoint", &[&source], &[]);
    assert_eq!(fs::read(source.join("redo.wal")).unwrap(), before);
    let output = run("compact", &[&source], &["--unexpected"]);
    assert!(!output.status.success());
    assert_eq!(fs::read(source.join("redo.wal")).unwrap(), before);
}
