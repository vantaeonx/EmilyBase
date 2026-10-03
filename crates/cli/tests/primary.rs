use serde_json::Value;
use std::process::Command;

#[test]
fn actual_cli_inspects_managed_primary_tree_without_writing_journal_or_optional_index_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let run = |command: &str, args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_emilybase"))
            .arg(command)
            .arg(&path)
            .args(args)
            .output()
            .unwrap()
    };
    assert!(run("db-init", &["--durable"]).status.success());
    assert!(
        run(
            "sql",
            &["CREATE TABLE t (id TEXT PRIMARY KEY); INSERT INTO t VALUES ('short')"]
        )
        .status
        .success()
    );
    let long = "界".repeat(1024);
    let parameters = serde_json::json!([{"type":"text","value":long}]).to_string();
    assert!(
        run(
            "sql",
            &["INSERT INTO t VALUES ($1)", "--parameters", &parameters]
        )
        .status
        .success()
    );
    let wal = std::fs::read(path.join("redo.wal")).unwrap();
    let entries = std::fs::read_dir(&path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    for _ in 0..2 {
        let output = run("primary-index-info", &["t"]);
        assert!(output.status.success());
        let info: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            info,
            serde_json::json!({"entries":1,"excluded_long_keys":1,"pages":1,"root_id":1})
        );
        assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), wal);
        assert_eq!(
            std::fs::read_dir(&path)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect::<Vec<_>>(),
            entries
        );
    }
    assert!(!run("primary-index-info", &["missing"]).status.success());
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), wal);
}
