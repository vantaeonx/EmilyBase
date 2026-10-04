use emilybase_catalog::{Key, Value};
use emilybase_query::{ExecutionError, execute, query};
use emilybase_transactions::Database;

fn initialize(database: &mut Database) {
    execute(
        database,
        "CREATE TABLE t(id INT PRIMARY KEY,n INT,payload TEXT)",
        &[],
    )
    .unwrap();
    let payload = [Value::Text("я".repeat(1536))];
    for start in (0..6000).step_by(200) {
        let values = (start..start + 200)
            .map(|id| format!("({id},{},$1)", id % 5))
            .collect::<Vec<_>>()
            .join(",");
        execute(
            database,
            &format!("INSERT INTO t VALUES {values}"),
            &payload,
        )
        .unwrap();
    }
}

fn limited(database: &Database) -> Vec<Vec<Value>> {
    query(
        database.view().unwrap(),
        "SELECT id,n FROM t WHERE n>=4 ORDER BY id DESC LIMIT 2",
        &[],
    )
    .unwrap()
    .rows
}

#[test]
fn streamed_script_work_output_rollback_caches_and_verified_restore_use_both_wals() {
    for version in [1, 2] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("source");
        let mut database = Database::create(&path).unwrap();
        if version == 2 {
            database.compact().unwrap();
        }
        initialize(&mut database);
        let old = database.view().unwrap().clone();
        let initial = limited(&database);
        assert_eq!(
            initial,
            vec![
                vec![Value::Integer(5999), Value::Integer(4)],
                vec![Value::Integer(5994), Value::Integer(4)]
            ]
        );
        let original_wal = database.committed_wal().unwrap();
        let transaction = database.last_transaction();
        // The old sort path charged 6000 row visits per statement and hit 100000.
        let script = "SELECT id FROM t ORDER BY id DESC LIMIT 1;".repeat(64);
        let report = execute(&mut database, &script, &[]).unwrap();
        assert_eq!(report.results.len(), 64);
        assert!(
            report
                .results
                .iter()
                .all(|result| result.rows == vec![vec![Value::Integer(5999)]])
        );
        assert_eq!(report.transaction, transaction);
        assert_eq!(database.committed_wal().unwrap(), original_wal);

        // LIMIT counts matches. Each unsuccessful predicate still consumes work.
        let predicate = ["n<0"; 12].join(" AND ");
        let failed = execute(
            &mut database,
            &format!(
                "UPDATE t SET n=99 WHERE id=5999; SELECT id FROM t WHERE {predicate} ORDER BY id DESC LIMIT 1"
            ),
            &[],
        );
        assert!(matches!(failed, Err(ExecutionError::Limit("query work"))));
        assert_eq!(database.committed_wal().unwrap(), original_wal);
        assert_eq!(limited(&database), initial);
        assert_eq!(database.last_transaction(), transaction);

        // Budget output across the entire script, even when each ordered result fits.
        let script = "UPDATE t SET n=99 WHERE id=5999;".to_string()
            + &"SELECT payload FROM t ORDER BY id DESC LIMIT 700;".repeat(4);
        assert!(matches!(
            execute(&mut database, &script, &[]),
            Err(ExecutionError::Limit("output bytes"))
        ));
        assert_eq!(database.committed_wal().unwrap(), original_wal);
        assert_eq!(limited(&database), initial);

        let report = execute(&mut database,"BEGIN; UPDATE t SET n=99 WHERE id=5999; SELECT n FROM t ORDER BY id DESC LIMIT 1; ROLLBACK",&[]).unwrap();
        assert!(!report.committed);
        assert_eq!(report.results[1].rows, vec![vec![Value::Integer(99)]]);
        assert_eq!(database.committed_wal().unwrap(), original_wal);
        assert_eq!(limited(&database), initial);
        assert!(execute(&mut database,"UPDATE t SET n=99 WHERE id=5999; SELECT n FROM t ORDER BY id DESC LIMIT 1; INSERT INTO t VALUES(NULL,0,NULL)",&[]).is_err());
        assert_eq!(database.committed_wal().unwrap(), original_wal);

        let report = execute(&mut database,"UPDATE t SET n=9 WHERE id=5999; DELETE FROM t WHERE id=5994; SELECT id,n FROM t WHERE n>=4 ORDER BY id DESC LIMIT 2",&[]).unwrap();
        assert_eq!(report.transaction, transaction + 1);
        let current = vec![
            vec![Value::Integer(5999), Value::Integer(9)],
            vec![Value::Integer(5989), Value::Integer(4)],
        ];
        assert_eq!(report.results[2].rows, current);
        assert_eq!(limited(&database), current);
        assert_eq!(
            query(
                &old,
                "SELECT id,n FROM t WHERE n>=4 ORDER BY id DESC LIMIT 2",
                &[]
            )
            .unwrap()
            .rows,
            initial
        );
        let committed_wal = database.committed_wal().unwrap();
        database.save_primary_index_cache("t").unwrap();
        database.checkpoint().unwrap();
        drop(database);
        database = Database::open(&path).unwrap();
        assert_eq!(database.primary_cache_startup().unwrap().loaded, 1);
        assert_eq!(limited(&database), current);
        assert_eq!(database.committed_wal().unwrap(), committed_wal);

        let archive = temporary.path().join("synthetic.backup");
        let report = emilybase_backup::create(&mut database, &archive).unwrap();
        assert_eq!(report.wal_version, version);
        assert_eq!(report.last_transaction, transaction + 1);
        emilybase_backup::inspect(&archive).unwrap();
        let restored = temporary.path().join("restored");
        emilybase_backup::restore(&archive, &restored).unwrap();
        let mut copy = Database::open(&restored).unwrap();
        assert_eq!(copy.primary_cache_startup().unwrap().loaded, 0);
        assert_eq!(limited(&copy), current);
        assert_eq!(copy.committed_wal().unwrap(), committed_wal);
        execute(&mut copy, "UPDATE t SET n=10 WHERE id=5999", &[]).unwrap();
        assert_eq!(copy.last_transaction(), transaction + 2);
        assert_eq!(limited(&copy)[0][1], Value::Integer(10));
        assert_eq!(limited(&database), current);
    }
}

#[test]
fn long_point_keys_staged_views_and_dropped_tables_keep_ordered_read_semantics() {
    let temporary = tempfile::tempdir().unwrap();
    let mut database = Database::create(temporary.path().join("source")).unwrap();
    execute(
        &mut database,
        "CREATE TABLE words(n INT,id TEXT PRIMARY KEY)",
        &[],
    )
    .unwrap();
    let long = format!("a{}", "x".repeat(3071));
    let parameters = [Value::Text(long.clone())];
    execute(
        &mut database,
        "INSERT INTO words VALUES(1,'a'),(2,$1),(3,'b')",
        &parameters,
    )
    .unwrap();
    let before = database.committed_wal().unwrap();
    let sql = "SELECT n FROM words WHERE id=$1 ORDER BY id DESC LIMIT 1";
    assert_eq!(
        query(database.view().unwrap(), sql, &parameters)
            .unwrap()
            .rows,
        vec![vec![Value::Integer(2)]]
    );
    let report = execute(
        &mut database,
        &format!("BEGIN; UPDATE words SET n=7 WHERE id=$1; {sql}; ROLLBACK"),
        &parameters,
    )
    .unwrap();
    assert_eq!(report.results[1].rows, vec![vec![Value::Integer(7)]]);
    assert_eq!(database.committed_wal().unwrap(), before);
    assert_eq!(
        database
            .view()
            .unwrap()
            .get("words", &Key::Text(long))
            .unwrap()
            .unwrap()[0],
        Value::Integer(2)
    );
    let report = execute(&mut database,"DROP TABLE words; CREATE TABLE words(n INT,id INT PRIMARY KEY); INSERT INTO words VALUES(9,10); SELECT n FROM words ORDER BY id DESC LIMIT 1",&[]).unwrap();
    assert_eq!(report.results[3].rows, vec![vec![Value::Integer(9)]]);
}
