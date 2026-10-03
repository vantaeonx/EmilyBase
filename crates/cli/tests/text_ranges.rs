use std::process::Command;

#[test]
fn actual_cli_plans_and_mutates_text_ranges_including_a_maximum_length_key() {
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
    assert!(run("sql", &["CREATE TABLE t(id TEXT PRIMARY KEY,n INT); INSERT INTO t VALUES ('a',1); INSERT INTO t VALUES ('b',2)"]).status.success());
    let parameters =
        serde_json::json!([{"type":"text","value":format!("a{}","x".repeat(3071))}]).to_string();
    assert!(
        run(
            "sql",
            &["INSERT INTO t VALUES ($1,7)", "--parameters", &parameters]
        )
        .status
        .success()
    );
    let source = "SELECT n FROM t WHERE id>='a' AND id<'b' ORDER BY id";
    let plan = run("sql", &[source, "--explain"]);
    assert!(plan.status.success());
    let plan: serde_json::Value = serde_json::from_slice(&plan.stdout).unwrap();
    assert_eq!(plan["access"], "primary_range");
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    let selected = run("sql", &[source]);
    assert!(selected.status.success());
    let selected: serde_json::Value = serde_json::from_slice(&selected.stdout).unwrap();
    assert_eq!(
        selected["results"][0]["rows"],
        serde_json::json!([[{"type":"integer","value":1}],[{"type":"integer","value":7}]])
    );
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    assert!(
        run(
            "sql",
            &["BEGIN; UPDATE t SET n=8 WHERE id>'a' AND id<'b'; ROLLBACK"]
        )
        .status
        .success()
    );
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    let written = run("sql", &["UPDATE t SET n=8 WHERE id>'a' AND id<'b'"]);
    assert!(written.status.success());
    let written: serde_json::Value = serde_json::from_slice(&written.stdout).unwrap();
    assert_eq!(written["results"][0]["affected"], 1);
    let selected = run("sql", &[source]);
    assert!(selected.status.success());
    let selected: serde_json::Value = serde_json::from_slice(&selected.stdout).unwrap();
    assert_eq!(selected["results"][0]["rows"][1][0]["value"], 8);
}
