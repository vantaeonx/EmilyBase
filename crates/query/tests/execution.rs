use emilybase_catalog::{Key, Value};
use emilybase_query::{ExecutionError, execute, explain};
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::collections::BTreeMap;

fn setup() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::create(dir.path().join("db")).unwrap();
    execute(&mut db, "CREATE TABLE items(id INTEGER PRIMARY KEY,title TEXT,active BOOLEAN,price FLOAT,payload BYTES)", &[]).unwrap();
    (dir, db)
}
fn select(db: &mut Database, sql: &str) -> Vec<Vec<Value>> {
    execute(db, sql, &[]).unwrap().results.remove(0).rows
}

#[test]
fn complete_sql_crud_join_sort_and_checkpoint_survive_reopen_and_compaction() {
    let (dir, mut db) = setup();
    let script = "BEGIN; INSERT INTO items VALUES(2,'é',NULL,2.0,X'ff'),(1,'a',TRUE,1.0,X'00'),(3,NULL,FALSE,NULL,NULL); CREATE TABLE labels(id INT PRIMARY KEY,owner INT,title TEXT); INSERT INTO labels VALUES(9,1,'joined'),(8,2,'second'); UPDATE items SET title=$1 WHERE id=3; SELECT a.id,b.title AS label FROM items AS a JOIN labels AS b ON a.id=b.owner WHERE a.id>=1 ORDER BY a.id DESC LIMIT 1; COMMIT";
    let report = execute(&mut db, script, &[Value::Text("updated".into())]).unwrap();
    assert!(report.committed);
    assert_eq!(report.results[0].affected, 3);
    assert_eq!(report.results[3].affected, 1);
    assert_eq!(report.results[4].columns, ["id", "label"]);
    assert_eq!(
        report.results[4].rows,
        [vec![Value::Integer(2), Value::Text("second".into())]]
    );
    assert_eq!(
        select(&mut db, "SELECT id FROM items ORDER BY title ASC"),
        [
            vec![Value::Integer(1)],
            vec![Value::Integer(3)],
            vec![Value::Integer(2)]
        ]
    );
    execute(
        &mut db,
        "DELETE FROM items WHERE id=2; DROP TABLE labels",
        &[],
    )
    .unwrap();
    let transaction = db.last_transaction();
    db.checkpoint().unwrap();
    db.compact().unwrap();
    drop(db);
    let mut db = Database::open(dir.path().join("db")).unwrap();
    assert_eq!(db.last_transaction(), transaction);
    assert_eq!(
        select(&mut db, "SELECT id FROM items ORDER BY id DESC"),
        [vec![Value::Integer(3)], vec![Value::Integer(1)]]
    );
}

#[test]
fn null_truth_tables_comparison_and_sorting_are_explicit() {
    let (_dir, mut db) = setup();
    execute(
        &mut db,
        "INSERT INTO items(id,active) VALUES(0,FALSE),(1,TRUE),(2,NULL)",
        &[],
    )
    .unwrap();
    for (predicate, expected) in [
        ("active", vec![1]),
        ("NOT active", vec![0]),
        ("active = NULL", vec![]),
        ("active IS NULL", vec![2]),
        ("active IS NOT NULL", vec![0, 1]),
        ("active OR NULL", vec![1]),
        ("active AND NULL", vec![]),
        ("NOT (active AND NULL)", vec![0]),
        ("active OR NOT active", vec![0, 1]),
        ("NOT (active OR NULL)", vec![]),
    ] {
        assert_eq!(
            select(&mut db, &format!("SELECT id FROM items WHERE {predicate}")),
            expected
                .into_iter()
                .map(|i| vec![Value::Integer(i)])
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(
        select(&mut db, "SELECT id FROM items ORDER BY active DESC"),
        [
            vec![Value::Integer(1)],
            vec![Value::Integer(0)],
            vec![Value::Integer(2)]
        ]
    );
    assert_eq!(
        select(
            &mut db,
            "SELECT id FROM items ORDER BY active DESC NULLS FIRST"
        ),
        [
            vec![Value::Integer(2)],
            vec![Value::Integer(1)],
            vec![Value::Integer(0)]
        ]
    );
    assert_eq!(
        select(
            &mut db,
            "SELECT id FROM items WHERE id<2 OR id>=2 ORDER BY id LIMIT 2"
        ),
        [vec![Value::Integer(0)], vec![Value::Integer(1)]]
    );
}

#[test]
fn every_failed_script_or_rollback_keeps_exact_committed_wal_and_state() {
    let (dir, mut db) = setup();
    let before = std::fs::read(dir.path().join("db/redo.wal")).unwrap();
    let id = db.last_transaction();
    for tail in [
        "INSERT INTO items(id) VALUES(1)",
        "SELECT missing FROM items",
        "UPDATE items SET title=TRUE WHERE FALSE",
        "SELECT * FROM items WHERE id='wrong'",
        "SELECT * FROM items LIMIT $1",
        "SELECT * FROM items WHERE id=$1",
        "UPDATE items SET id=9",
        "INSERT INTO items(id,id) VALUES(2,2)",
        "UPDATE items SET title='a',title='b'",
        "SELECT * FROM items WHERE title",
        "SELECT * FROM items WHERE active=1",
    ] {
        let result = execute(
            &mut db,
            &format!("INSERT INTO items(id) VALUES(1); {tail}"),
            &[],
        );
        assert!(result.is_err(), "{tail}");
        assert_eq!(
            std::fs::read(dir.path().join("db/redo.wal")).unwrap(),
            before
        );
        assert_eq!(db.last_transaction(), id);
        assert_eq!(db.view().unwrap().row_count(), 0);
    }
    let report = execute(
        &mut db,
        "BEGIN; INSERT INTO items(id) VALUES(2); SELECT id FROM items; ROLLBACK",
        &[],
    )
    .unwrap();
    assert!(!report.committed);
    assert_eq!(report.results[1].rows, [vec![Value::Integer(2)]]);
    assert_eq!(
        std::fs::read(dir.path().join("db/redo.wal")).unwrap(),
        before
    );
    for sql in [
        "COMMIT",
        "ROLLBACK",
        "BEGIN",
        "BEGIN;SELECT * FROM items",
        "SELECT * FROM items; COMMIT",
        "BEGIN; BEGIN; COMMIT",
        "BEGIN;ROLLBACK;COMMIT",
    ] {
        assert!(matches!(
            execute(&mut db, sql, &[]),
            Err(ExecutionError::Control)
        ));
    }
}

#[test]
fn binding_injection_stays_data_and_resolution_runs_even_for_empty_or_zero_limit() {
    let (_dir, mut db) = setup();
    let text = "'; DROP TABLE items; -- secret";
    execute(
        &mut db,
        "INSERT INTO items(title,id) VALUES($1,$2)",
        &[Value::Text(text.into()), Value::Integer(7)],
    )
    .unwrap();
    let report = execute(
        &mut db,
        "SELECT title FROM items WHERE id=$1 LIMIT $2",
        &[Value::Integer(7), Value::Integer(1)],
    )
    .unwrap();
    assert_eq!(report.results[0].rows, [vec![Value::Text(text.into())]]);
    let error = execute(
        &mut db,
        "SELECT * FROM items WHERE id=$1",
        &[Value::Text(text.into())],
    )
    .unwrap_err();
    assert!(!error.to_string().contains("secret"));
    for sql in [
        "SELECT missing FROM items LIMIT 0",
        "SELECT * FROM items WHERE id='text' LIMIT 0",
        "SELECT * FROM items AS a JOIN items AS b ON id=id LIMIT 0",
        "SELECT * FROM items AS a JOIN items AS a ON TRUE LIMIT 0",
        "SELECT * FROM items AS a WHERE items.id=7",
    ] {
        assert!(execute(&mut db, sql, &[]).is_err(), "{sql}");
    }
    assert_eq!(
        explain(
            db.view().unwrap(),
            "SELECT * FROM items WHERE id=$1",
            &[Value::Integer(7)]
        )
        .unwrap()
        .access,
        "primary_key"
    );
    assert_eq!(
        explain(
            db.view().unwrap(),
            "SELECT * FROM items WHERE id=7 OR active",
            &[]
        )
        .unwrap()
        .access,
        "scan"
    );
    assert_eq!(
        explain(
            db.view().unwrap(),
            "SELECT * FROM items AS a JOIN items AS b ON a.id=b.id",
            &[]
        )
        .unwrap()
        .access,
        "primary_join"
    );
    assert!(execute(&mut db, "SELECT * FROM items", &[Value::Float(f64::NAN)]).is_err());
    assert!(execute(&mut db, "SELECT * FROM items", &vec![Value::Null; 257]).is_err());
    for value in [
        Value::Integer(-1),
        Value::Integer(10001),
        Value::Float(1.0),
        Value::Null,
    ] {
        assert!(execute(&mut db, "SELECT * FROM items LIMIT $1", &[value]).is_err());
    }
}

#[test]
fn excessive_join_work_and_multirow_writes_rollback_the_whole_script() {
    let (dir, mut db) = setup();
    for start in [0, 200] {
        let values = (start..start + 200)
            .map(|i| format!("({i})"))
            .collect::<Vec<_>>()
            .join(",");
        execute(
            &mut db,
            &format!("INSERT INTO items(id) VALUES {values}"),
            &[],
        )
        .unwrap();
    }
    let before = std::fs::read(dir.path().join("db/redo.wal")).unwrap();
    let result = execute(
        &mut db,
        "UPDATE items SET title='staged' WHERE id=1; SELECT a.id FROM items AS a JOIN items AS b ON FALSE",
        &[],
    );
    assert!(matches!(result, Err(ExecutionError::Limit("query work"))));
    assert_eq!(
        std::fs::read(dir.path().join("db/redo.wal")).unwrap(),
        before
    );
    assert_eq!(
        db.view()
            .unwrap()
            .get("items", &Key::Integer(1))
            .unwrap()
            .unwrap()[1],
        Value::Null
    );
    assert!(execute(&mut db, "UPDATE items SET title='too many'", &[]).is_err());
    assert_eq!(
        std::fs::read(dir.path().join("db/redo.wal")).unwrap(),
        before
    );
    assert_eq!(
        select(
            &mut db,
            "SELECT a.id FROM items AS a JOIN items AS b ON TRUE LIMIT 1"
        ),
        [vec![Value::Integer(0)]]
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_sql_batches_match_committed_map_after_reopen(
        batches in prop::collection::vec((any::<bool>(),prop::collection::vec((0u8..3,0i64..12,"[a-z' ;]{0,16}"),0..8)),1..10)
    ) {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("db");
        let mut db=Database::create(&path).unwrap();
        execute(&mut db,"CREATE TABLE t(id INT PRIMARY KEY,title TEXT)",&[]).unwrap();
        let mut model=BTreeMap::new();
        for (commit,operations) in batches {
            let before=std::fs::read(path.join("redo.wal")).unwrap();let mut staged=model.clone();let mut failed=false;
            let mut sql=String::from("BEGIN;");let mut parameters=Vec::new();
            for (kind,id,text) in operations {
                match kind {
                    0=>{parameters.push(Value::Text(text.clone()));sql+=&format!("INSERT INTO t VALUES({id},${});",parameters.len());if staged.insert(id,text).is_some(){failed=true;}}
                    1=>{parameters.push(Value::Text(text.clone()));sql+=&format!("UPDATE t SET title=${} WHERE id={id};",parameters.len());if let Some(v)=staged.get_mut(&id){*v=text;}}
                    _=>{sql+=&format!("DELETE FROM t WHERE id={id};");staged.remove(&id);}
                }
            }
            sql+=if commit {"COMMIT"}else{"ROLLBACK"};
            let result=execute(&mut db,&sql,&parameters);
            prop_assert_eq!(result.is_err(),failed);
            if !failed && commit {model=staged;}else{prop_assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(),before);}
            drop(db);db=Database::open(&path).unwrap();
            let actual=select(&mut db,"SELECT * FROM t ORDER BY id");
            let expected=model.iter().map(|(id,text)|vec![Value::Integer(*id),Value::Text(text.clone())]).collect::<Vec<_>>();
            prop_assert_eq!(actual,expected);
        }
    }
}
