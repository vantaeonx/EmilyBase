use emilybase_catalog::{Key, Value};
use emilybase_query::execute;
use emilybase_transactions::Database;
use std::process::Command;

#[test]
fn actual_cli_limits_whole_text_mutations_and_checks_long_point_keys() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("source");
    let mut database = Database::create(&path).unwrap();
    execute(
        &mut database,
        "CREATE TABLE t(id TEXT PRIMARY KEY,n INT)",
        &[],
    )
    .unwrap();
    for start in [0, 150] {
        let mut transaction = database.begin().unwrap();
        for id in start..start + 150 {
            transaction
                .insert(
                    "t",
                    vec![Value::Text(format!("k{id:05}")), Value::Integer(id)],
                )
                .unwrap();
        }
        transaction.commit().unwrap();
    }
    let long = format!("a{}", "x".repeat(3071));
    execute(
        &mut database,
        "INSERT INTO t VALUES($1,0)",
        &[Value::Text(long.clone())],
    )
    .unwrap();
    database.save_primary_index_cache("t").unwrap();
    let before = database.committed_wal().unwrap();
    let transaction = database.last_transaction();
    drop(database);
    let parameters = serde_json::json!([{"type":"text","value":long}]).to_string();
    let run = |sql: &str| {
        Command::new(env!("CARGO_BIN_EXE_emilybase"))
            .arg("sql")
            .arg(&path)
            .arg(sql)
            .args(["--parameters", &parameters])
            .output()
            .unwrap()
    };
    let failed = run("UPDATE t SET n=9 WHERE id=$1; DELETE FROM t");
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    let error = String::from_utf8(failed.stderr).unwrap();
    assert!(error.contains("transaction limit"));
    for private in [
        &long,
        path.to_str().unwrap(),
        "UPDATE t SET",
        "DELETE FROM t",
    ] {
        assert!(!error.contains(private));
    }
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    let accepted = run(
        "DELETE FROM t WHERE id>='k00000' AND id<'k00255'; UPDATE t SET n=7 WHERE id=$1; SELECT n FROM t WHERE id=$1 ORDER BY id DESC LIMIT 1",
    );
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&accepted.stdout).unwrap();
    assert_eq!(report["transaction"], transaction + 1);
    assert_eq!(report["results"][0]["affected"], 255);
    assert_eq!(report["results"][1]["affected"], 1);
    assert_eq!(report["results"][2]["rows"][0][0]["value"], 7);
    let database = Database::open(&path).unwrap();
    assert_eq!(database.primary_cache_startup().unwrap().rejected, 1);
    assert_eq!(database.last_transaction(), transaction + 1);
    assert_eq!(database.view().unwrap().row_count(), 46);
    assert_eq!(
        database
            .view()
            .unwrap()
            .get("t", &Key::Text(long))
            .unwrap()
            .unwrap()[1],
        Value::Integer(7)
    );
}
