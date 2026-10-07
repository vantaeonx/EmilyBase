use emilybase_catalog::{Key, Value};
use emilybase_query::{ExecutionError, execute, query};
use emilybase_transactions::Database;

#[test]
fn actual_intermediate_and_shared_output_bounds_preserve_committed_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    execute(&mut db, "CREATE TABLE t(id INT PRIMARY KEY,text TEXT)", &[]).unwrap();
    let text = Value::Text("я".repeat(1536));
    for start in (0..2800).step_by(200) {
        let values = (start..start + 200)
            .map(|i| format!("({i},$1)"))
            .collect::<Vec<_>>()
            .join(",");
        execute(
            &mut db,
            &format!("INSERT INTO t VALUES {values}"),
            std::slice::from_ref(&text),
        )
        .unwrap();
    }
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    let tx = db.last_transaction();
    assert_eq!(
        query(db.view().unwrap(), "SELECT id FROM t LIMIT 1", &[])
            .unwrap()
            .rows,
        [vec![Value::Integer(0)]]
    );
    let result = execute(
        &mut db,
        "UPDATE t SET text='staged' WHERE id=1; SELECT id FROM t ORDER BY text,id LIMIT 2800",
        &[],
    );
    assert!(matches!(
        result,
        Err(ExecutionError::Limit("intermediate rows/bytes"))
    ));
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    let result = execute(
        &mut db,
        "UPDATE t SET text='staged' WHERE id=1; SELECT text FROM t LIMIT 700; SELECT text FROM t LIMIT 700; SELECT text FROM t LIMIT 700; SELECT text FROM t LIMIT 700",
        &[],
    );
    assert!(matches!(result, Err(ExecutionError::Limit("output bytes"))));
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    assert_eq!(db.last_transaction(), tx);
    assert_eq!(
        db.view()
            .unwrap()
            .get("t", &Key::Integer(1))
            .unwrap()
            .unwrap()[1],
        text
    );
    drop(db);
    let db = Database::open(&path).unwrap();
    assert_eq!(db.view().unwrap().row_count(), 2800);
    assert_eq!(db.last_transaction(), tx);
}

#[test]
fn whole_transaction_event_bound_counts_all_statements_and_row_mutations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    execute(&mut db, "CREATE TABLE t(id INT PRIMARY KEY,text TEXT)", &[]).unwrap();
    let values = (0..256)
        .map(|i| format!("({i},NULL)"))
        .collect::<Vec<_>>()
        .join(",");
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    assert!(
        execute(
            &mut db,
            &format!("INSERT INTO t VALUES {values};UPDATE t SET text='over' WHERE id=0"),
            &[]
        )
        .is_err()
    );
    assert_eq!(db.view().unwrap().row_count(), 0);
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    execute(&mut db, &format!("INSERT INTO t VALUES {values}"), &[]).unwrap();
    assert_eq!(db.view().unwrap().row_count(), 256);
    let report = execute(&mut db, "UPDATE t SET text='at bound'", &[]).unwrap();
    assert_eq!(report.results[0].affected, 256);
    let report = execute(&mut db, "DELETE FROM t", &[]).unwrap();
    assert_eq!(report.results[0].affected, 256);
    assert_eq!(db.view().unwrap().row_count(), 0);
}
