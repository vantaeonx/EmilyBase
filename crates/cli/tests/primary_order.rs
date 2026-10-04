use emilybase_catalog::Value;
use emilybase_query::execute;
use emilybase_transactions::Database;
use std::process::{Command, Output};

fn json(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn actual_cli_orders_wide_rows_before_limit_and_keeps_readonly_history() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("source");
    let mut database = Database::create(&path).unwrap();
    execute(
        &mut database,
        "CREATE TABLE t(id INT PRIMARY KEY,payload TEXT)",
        &[],
    )
    .unwrap();
    for start in (0..6000).step_by(200) {
        let tuples = (start..start + 200)
            .map(|id| format!("({id},$1)"))
            .collect::<Vec<_>>()
            .join(",");
        execute(
            &mut database,
            &format!("INSERT INTO t VALUES {tuples}"),
            &[Value::Text("x".repeat(3072))],
        )
        .unwrap();
    }
    database.save_primary_index_cache("t").unwrap();
    let wal = database.committed_wal().unwrap();
    let transaction = database.last_transaction();
    drop(database);
    let run = |sql: &str, args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_emilybase"))
            .arg("sql")
            .arg(&path)
            .arg(sql)
            .args(args)
            .output()
            .unwrap()
    };
    let sql = "SELECT id AS key FROM t ORDER BY id DESC LIMIT $1";
    let parameters = "[{\"type\":\"integer\",\"value\":2}]";
    let plan = json(run(sql, &["--parameters", parameters, "--explain"]));
    assert_eq!(plan["access"], "primary_range");
    assert_eq!(plan["sorted"], true);
    let report = json(run(sql, &["--parameters", parameters]));
    assert_eq!(report["transaction"], transaction);
    assert_eq!(report["results"][0]["columns"], serde_json::json!(["key"]));
    assert_eq!(
        report["results"][0]["rows"],
        serde_json::json!([
            [{"type":"integer","value":5999}],
            [{"type":"integer","value":5998}]
        ])
    );
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), wal);
    let failure = run(
        "UPDATE t SET payload='staged' WHERE id=0; SELECT payload FROM t ORDER BY id DESC",
        &[],
    );
    assert!(!failure.status.success());
    assert!(failure.stdout.is_empty());
    let error = String::from_utf8(failure.stderr).unwrap();
    assert!(error.contains("output bytes"));
    assert!(!error.contains("staged"));
    assert!(!error.contains(path.to_str().unwrap()));
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), wal);
    let report = json(run(
        "SELECT id FROM t WHERE id>=5997 ORDER BY id DESC LIMIT 1",
        &[],
    ));
    assert_eq!(report["results"][0]["rows"][0][0]["value"], 5999);
    let database = Database::open(&path).unwrap();
    assert_eq!(database.primary_cache_startup().unwrap().loaded, 1);
    assert_eq!(database.last_transaction(), transaction);
}
