use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn actual_cli_saves_loads_refreshes_and_rejects_damage_without_changing_wal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let run = |action: &str, args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_emilybase"))
            .arg(action)
            .arg(&path)
            .args(args)
            .output()
            .unwrap()
    };
    assert!(run("db-init", &["--durable"]).status.success());
    assert!(
        run(
            "sql",
            &["CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (7)"]
        )
        .status
        .success()
    );
    let missing = run("primary-index-load", &["t"]);
    assert!(missing.status.success());
    assert_eq!(missing.stdout, b"null\n");
    let wal = fs::read(path.join("redo.wal")).unwrap();
    let saved = run("primary-index-save", &["t"]);
    assert!(saved.status.success());
    let report: serde_json::Value = serde_json::from_slice(&saved.stdout).unwrap();
    assert_eq!(report["entries"], 1);
    assert_eq!(report["table_id"], 1);
    let active = path.join("primary-1.table-index");
    assert_eq!(
        fs::metadata(&active).unwrap().permissions().mode() & 0o7777,
        0o600
    );
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), wal);
    let loaded = run("primary-index-load", &["t"]);
    assert!(loaded.status.success());
    let info: serde_json::Value = serde_json::from_slice(&loaded.stdout).unwrap();
    assert_eq!(info["entries"], 1);
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), wal);
    assert!(run("sql", &["INSERT INTO t VALUES (8)"]).status.success());
    assert!(!run("primary-index-load", &["t"]).status.success());
    assert!(run("primary-index-save", &["t"]).status.success());
    let old = fs::read(&active).unwrap();
    assert!(!run("primary-index-save", &["../t"]).status.success());
    assert_eq!(fs::read(&active).unwrap(), old);
    let wal = fs::read(path.join("redo.wal")).unwrap();
    fs::write(&active, b"synthetic-cache-contents-must-not-be-reflected").unwrap();
    for action in ["primary-index-load", "primary-index-save"] {
        let denied = run(action, &["t"]);
        assert!(!denied.status.success());
        assert!(!String::from_utf8_lossy(&denied.stderr).contains("synthetic-cache-contents"));
        assert_eq!(
            fs::read(&active).unwrap(),
            b"synthetic-cache-contents-must-not-be-reflected"
        );
        assert_eq!(fs::read(path.join("redo.wal")).unwrap(), wal);
    }
    assert!(
        run("sql", &["SELECT * FROM t WHERE id >= 7"])
            .status
            .success()
    );
    fs::remove_file(active).unwrap();
    assert!(run("primary-index-save", &["t"]).status.success());
    assert!(run("primary-index-load", &["t"]).status.success());
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), wal);
}
