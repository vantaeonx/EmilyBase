use serde_json::Value;
use std::process::Command;

#[test]
fn actual_cli_explains_ranges_and_retains_atomic_mutation_and_reopen_behavior() {
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
    assert!(run("sql",&["CREATE TABLE t(id INTEGER PRIMARY KEY,n INTEGER); INSERT INTO t VALUES (1,10),(2,20),(3,30)"]).status.success());
    let plan = run(
        "sql",
        &["SELECT * FROM t WHERE id>=1 AND id<3", "--explain"],
    );
    assert!(plan.status.success());
    let plan: Value = serde_json::from_slice(&plan.stdout).unwrap();
    assert_eq!(plan["access"], "primary_range");
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    let rollback = run(
        "sql",
        &["BEGIN; DELETE FROM t WHERE id>=1 AND id<3; ROLLBACK"],
    );
    assert!(rollback.status.success());
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    let commit = run(
        "sql",
        &["UPDATE t SET n=99 WHERE id>=1 AND id<3; DELETE FROM t WHERE id>2"],
    );
    assert!(commit.status.success());
    assert!(run("compact", &[]).status.success());
    let found = run(
        "sql",
        &["SELECT n FROM t WHERE id>=1 AND id<=2 ORDER BY id DESC"],
    );
    assert!(found.status.success());
    let result: Value = serde_json::from_slice(&found.stdout).unwrap();
    assert_eq!(
        result["results"][0]["rows"],
        serde_json::json!([[{"type":"integer","value":99}],[{"type":"integer","value":99}]])
    );
    let current = std::fs::read(path.join("redo.wal")).unwrap();
    assert!(
        !run("sql", &["SELECT missing FROM t WHERE id>9 LIMIT 0"])
            .status
            .success()
    );
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), current);
}
