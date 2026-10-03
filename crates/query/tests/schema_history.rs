use emilybase_catalog::Value;
use emilybase_query::{execute, explain, query};
use emilybase_transactions::Database;

#[test]
fn nonfirst_table_constraint_key_drives_named_insert_lookup_update_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    execute(&mut db, "CREATE TABLE t(count INTEGER,name TEXT,note TEXT,PRIMARY KEY(name)); INSERT INTO t(note,name) VALUES('optional','key')", &[]).unwrap();
    assert_eq!(db.view().unwrap().schema("t").unwrap().primary_key, 1);
    assert!(!db.view().unwrap().schema("t").unwrap().columns[1].nullable);
    assert_eq!(
        query(db.view().unwrap(), "SELECT * FROM t WHERE name='key'", &[])
            .unwrap()
            .rows,
        [vec![
            Value::Null,
            Value::Text("key".into()),
            Value::Text("optional".into())
        ]]
    );
    let plan = explain(
        db.view().unwrap(),
        "SELECT * FROM t WHERE count IS NULL AND name=$1",
        &[Value::Text("key".into())],
    )
    .unwrap();
    assert_eq!(plan.access, "primary_key");
    let report = execute(
        &mut db,
        "UPDATE t SET count=7 WHERE name=$1 AND count IS NULL",
        &[Value::Text("key".into())],
    )
    .unwrap();
    assert_eq!(report.results[0].affected, 1);
    assert_eq!(
        execute(
            &mut db,
            "UPDATE t SET count=9 WHERE name='key' AND count IS NULL",
            &[]
        )
        .unwrap()
        .results[0]
            .affected,
        0
    );
    drop(db);
    let mut db = Database::open(&path).unwrap();
    assert_eq!(
        query(
            db.view().unwrap(),
            "SELECT count FROM t WHERE 'key'=name",
            &[]
        )
        .unwrap()
        .rows,
        [vec![Value::Integer(7)]]
    );
    let report = execute(
        &mut db,
        "DELETE FROM t WHERE name=$1",
        &[Value::Text("key".into())],
    )
    .unwrap();
    assert_eq!(report.results[0].affected, 1);
    assert_eq!(db.view().unwrap().row_count(), 0);
}

#[test]
fn rolled_back_or_failed_ddl_keeps_original_ids_and_committed_recreation_uses_fresh_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    execute(&mut db,"CREATE TABLE t(id INT PRIMARY KEY,title TEXT);INSERT INTO t VALUES(1,'old');CREATE TABLE keep(id INT PRIMARY KEY)",&[]).unwrap();
    let original = db.view().unwrap().table_id("t").unwrap();
    let retained = db.view().unwrap().table_id("keep").unwrap();
    let next = db.view().unwrap().next_table_id();
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    let rebuild = "DROP TABLE t;CREATE TABLE t(id TEXT PRIMARY KEY,title TEXT);INSERT INTO t VALUES('new','new')";
    let report = execute(&mut db, &format!("BEGIN;{rebuild};ROLLBACK"), &[]).unwrap();
    assert!(!report.committed);
    assert_eq!(db.view().unwrap().next_table_id(), next);
    assert_eq!(db.view().unwrap().table_id("t").unwrap(), original);
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    assert!(
        execute(
            &mut db,
            &format!("{rebuild};CREATE TABLE keep(id INT PRIMARY KEY)"),
            &[]
        )
        .is_err()
    );
    assert_eq!(db.view().unwrap().next_table_id(), next);
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    let before_transaction = db.last_transaction();
    execute(&mut db, rebuild, &[]).unwrap();
    let fresh = db.view().unwrap().table_id("t").unwrap();
    assert!(fresh > original && fresh > retained);
    assert_eq!(fresh, next);
    assert_eq!(db.last_transaction(), before_transaction + 1);
    db.compact().unwrap();
    drop(db);
    let db = Database::open(&path).unwrap();
    assert_eq!(db.view().unwrap().table_id("t").unwrap(), fresh);
    assert_eq!(db.view().unwrap().table_id("keep").unwrap(), retained);
    assert_eq!(
        query(db.view().unwrap(), "SELECT * FROM t", &[])
            .unwrap()
            .rows,
        [vec![Value::Text("new".into()), Value::Text("new".into())]]
    );
    assert!(query(db.view().unwrap(), "SELECT * FROM t WHERE id=1", &[]).is_err());
}
