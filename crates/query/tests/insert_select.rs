use emilybase_catalog::{Key, Value};
use emilybase_query::{ExecutionError, ast::Statement, execute, parse, query};
use emilybase_transactions::Database;
use proptest::prelude::*;

fn fixture() -> (tempfile::TempDir, Database) {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::create(directory.path().join("db")).unwrap();
    (directory, database)
}

#[test]
fn parser_distinguishes_values_and_select_without_accepting_new_statement_controls() {
    for sql in [
        "INSERT INTO d SELECT * FROM s",
        "INSERT INTO d(a,b) SELECT x,y FROM s WHERE x>=$1 ORDER BY y DESC LIMIT $2",
        "INSERT INTO d SELECT a.id,b.name FROM s AS a JOIN names AS b ON a.id=b.id",
    ] {
        assert!(matches!(
            &parse(sql).unwrap()[0],
            Statement::InsertSelect { .. }
        ));
    }
    assert!(matches!(
        &parse("INSERT INTO d VALUES(1)").unwrap()[0],
        Statement::Insert { .. }
    ));
    for sql in [
        "INSERT INTO d",
        "INSERT INTO d SELECT",
        "INSERT INTO d SELECT *",
        "INSERT INTO d VALUES SELECT * FROM s",
        "INSERT INTO d SELECT * FROM s VALUES(1)",
    ] {
        assert!(parse(sql).is_err(), "{sql}");
    }
}

#[test]
fn typed_copy_reorders_target_columns_preserves_bits_and_staged_sources_on_both_wals() {
    for format in [1, 2] {
        let (directory, mut database) = fixture();
        if format == 2 {
            database.compact().unwrap();
        }
        execute(&mut database,"CREATE TABLE src(id INT PRIMARY KEY,txt TEXT,b BOOL,f FLOAT,payload BYTES); CREATE TABLE dst(note TEXT,id INT PRIMARY KEY,txt TEXT,b BOOL,f FLOAT,payload BYTES)",&[]).unwrap();
        let parameters = [
            Value::Integer(i64::MIN),
            Value::Text("界\0'; DROP TABLE src; --".into()),
            Value::Boolean(true),
            Value::Float(-0.0),
            Value::Bytes(vec![0, 255, 42]),
        ];
        let report=execute(&mut database,"INSERT INTO src VALUES($1,$2,$3,$4,$5); INSERT INTO dst(id,txt,b,f,payload) SELECT id,txt,b,f,payload FROM src WHERE id=$1 ORDER BY id DESC LIMIT 1; SELECT * FROM dst",&parameters).unwrap();
        assert_eq!(report.results[1].affected, 1);
        assert!(report.results[1].columns.is_empty());
        assert!(report.results[1].rows.is_empty());
        assert_eq!(report.results[2].rows[0][0], Value::Null);
        assert_eq!(&report.results[2].rows[0][1..], &parameters);
        let Value::Float(value) = report.results[2].rows[0][4] else {
            panic!()
        };
        assert_eq!(value.to_bits(), (-0.0f64).to_bits());
        let transaction = report.transaction;
        drop(database);
        let database = Database::open(directory.path().join("db")).unwrap();
        assert_eq!(database.last_transaction(), transaction);
        let row = database
            .view()
            .unwrap()
            .get("dst", &Key::Integer(i64::MIN))
            .unwrap()
            .unwrap();
        let Value::Float(value) = row[4] else {
            panic!()
        };
        assert_eq!(value.to_bits(), (-0.0f64).to_bits());
        assert_eq!(row[2], parameters[1]);
    }
}

#[test]
fn target_types_width_and_bindings_resolve_even_for_empty_sources_and_limit_zero() {
    let (_directory, mut database) = fixture();
    execute(&mut database,"CREATE TABLE src(id INT PRIMARY KEY,n INT,txt TEXT); CREATE TABLE dst(id INT PRIMARY KEY,txt TEXT)",&[]).unwrap();
    let before = database.committed_wal().unwrap();
    for tail in [
        "INSERT INTO dst SELECT id FROM src",
        "INSERT INTO dst SELECT id,n FROM src LIMIT 0",
        "INSERT INTO dst(id,id) SELECT id,n FROM src LIMIT 0",
        "INSERT INTO dst(id,absent) SELECT id,txt FROM src",
        "INSERT INTO dst SELECT id,missing FROM src LIMIT 0",
        "INSERT INTO dst SELECT id,txt FROM src WHERE id=$1 LIMIT 0",
        "INSERT INTO absent SELECT id,txt FROM src LIMIT 0",
    ] {
        assert!(
            execute(
                &mut database,
                &format!("CREATE TABLE prefix(id INT PRIMARY KEY); {tail}"),
                &[]
            )
            .is_err(),
            "{tail}"
        );
        assert_eq!(database.committed_wal().unwrap(), before);
        assert!(database.view().unwrap().schema("prefix").is_err());
    }
    let base = database.last_transaction();
    let report = execute(
        &mut database,
        "INSERT INTO dst SELECT id,txt FROM src LIMIT 0",
        &[],
    )
    .unwrap();
    assert_eq!(report.results[0].affected, 0);
    assert_eq!(report.transaction, base);
    assert_eq!(database.committed_wal().unwrap(), before);
}

#[test]
fn source_selection_finishes_before_self_insertion_and_late_conflicts_abort_everything() {
    let (_directory, mut database) = fixture();
    execute(
        &mut database,
        "CREATE TABLE t(id INT PRIMARY KEY,peer INT); INSERT INTO t VALUES(1,3),(2,4)",
        &[],
    )
    .unwrap();
    let report = execute(
        &mut database,
        "INSERT INTO t(id,peer) SELECT peer,id FROM t ORDER BY id",
        &[],
    )
    .unwrap();
    assert_eq!(report.results[0].affected, 2);
    assert_eq!(
        query(database.view().unwrap(), "SELECT * FROM t ORDER BY id", &[])
            .unwrap()
            .rows,
        vec![
            vec![Value::Integer(1), Value::Integer(3)],
            vec![Value::Integer(2), Value::Integer(4)],
            vec![Value::Integer(3), Value::Integer(1)],
            vec![Value::Integer(4), Value::Integer(2)]
        ]
    );
    execute(
        &mut database,
        "CREATE TABLE bad(id INT PRIMARY KEY,peer INT); INSERT INTO bad VALUES(1,3),(2,2)",
        &[],
    )
    .unwrap();
    let before = database.committed_wal().unwrap();
    assert!(execute(&mut database,"UPDATE t SET peer=99 WHERE id=1; INSERT INTO bad(id,peer) SELECT peer,id FROM bad ORDER BY id",&[]).is_err());
    assert_eq!(database.committed_wal().unwrap(), before);
    assert!(
        database
            .view()
            .unwrap()
            .get("bad", &Key::Integer(3))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        database
            .view()
            .unwrap()
            .get("t", &Key::Integer(1))
            .unwrap()
            .unwrap()[1],
        Value::Integer(3)
    );
}

#[test]
fn joins_keep_filter_order_limit_and_duplicate_or_null_key_refusals_atomic() {
    let (_directory, mut database) = fixture();
    execute(&mut database,"CREATE TABLE src(id INT PRIMARY KEY); INSERT INTO src VALUES(1),(2),(3); CREATE TABLE labels(id INT PRIMARY KEY,owner INT,title TEXT); INSERT INTO labels VALUES(11,1,'a'),(12,2,'b'),(13,3,NULL); CREATE TABLE dst(id INT PRIMARY KEY,title TEXT)",&[]).unwrap();
    let report=execute(&mut database,"INSERT INTO dst SELECT a.id,b.title FROM src AS a JOIN labels AS b ON a.id=b.owner WHERE b.title IS NOT NULL ORDER BY a.id DESC LIMIT $1",&[Value::Integer(1)]).unwrap();
    assert_eq!(report.results[0].affected, 1);
    assert!(
        database
            .view()
            .unwrap()
            .get("dst", &Key::Integer(2))
            .unwrap()
            .is_some()
    );
    execute(&mut database,"INSERT INTO labels VALUES(14,1,'duplicate'); CREATE TABLE empty(id INT PRIMARY KEY,title TEXT)",&[]).unwrap();
    let before = database.committed_wal().unwrap();
    for sql in [
        "UPDATE dst SET title='partial'; INSERT INTO empty SELECT a.id,b.title FROM src AS a JOIN labels AS b ON a.id=b.owner ORDER BY a.id,b.id",
        "INSERT INTO empty SELECT owner,title FROM labels WHERE title IS NULL; INSERT INTO empty SELECT owner,title FROM labels WHERE id=13",
    ] {
        assert!(execute(&mut database, sql, &[]).is_err());
        assert_eq!(database.committed_wal().unwrap(), before);
    }
    execute(
        &mut database,
        "INSERT INTO labels VALUES(15,NULL,'null key')",
        &[],
    )
    .unwrap();
    let before = database.committed_wal().unwrap();
    assert!(
        execute(
            &mut database,
            "INSERT INTO empty SELECT owner,title FROM labels WHERE id=15",
            &[]
        )
        .is_err()
    );
    assert_eq!(database.committed_wal().unwrap(), before);
    assert_eq!(database.view().unwrap().scan("empty", 10).unwrap().len(), 0);
}

#[test]
fn copy_capacity_is_shared_with_prior_statements_and_refuses_instead_of_truncating() {
    let (_directory, mut database) = fixture();
    execute(
        &mut database,
        "CREATE TABLE src(id INT PRIMARY KEY); CREATE TABLE dst(id INT PRIMARY KEY)",
        &[],
    )
    .unwrap();
    for start in [0, 200] {
        let sql = format!(
            "INSERT INTO src VALUES {}",
            (start..(start + 200).min(260))
                .map(|id| format!("({id})"))
                .collect::<Vec<_>>()
                .join(",")
        );
        execute(&mut database, &sql, &[]).unwrap();
    }
    let before = database.committed_wal().unwrap();
    for sql in [
        "INSERT INTO dst SELECT * FROM src",
        "CREATE TABLE prefix(id INT PRIMARY KEY); INSERT INTO dst SELECT * FROM src LIMIT 256",
    ] {
        assert!(matches!(
            execute(&mut database, sql, &[]),
            Err(ExecutionError::Transaction(
                emilybase_transactions::Error::Limit
            ))
        ));
        assert_eq!(database.committed_wal().unwrap(), before);
        assert!(database.view().unwrap().schema("prefix").is_err());
    }
    let report = execute(
        &mut database,
        "INSERT INTO dst SELECT * FROM src ORDER BY id DESC LIMIT 256",
        &[],
    )
    .unwrap();
    assert_eq!(report.results[0].affected, 256);
    assert_eq!(
        database.view().unwrap().scan("dst", 1000).unwrap().len(),
        256
    );
    assert!(
        database
            .view()
            .unwrap()
            .get("dst", &Key::Integer(3))
            .unwrap()
            .is_none()
    );
    assert!(
        database
            .view()
            .unwrap()
            .get("dst", &Key::Integer(4))
            .unwrap()
            .is_some()
    );
}

#[test]
fn rebuild_with_nullable_field_rolls_back_or_publishes_the_whole_replacement() {
    let (_directory, mut database) = fixture();
    execute(
        &mut database,
        "CREATE TABLE t(id INT PRIMARY KEY,title TEXT); INSERT INTO t VALUES(1,'a'),(2,'b')",
        &[],
    )
    .unwrap();
    let old = database.view().unwrap().clone();
    let before = database.committed_wal().unwrap();
    let script = "CREATE TABLE replacement(id INT PRIMARY KEY,title TEXT,note TEXT); INSERT INTO replacement(id,title) SELECT * FROM t; DROP TABLE t; CREATE TABLE t(id INT PRIMARY KEY,title TEXT,note TEXT); INSERT INTO t SELECT * FROM replacement; DROP TABLE replacement; SELECT * FROM t ORDER BY id";
    let report = execute(&mut database, &format!("BEGIN; {script}; ROLLBACK"), &[]).unwrap();
    assert!(!report.committed);
    assert_eq!(report.results[6].rows[0][2], Value::Null);
    assert_eq!(database.committed_wal().unwrap(), before);
    let base = database.last_transaction();
    let report = execute(&mut database, script, &[]).unwrap();
    assert_eq!(report.transaction, base + 1);
    assert_eq!(
        database.view().unwrap().schema("t").unwrap().columns.len(),
        3
    );
    assert!(database.view().unwrap().schema("replacement").is_err());
    assert_eq!(old.schema("t").unwrap().columns.len(), 2);
    assert_eq!(old.get("t", &Key::Integer(1)).unwrap().unwrap().len(), 2);
}

#[test]
fn copy_query_work_and_materialized_output_keep_whole_script_budgets() {
    let (_directory, mut database) = fixture();
    execute(
        &mut database,
        "CREATE TABLE src(id INT PRIMARY KEY,txt TEXT); CREATE TABLE dst(id INT PRIMARY KEY)",
        &[],
    )
    .unwrap();
    for start in [0, 200] {
        let sql = format!(
            "INSERT INTO src VALUES {}",
            (start..start + 200)
                .map(|id| format!("({id},$1)"))
                .collect::<Vec<_>>()
                .join(",")
        );
        execute(&mut database, &sql, &[Value::Text("x".repeat(3072))]).unwrap();
    }
    let before = database.committed_wal().unwrap();
    assert!(matches!(
        execute(
            &mut database,
            "CREATE TABLE prefix(id INT PRIMARY KEY); INSERT INTO dst SELECT a.id FROM src AS a JOIN src AS b ON FALSE",
            &[]
        ),
        Err(ExecutionError::Limit("query work"))
    ));
    assert_eq!(database.committed_wal().unwrap(), before);
    let columns = (0..63)
        .map(|n| format!("c{n} TEXT"))
        .collect::<Vec<_>>()
        .join(",");
    let projection = ["txt"; 63].join(",");
    let sql = format!(
        "CREATE TABLE wide(id INT PRIMARY KEY,{columns}); INSERT INTO wide SELECT id,{projection} FROM src LIMIT 100"
    );
    assert!(matches!(
        execute(&mut database, &sql, &[]),
        Err(ExecutionError::Limit("output bytes"))
    ));
    assert_eq!(database.committed_wal().unwrap(), before);
    assert!(database.view().unwrap().schema("wide").is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_filter_sort_limit_copies_match_an_independent_target_model(
        numbers in prop::collection::vec(-20i64..20,0..64),lower in 0i64..70,upper in 0i64..70,limit in 0usize..66,
    ) {
        let (directory,mut database)=fixture();execute(&mut database,"CREATE TABLE src(id INT PRIMARY KEY,n INT); CREATE TABLE dst(id INT PRIMARY KEY,n INT)",&[]).unwrap();
        if !numbers.is_empty() {
            let sql=format!("INSERT INTO src VALUES {}",numbers.iter().enumerate().map(|(id,n)|format!("({id},{n})")).collect::<Vec<_>>().join(","));execute(&mut database,&sql,&[]).unwrap();
        }
        let mut expected=numbers.iter().enumerate().filter(|(id,_)| *id as i64>=lower && (*id as i64)<upper).map(|(id,n)|(id as i64,*n)).collect::<Vec<_>>();
        expected.sort_by(|(a,an),(b,bn)|bn.cmp(an).then(a.cmp(b)));expected.truncate(limit);expected.sort();
        let sql="INSERT INTO dst SELECT id,n FROM src WHERE id>=$1 AND id<$2 ORDER BY n DESC,id LIMIT $3";
        let bindings=[Value::Integer(lower),Value::Integer(upper),Value::Integer(limit as i64)];
        let report=execute(&mut database,sql,&bindings).unwrap();prop_assert_eq!(report.results[0].affected,expected.len());
        let before=database.committed_wal().unwrap();let repeated=execute(&mut database,sql,&bindings);prop_assert_eq!(repeated.is_err(),!expected.is_empty());prop_assert_eq!(database.committed_wal().unwrap(),before);
        drop(database);let database=Database::open(directory.path().join("db")).unwrap();
        let actual=query(database.view().unwrap(),"SELECT * FROM dst ORDER BY id",&[]).unwrap().rows;
        let expected=expected.into_iter().map(|(id,n)|vec![Value::Integer(id),Value::Integer(n)]).collect::<Vec<_>>();prop_assert_eq!(actual,expected);
        prop_assert_eq!(database.view().unwrap().scan("src",1000).unwrap().len(),numbers.len());
    }
}

#[test]
fn long_text_primary_keys_and_embedded_nul_copy_through_checked_ranges_exactly() {
    let (directory, mut database) = fixture();
    execute(&mut database,"CREATE TABLE src(id TEXT PRIMARY KEY,v TEXT); CREATE TABLE dst(v TEXT,id TEXT PRIMARY KEY)",&[]).unwrap();
    let long = format!("a{}zz", "界".repeat(1023));
    assert_eq!(long.len(), 3072);
    for (index, key) in ["", "\0", "a", "a\0", long.as_str()]
        .into_iter()
        .enumerate()
    {
        execute(
            &mut database,
            "INSERT INTO src VALUES($1,$2)",
            &[Value::Text(key.into()), Value::Text(format!("row-{index}"))],
        )
        .unwrap();
    }
    let report = execute(
        &mut database,
        "INSERT INTO dst SELECT v,id FROM src WHERE id >= $1 AND id <= $2 ORDER BY id DESC LIMIT 2",
        &[Value::Text("a".into()), Value::Text(long.clone())],
    )
    .unwrap();
    assert_eq!(report.results[0].affected, 2);
    drop(database);
    let database = Database::open(directory.path().join("db")).unwrap();
    assert_eq!(
        database
            .view()
            .unwrap()
            .get("dst", &Key::Text(long.clone()))
            .unwrap()
            .unwrap(),
        &vec![Value::Text("row-4".into()), Value::Text(long)]
    );
    assert!(
        database
            .view()
            .unwrap()
            .get("dst", &Key::Text("a\0".into()))
            .unwrap()
            .is_some()
    );
    assert!(
        database
            .view()
            .unwrap()
            .get("dst", &Key::Text("a".into()))
            .unwrap()
            .is_none()
    );
}
