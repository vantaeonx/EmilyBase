use emilybase_catalog::{MAX_VALUE_BYTES, Value};
use emilybase_query::{ExecutionError, execute, query};
use emilybase_transactions::Database;

fn rows(db: &Database, sql: &str, parameters: &[Value]) -> Vec<Vec<Value>> {
    query(db.view().unwrap(), sql, parameters).unwrap().rows
}

#[test]
fn every_catalog_type_orders_and_compares_without_implicit_coercion() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::create(dir.path().join("db")).unwrap();
    execute(&mut db,"CREATE TABLE t(id INT PRIMARY KEY,text TEXT,f FLOAT,b BOOLEAN,data BYTES); INSERT INTO t VALUES(3,'é',1.5,TRUE,X'ff'),(2,'e',0.0,FALSE,X'00'),(1,'é',-0.0,FALSE,X''),(4,NULL,NULL,NULL,NULL)",&[]).unwrap();
    assert_eq!(
        rows(&db, "SELECT id FROM t ORDER BY text", &[]),
        [
            vec![Value::Integer(2)],
            vec![Value::Integer(1)],
            vec![Value::Integer(3)],
            vec![Value::Integer(4)]
        ]
    );
    assert_eq!(
        rows(&db, "SELECT id FROM t ORDER BY f ASC,id DESC", &[]),
        [
            vec![Value::Integer(2)],
            vec![Value::Integer(1)],
            vec![Value::Integer(3)],
            vec![Value::Integer(4)]
        ]
    );
    assert_eq!(
        rows(&db, "SELECT id FROM t WHERE f=0.0 ORDER BY id", &[]),
        [vec![Value::Integer(1)], vec![Value::Integer(2)]]
    );
    assert_eq!(
        rows(&db, "SELECT id FROM t ORDER BY data DESC NULLS FIRST", &[]),
        [
            vec![Value::Integer(4)],
            vec![Value::Integer(3)],
            vec![Value::Integer(2)],
            vec![Value::Integer(1)]
        ]
    );
    assert_eq!(
        rows(&db, "SELECT id FROM t WHERE data > X'00'", &[]),
        [vec![Value::Integer(3)]]
    );
    assert_eq!(
        rows(
            &db,
            "SELECT id FROM t WHERE data = $1",
            &[Value::Bytes(vec![])]
        ),
        [vec![Value::Integer(1)]]
    );
    for sql in [
        "SELECT * FROM t WHERE f=0",
        "SELECT * FROM t WHERE id=1.0",
        "SELECT * FROM t WHERE b=0",
        "SELECT * FROM t WHERE data=''",
        "SELECT * FROM t WHERE text=TRUE",
    ] {
        assert!(matches!(
            query(db.view().unwrap(), sql, &[]),
            Err(ExecutionError::Type)
        ));
    }
    let before = db.last_transaction();
    execute(
        &mut db,
        "UPDATE t SET f=$1,b=FALSE,data=$2 WHERE id=3",
        &[Value::Float(f64::MAX), Value::Bytes(vec![255, 0])],
    )
    .unwrap();
    assert_eq!(db.last_transaction(), before + 1);
    let result = rows(&db, "SELECT f,data FROM t WHERE id=3", &[]);
    assert_eq!(
        result,
        [vec![Value::Float(f64::MAX), Value::Bytes(vec![255, 0])]]
    );
}

#[test]
fn long_existing_text_keys_remain_supported_independently_of_the_index_codec() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    execute(
        &mut db,
        "CREATE TABLE t(id TEXT PRIMARY KEY,value INT)",
        &[],
    )
    .unwrap();
    let keys = [
        "k".repeat(255),
        "k".repeat(256),
        "k".repeat(257),
        "я".repeat(MAX_VALUE_BYTES / 2),
    ];
    for (i, key) in keys.iter().enumerate() {
        execute(
            &mut db,
            "INSERT INTO t VALUES($1,$2)",
            &[Value::Text(key.clone()), Value::Integer(i as i64)],
        )
        .unwrap();
    }
    for (i, key) in keys.iter().enumerate() {
        assert_eq!(
            rows(
                &db,
                "SELECT value FROM t WHERE id=$1",
                &[Value::Text(key.clone())]
            ),
            [vec![Value::Integer(i as i64)]]
        );
    }
    execute(
        &mut db,
        "UPDATE t SET value=99 WHERE id=$1",
        &[Value::Text(keys[3].clone())],
    )
    .unwrap();
    db.compact().unwrap();
    drop(db);
    let mut db = Database::open(&path).unwrap();
    assert_eq!(
        rows(
            &db,
            "SELECT value FROM t WHERE id=$1",
            &[Value::Text(keys[3].clone())]
        ),
        [vec![Value::Integer(99)]]
    );
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    assert!(
        execute(
            &mut db,
            "INSERT INTO t VALUES($1,100)",
            &[Value::Text("k".repeat(MAX_VALUE_BYTES + 1))]
        )
        .is_err()
    );
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    execute(
        &mut db,
        "DELETE FROM t WHERE id=$1",
        &[Value::Text(keys[3].clone())],
    )
    .unwrap();
    assert_eq!(db.view().unwrap().row_count(), 3);
}

#[test]
fn parameter_and_row_encoding_failures_preserve_prior_commits_without_echoing_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    execute(
        &mut db,
        "CREATE TABLE t(id INT PRIMARY KEY,a TEXT NOT NULL,b BYTES)",
        &[],
    )
    .unwrap();
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    for parameter in [
        Value::Text("secret".repeat(600)),
        Value::Bytes(vec![255; MAX_VALUE_BYTES + 1]),
        Value::Float(f64::NAN),
        Value::Float(f64::INFINITY),
        Value::Float(f64::NEG_INFINITY),
    ] {
        let error = execute(
            &mut db,
            "INSERT INTO t(id,a) VALUES(1,'staged');SELECT * FROM t",
            &[parameter],
        )
        .unwrap_err();
        assert!(!error.to_string().contains("secret"));
        assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    }
    let result = execute(
        &mut db,
        "INSERT INTO t(id,a) VALUES(1,'staged');INSERT INTO t VALUES(2,$1,$2)",
        &[Value::Text("x".repeat(3000)), Value::Bytes(vec![0; 3000])],
    );
    assert!(result.is_err());
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    for sql in [
        "INSERT INTO t(id) VALUES(1)",
        "UPDATE t SET a=NULL WHERE FALSE",
        "INSERT INTO t(id,a,missing) VALUES(1,'a',1)",
    ] {
        assert!(execute(&mut db, sql, &[]).is_err());
    }
    assert_eq!(db.view().unwrap().row_count(), 0);
    let parameters = vec![Value::Null; 256];
    assert!(
        query(
            db.view().unwrap(),
            "SELECT id FROM t WHERE $256 IS NULL",
            &parameters
        )
        .is_ok()
    );
    assert!(matches!(
        query(
            db.view().unwrap(),
            "SELECT id FROM t WHERE $256 IS NULL",
            &parameters[..255]
        ),
        Err(ExecutionError::Binding(256))
    ));
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
}

#[test]
fn intermediate_row_cap_and_join_projection_metadata_are_verified_at_real_size() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::create(dir.path().join("db")).unwrap();
    execute(&mut db, "CREATE TABLE t(id INT PRIMARY KEY)", &[]).unwrap();
    let values = (0..101)
        .map(|i| format!("({i})"))
        .collect::<Vec<_>>()
        .join(",");
    execute(&mut db, &format!("INSERT INTO t VALUES {values}"), &[]).unwrap();
    let result = query(
        db.view().unwrap(),
        "SELECT * FROM t AS a JOIN t AS b ON TRUE ORDER BY a.id LIMIT 1",
        &[],
    );
    assert!(matches!(
        result,
        Err(ExecutionError::Limit("intermediate rows/bytes"))
    ));
    let result = query(
        db.view().unwrap(),
        "SELECT * FROM t AS a JOIN t AS b ON TRUE LIMIT 10000",
        &[],
    )
    .unwrap();
    assert_eq!(result.columns, ["a.id", "b.id"]);
    assert_eq!(result.rows.len(), 10000);
    assert_eq!(result.rows[0], [Value::Integer(0), Value::Integer(0)]);
    assert_eq!(result.rows[9999], [Value::Integer(99), Value::Integer(0)]);
    let zero = query(
        db.view().unwrap(),
        "SELECT a.id AS left_id,b.id AS right_id FROM t AS a JOIN t AS b ON TRUE LIMIT 0",
        &[],
    )
    .unwrap();
    assert_eq!(zero.columns, ["left_id", "right_id"]);
    assert!(zero.rows.is_empty());
}
