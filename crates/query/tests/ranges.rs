use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_query::{execute, explain, query};
use emilybase_transactions::Database;
use proptest::prelude::*;

fn initialized(path: &std::path::Path, keys: &[i64]) -> Database {
    let mut database = Database::create(path).unwrap();
    let mut transaction = database.begin().unwrap();
    transaction
        .create_table(Schema {
            name: "t".into(),
            columns: vec![
                Column {
                    name: "id".into(),
                    data_type: DataType::Integer,
                    nullable: false,
                },
                Column {
                    name: "n".into(),
                    data_type: DataType::Integer,
                    nullable: true,
                },
            ],
            primary_key: 0,
        })
        .unwrap();
    for key in keys {
        transaction
            .insert(
                "t",
                vec![
                    Value::Integer(*key),
                    if *key % 3 == 0 {
                        Value::Null
                    } else {
                        Value::Integer(*key)
                    },
                ],
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    database
}
fn result(database: &Database, sql: &str, parameters: &[Value]) -> Vec<i64> {
    query(database.view().unwrap(), sql, parameters)
        .unwrap()
        .rows
        .into_iter()
        .map(|row| match row[0] {
            Value::Integer(n) => n,
            _ => panic!("wrong type"),
        })
        .collect()
}

#[test]
fn integer_sql_ranges_handle_reversed_nested_and_overflowing_bounds_without_wrapping() {
    let dir = tempfile::tempdir().unwrap();
    let database = initialized(
        &dir.path().join("db"),
        &[i64::MIN, -2, -1, 0, 1, 2, 3, i64::MAX],
    );
    let cases = [
        ("id >= $1 AND id < $2", -1, 3, vec![-1, 0, 1, 2]),
        ("$1 <= id AND $2 > id", -1, 3, vec![-1, 0, 1, 2]),
        ("id > $1 AND id <= $2", -1, 3, vec![0, 1, 2, 3]),
        ("id > $1", i64::MAX, 0, vec![]),
        (
            "id <= $1",
            i64::MAX,
            0,
            vec![i64::MIN, -2, -1, 0, 1, 2, 3, i64::MAX],
        ),
        ("id >= $1", i64::MAX, 0, vec![i64::MAX]),
        ("id < $1", i64::MIN, 0, vec![]),
        ("id >= $1 AND id < $2", 3, -1, vec![]),
    ];
    for (filter, a, b, expected) in cases {
        let sql = format!("SELECT id FROM t WHERE {filter} ORDER BY id");
        let parameters = [Value::Integer(a), Value::Integer(b)];
        assert_eq!(
            explain(database.view().unwrap(), &sql, &parameters)
                .unwrap()
                .access,
            "primary_range"
        );
        assert_eq!(result(&database, &sql, &parameters), expected);
    }
    assert_eq!(
        result(
            &database,
            "SELECT id FROM t WHERE id>=-1 AND (id<3 AND id>=0) AND id<2 ORDER BY id",
            &[]
        ),
        vec![0, 1]
    );
    assert_eq!(
        result(
            &database,
            "SELECT id FROM t WHERE id>=0 AND id<=3 ORDER BY id DESC LIMIT 2",
            &[]
        ),
        vec![3, 2]
    );
}

#[test]
fn range_extraction_preserves_or_not_null_and_schema_validation_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let mut database = initialized(&dir.path().join("db"), &[-2, -1, 0, 1, 2, 3]);
    for sql in [
        "SELECT id FROM t WHERE id<0 OR id>2",
        "SELECT id FROM t WHERE NOT (id<0)",
        "SELECT id FROM t WHERE n>=0",
        "SELECT id FROM t WHERE id>=NULL",
    ] {
        assert_eq!(
            explain(database.view().unwrap(), sql, &[]).unwrap().access,
            "scan"
        );
    }
    assert_eq!(
        result(
            &database,
            "SELECT id FROM t WHERE id<0 OR id>2 ORDER BY id",
            &[]
        ),
        vec![-2, -1, 3]
    );
    assert_eq!(
        result(
            &database,
            "SELECT id FROM t WHERE id>=0 AND (n=NULL OR n=1) ORDER BY id",
            &[]
        ),
        vec![1]
    );
    assert_eq!(
        explain(
            database.view().unwrap(),
            "SELECT id FROM t WHERE id=1 AND id>9",
            &[]
        )
        .unwrap()
        .access,
        "primary_key"
    );
    assert!(result(&database, "SELECT id FROM t WHERE id=1 AND id>9", &[]).is_empty());
    for sql in [
        "SELECT missing FROM t WHERE id>9 LIMIT 0",
        "SELECT id FROM t WHERE id>9 AND missing=1",
        "SELECT id FROM t WHERE id>$1 LIMIT 0",
        "SELECT id FROM t WHERE id>'invalid' LIMIT 0",
    ] {
        assert!(query(database.view().unwrap(), sql, &[]).is_err());
    }
    execute(
        &mut database,
        "CREATE TABLE s(id TEXT PRIMARY KEY); INSERT INTO s VALUES ('long')",
        &[],
    )
    .unwrap();
    assert_eq!(
        explain(
            database.view().unwrap(),
            "SELECT id FROM s WHERE id>'a'",
            &[]
        )
        .unwrap()
        .access,
        "primary_range"
    );
    assert_eq!(
        explain(
            database.view().unwrap(),
            "SELECT a.id FROM t AS a JOIN t AS b ON a.id=b.id WHERE a.id>=0",
            &[]
        )
        .unwrap()
        .access,
        "bounded_nested_loop"
    );
}

#[test]
fn ranges_resolve_nonleading_primary_columns_and_aliases_before_eliminating_empty_results() {
    let dir = tempfile::tempdir().unwrap();
    let mut database = Database::create(dir.path().join("db")).unwrap();
    execute(&mut database,"CREATE TABLE t(n INTEGER,id INTEGER PRIMARY KEY); INSERT INTO t VALUES (10,1),(20,2),(30,3)",&[]).unwrap();
    let sql = "SELECT x.id FROM t AS x WHERE x.id>=1 AND x.id<3 ORDER BY x.id";
    assert_eq!(
        explain(database.view().unwrap(), sql, &[]).unwrap().access,
        "primary_range"
    );
    assert_eq!(result(&database, sql, &[]), vec![1, 2]);
    let change = execute(&mut database, "UPDATE t SET n=99 WHERE id>=1 AND id<3", &[]).unwrap();
    assert_eq!(change.results[0].affected, 2);
    assert_eq!(
        query(
            database.view().unwrap(),
            "SELECT n FROM t WHERE id>=1 AND id<3 ORDER BY id",
            &[]
        )
        .unwrap()
        .rows,
        vec![vec![Value::Integer(99)]; 2]
    );
    for sql in [
        "SELECT t.id FROM t AS x WHERE x.id>99 LIMIT 0",
        "SELECT x.id FROM t AS x WHERE x.missing>99 LIMIT 0",
    ] {
        assert!(query(database.view().unwrap(), sql, &[]).is_err());
    }
    let before = database.committed_wal().unwrap();
    for sql in [
        "UPDATE t SET missing=1 WHERE id>99",
        "DELETE FROM t WHERE id>99 AND missing=1",
        "UPDATE t SET n=$1 WHERE id>99",
    ] {
        assert!(execute(&mut database, sql, &[]).is_err());
        assert_eq!(database.committed_wal().unwrap(), before);
    }
}

#[test]
fn ranged_writes_are_atomic_through_rollback_reopen_compaction_and_verified_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut database = initialized(&path, &(0..32).collect::<Vec<_>>());
    let before = database.committed_wal().unwrap();
    let report=execute(&mut database,"BEGIN; UPDATE t SET n=99 WHERE id>=10 AND id<20; DELETE FROM t WHERE id>=0 AND id<5; SELECT id FROM t WHERE id>=0 AND id<10; ROLLBACK",&[]).unwrap();
    assert_eq!(report.results[0].affected, 10);
    assert_eq!(report.results[1].affected, 5);
    assert!(!report.committed);
    assert_eq!(database.committed_wal().unwrap(), before);
    assert!(
        execute(
            &mut database,
            "UPDATE t SET n=99 WHERE id>=10 AND id<20; INSERT INTO t VALUES (1,0)",
            &[]
        )
        .is_err()
    );
    assert_eq!(database.committed_wal().unwrap(), before);
    let report = execute(
        &mut database,
        "UPDATE t SET n=99 WHERE id>=10 AND id<20; DELETE FROM t WHERE id>=0 AND id<5",
        &[],
    )
    .unwrap();
    assert_eq!(report.results[0].affected, 10);
    assert_eq!(report.results[1].affected, 5);
    let transaction = database.last_transaction();
    for compacted in [false, true] {
        if compacted {
            database.compact().unwrap();
        }
        let archive = dir.path().join(format!("{compacted}.backup"));
        emilybase_backup::create(&mut database, &archive).unwrap();
        let target = dir.path().join(format!("copy-{compacted}"));
        emilybase_backup::restore(&archive, &target).unwrap();
        let copy = Database::open(target).unwrap();
        assert_eq!(
            result(
                &copy,
                "SELECT id FROM t WHERE id>=0 AND id<10 ORDER BY id",
                &[]
            ),
            (5..10).collect::<Vec<_>>()
        );
        assert_eq!(
            query(
                copy.view().unwrap(),
                "SELECT n FROM t WHERE id>=10 AND id<20",
                &[]
            )
            .unwrap()
            .rows,
            vec![vec![Value::Integer(99)]; 10]
        );
        assert_eq!(copy.last_transaction(), transaction);
    }
    drop(database);
    let database = Database::open(path).unwrap();
    assert_eq!(
        result(
            &database,
            "SELECT id FROM t WHERE id>=0 AND id<10 ORDER BY id",
            &[]
        ),
        (5..10).collect::<Vec<_>>()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn generated_sql_comparisons_match_an_independent_integer_model(
        a in prop_oneof![any::<i64>(),-12i64..12],b in prop_oneof![any::<i64>(),-12i64..12],limit in 0usize..30,descending in any::<bool>()
    ) {
        let dir=tempfile::tempdir().unwrap();
        let input=[i64::MIN,-10,-3,-1,0,1,2,3,10,i64::MAX];
        let database=initialized(&dir.path().join("db"),&input);
        for (include_lower,include_upper) in [(true,false),(false,true)] {
            let operators=if include_lower {(">=","<")} else {(">","<=")};
            let direction=if descending {"DESC"} else {"ASC"};
            let sql=format!("SELECT id FROM t WHERE id{}$1 AND id{}$2 ORDER BY id {direction} LIMIT $3",operators.0,operators.1);
            let mut expected=input.iter().filter(|key| if include_lower {**key>=a}else{**key>a})
                .filter(|key|if include_upper {**key<=b}else{**key<b}).copied().collect::<Vec<_>>();
            if descending {expected.reverse();}expected.truncate(limit);
            prop_assert_eq!(result(&database,&sql,&[Value::Integer(a),Value::Integer(b),Value::Integer(limit as i64)]),expected);
        }
    }

    #[test]
    fn generated_range_conjuncts_preserve_three_valued_nullable_predicates(
        lower in -16i64..32,upper in -16i64..32,threshold in -16i64..32,
        choice in 0u8..3,limit in 0usize..25
    ) {
        let dir=tempfile::tempdir().unwrap();
        let input=(0..24).collect::<Vec<_>>();
        let database=initialized(&dir.path().join("db"),&input);
        let predicate=match choice {
            0=>"(n >= $3 OR n IS NULL)",
            1=>"(n >= $3 OR n = NULL)",
            _=>"NOT (n >= $3)",
        };
        let sql=format!("SELECT id FROM t WHERE id >= $1 AND id < $2 AND {predicate} ORDER BY id LIMIT $4");
        let expected=input.into_iter().filter(|key|*key>=lower && *key<upper)
            .filter(|key|match (choice,*key%3==0) {
                (0,true)=>true,
                (_,true)=>false,
                (0|1,false)=>*key>=threshold,
                _=>*key<threshold,
            }).take(limit).collect::<Vec<_>>();
        let parameters=[Value::Integer(lower),Value::Integer(upper),Value::Integer(threshold),Value::Integer(limit as i64)];
        prop_assert_eq!(explain(database.view().unwrap(),&sql,&parameters).unwrap().access,"primary_range");
        prop_assert_eq!(result(&database,&sql,&parameters),expected);
    }
}
