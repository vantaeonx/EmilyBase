use emilybase_catalog::Value;
use emilybase_query::{execute, query};
use emilybase_transactions::Database;

#[test]
fn sql_point_reads_and_writes_follow_staged_primary_tree_through_rollback_restore_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut database = Database::create(&path).unwrap();
    execute(
        &mut database,
        "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1,'one'),(2,'two')",
        &[],
    )
    .unwrap();
    let original = database.view().unwrap().clone();
    original.primary_index_info("t").unwrap();
    let before = database.committed_wal().unwrap();
    let report=execute(&mut database,"BEGIN; UPDATE t SET v='staged' WHERE id=1; SELECT v FROM t WHERE id=1; DELETE FROM t WHERE id=2; SELECT * FROM t WHERE id=2; ROLLBACK",&[]).unwrap();
    assert!(!report.committed);
    assert_eq!(
        report.results[1].rows,
        vec![vec![Value::Text("staged".into())]]
    );
    assert!(report.results[3].rows.is_empty());
    assert_eq!(database.committed_wal().unwrap(), before);
    assert_eq!(
        query(database.view().unwrap(), "SELECT v FROM t WHERE id=1", &[])
            .unwrap()
            .rows,
        vec![vec![Value::Text("one".into())]]
    );
    let report = execute(
        &mut database,
        "UPDATE t SET v=$1 WHERE id=$2; SELECT v FROM t WHERE id=$2",
        &[Value::Text("committed".into()), Value::Integer(1)],
    )
    .unwrap();
    assert_eq!(
        report.results[1].rows,
        vec![vec![Value::Text("committed".into())]]
    );
    assert!(
        execute(
            &mut database,
            "UPDATE t SET v='discard'; INSERT INTO t VALUES (1,'duplicate')",
            &[]
        )
        .is_err()
    );
    for compacted in [false, true] {
        if compacted {
            database.compact().unwrap();
        }
        let archive = dir.path().join(format!("copy-{compacted}.backup"));
        emilybase_backup::create(&mut database, &archive).unwrap();
        let target = dir.path().join(format!("restored-{compacted}"));
        emilybase_backup::restore(&archive, &target).unwrap();
        let restored = Database::open(target).unwrap();
        assert_eq!(
            query(
                restored.view().unwrap(),
                "SELECT v FROM t WHERE id=$1",
                &[Value::Integer(1)]
            )
            .unwrap()
            .rows,
            vec![vec![Value::Text("committed".into())]]
        );
        assert_eq!(
            restored
                .view()
                .unwrap()
                .primary_index_info("t")
                .unwrap()
                .entries,
            2
        );
    }
    drop(database);
    let database = Database::open(path).unwrap();
    assert_eq!(
        query(database.view().unwrap(), "SELECT v FROM t WHERE id=1", &[])
            .unwrap()
            .rows,
        vec![vec![Value::Text("committed".into())]]
    );
    assert_eq!(
        query(&original, "SELECT v FROM t WHERE id=1", &[])
            .unwrap()
            .rows,
        vec![vec![Value::Text("one".into())]]
    );
}

#[test]
fn sql_long_primary_keys_and_null_values_keep_their_previous_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let mut database = Database::create(dir.path().join("db")).unwrap();
    execute(
        &mut database,
        "CREATE TABLE t (id TEXT PRIMARY KEY, n INTEGER)",
        &[],
    )
    .unwrap();
    let short = Value::Text("s".repeat(256));
    let long = Value::Text("界".repeat(1024));
    for key in [&short, &long] {
        execute(
            &mut database,
            "INSERT INTO t VALUES ($1,NULL)",
            std::slice::from_ref(key),
        )
        .unwrap();
    }
    let before = database.committed_wal().unwrap();
    for key in [&short, &long] {
        let found = query(
            database.view().unwrap(),
            "SELECT n FROM t WHERE id=$1",
            std::slice::from_ref(key),
        )
        .unwrap();
        assert_eq!(found.rows, vec![vec![Value::Null]]);
        assert!(
            query(
                database.view().unwrap(),
                "SELECT n FROM t WHERE id=$1 AND n=NULL",
                std::slice::from_ref(key)
            )
            .unwrap()
            .rows
            .is_empty()
        );
    }
    let info = database.view().unwrap().primary_index_info("t").unwrap();
    assert_eq!((info.entries, info.excluded_long_keys), (1, 1));
    assert_eq!(database.committed_wal().unwrap(), before);
    execute(
        &mut database,
        "UPDATE t SET n=7 WHERE id=$1; DELETE FROM t WHERE id=$2",
        &[long.clone(), short],
    )
    .unwrap();
    assert_eq!(
        query(
            database.view().unwrap(),
            "SELECT n FROM t WHERE id=$1",
            &[long]
        )
        .unwrap()
        .rows,
        vec![vec![Value::Integer(7)]]
    );
    assert_eq!(
        database
            .view()
            .unwrap()
            .primary_index_info("t")
            .unwrap()
            .excluded_long_keys,
        1
    );
}
