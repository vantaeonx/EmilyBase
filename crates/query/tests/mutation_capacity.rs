use emilybase_catalog::{Key, Value};
use emilybase_query::{ExecutionError, execute};
use emilybase_transactions::Database;

fn tuples(start: i64, end: i64) -> String {
    (start..end)
        .map(|id| format!("({id},0)"))
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
fn ddl_and_prior_writes_share_event_capacity_but_zero_match_mutations_do_not() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("source");
    let mut database = Database::create(&path).unwrap();
    let sql = format!(
        "CREATE TABLE t(id INT PRIMARY KEY,n INT); INSERT INTO t VALUES {}; SELECT id FROM t ORDER BY id DESC LIMIT 1; UPDATE t SET n=9 WHERE id=999; DELETE FROM t WHERE id<0",
        tuples(0, 255)
    );
    let report = execute(&mut database, &sql, &[]).unwrap();
    assert_eq!(report.results[1].affected, 255);
    assert_eq!(report.results[2].rows, vec![vec![Value::Integer(254)]]);
    assert_eq!(report.results[3].affected, 0);
    assert_eq!(report.results[4].affected, 0);
    execute(&mut database, "INSERT INTO t VALUES (255,0),(256,0)", &[]).unwrap();
    let wal = database.committed_wal().unwrap();
    let transaction = database.last_transaction();
    for script in [
        "UPDATE t SET n=1",
        "DELETE FROM t",
        "CREATE TABLE extra(id INT PRIMARY KEY); UPDATE t SET n=1 WHERE id<256",
        "UPDATE t SET n=9 WHERE id=256; DELETE FROM t WHERE id<256",
        "DELETE FROM t WHERE id<256; DELETE FROM t WHERE id=256",
    ] {
        assert!(
            matches!(
                execute(&mut database, script, &[]),
                Err(ExecutionError::Transaction(
                    emilybase_transactions::Error::Limit
                ))
            ),
            "{script}"
        );
        assert_eq!(database.committed_wal().unwrap(), wal);
        assert_eq!(database.last_transaction(), transaction);
        assert_eq!(database.view().unwrap().row_count(), 257);
        assert!(database.view().unwrap().schema("extra").is_err());
        assert_eq!(
            database
                .view()
                .unwrap()
                .get("t", &Key::Integer(256))
                .unwrap()
                .unwrap()[1],
            Value::Integer(0)
        );
    }
    // 256 total selected writes fit even when two statements share the capacity.
    let report = execute(
        &mut database,
        "UPDATE t SET n=9 WHERE id=256; UPDATE t SET n=1 WHERE id<255; DELETE FROM t WHERE id=999",
        &[],
    )
    .unwrap();
    assert_eq!(
        report
            .results
            .iter()
            .map(|r| r.affected)
            .collect::<Vec<_>>(),
        [1, 255, 0]
    );
    assert_eq!(report.transaction, transaction + 1);
    drop(database);
    let database = Database::open(&path).unwrap();
    assert_eq!(database.last_transaction(), transaction + 1);
    assert_eq!(
        database
            .view()
            .unwrap()
            .get("t", &Key::Integer(0))
            .unwrap()
            .unwrap()[1],
        Value::Integer(1)
    );
}

#[test]
fn mutation_binding_is_complete_even_at_zero_capacity_empty_ranges_and_no_matches() {
    let temporary = tempfile::tempdir().unwrap();
    let mut database = Database::create(temporary.path().join("source")).unwrap();
    execute(
        &mut database,
        &format!(
            "CREATE TABLE t(id INT PRIMARY KEY,n INT); INSERT INTO t VALUES {}",
            tuples(0, 255)
        ),
        &[],
    )
    .unwrap();
    let before = database.committed_wal().unwrap();
    let prefix =
        "UPDATE t SET n=1 WHERE id>=0 AND id<255; UPDATE t SET n=2 WHERE id=0;".to_string();
    for suffix in [
        "UPDATE t SET absent=0 WHERE id>9 AND id<3",
        "UPDATE t SET n='wrong' WHERE id>9 AND id<3",
        "UPDATE t SET n=$1 WHERE id>9 AND id<3",
        "UPDATE t SET n=1 WHERE id>9 AND id<3 AND absent=0",
        "DELETE FROM t WHERE id>9 AND id<3 AND n='wrong'",
        "DELETE FROM t WHERE id>9 AND id<3 AND n=$1",
    ] {
        assert!(execute(&mut database, &(prefix.clone() + suffix), &[]).is_err());
        assert_eq!(database.committed_wal().unwrap(), before);
    }
    let report = execute(&mut database,"UPDATE t SET n=1 WHERE id>=0 AND id<255; UPDATE t SET n=2 WHERE id=0; UPDATE t SET n=9 WHERE id=NULL; DELETE FROM t WHERE id>9223372036854775807",&[]).unwrap();
    assert_eq!(
        report
            .results
            .iter()
            .map(|r| r.affected)
            .collect::<Vec<_>>(),
        [255, 1, 0, 0]
    );
    let before = database.committed_wal().unwrap();
    assert!(matches!(
        execute(
            &mut database,
            "UPDATE t SET n=3 WHERE id<255; DELETE FROM t WHERE id=0; DELETE FROM t WHERE id=1",
            &[]
        ),
        Err(ExecutionError::Transaction(
            emilybase_transactions::Error::Limit
        ))
    ));
    assert_eq!(database.committed_wal().unwrap(), before);
}
