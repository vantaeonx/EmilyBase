use emilybase_catalog::{Key, Value};
use emilybase_query::{ExecutionError, execute, query, stage};
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::collections::BTreeMap;

fn fixture(version: u16) -> (tempfile::TempDir, Database) {
    let directory = tempfile::tempdir().unwrap();
    let mut database = Database::create(directory.path().join("db")).unwrap();
    execute(
        &mut database,
        "CREATE TABLE t(id INT PRIMARY KEY,value TEXT); INSERT INTO t VALUES(0,'original'); \
         CREATE TABLE receipts(id INT PRIMARY KEY)",
        &[],
    )
    .unwrap();
    if version == 2 {
        database.compact().unwrap();
    }
    (directory, database)
}

fn rows(database: &Database) -> Vec<Vec<Value>> {
    query(database.view().unwrap(), "SELECT * FROM t ORDER BY id", &[])
        .unwrap()
        .rows
}

fn original() -> Vec<Vec<Value>> {
    vec![vec![Value::Integer(0), Value::Text("original".into())]]
}

#[test]
fn typed_prefix_sql_and_typed_receipt_publish_in_one_durable_commit() {
    for version in [1, 2] {
        let (directory, mut database) = fixture(version);
        let old = database.view().unwrap().clone();
        let before = database.committed_wal().unwrap();
        let base = database.last_transaction();
        let mut transaction = database.begin().unwrap();
        transaction
            .insert("t", vec![Value::Integer(1), Value::Text("prefix".into())])
            .unwrap();
        let sql = "UPDATE t SET value=$1 WHERE id=0; CREATE TABLE next(id INT PRIMARY KEY); \
                   INSERT INTO next VALUES(9); SELECT * FROM t ORDER BY id";
        let staged = stage(transaction, sql, &[Value::Text("SQL".into())]).unwrap();
        assert_eq!(staged.results().len(), 4);
        assert_eq!(staged.results()[0].affected, 1);
        assert_eq!(staged.results()[3].rows.len(), 2);
        // Staging has not written a byte to the authoritative log.
        assert_eq!(
            std::fs::read(directory.path().join("db/redo.wal")).unwrap(),
            before
        );
        assert_eq!(
            query(&old, "SELECT * FROM t ORDER BY id", &[])
                .unwrap()
                .rows,
            original()
        );
        let (mut transaction, results) = staged.into_parts();
        assert_eq!(results[3].rows[0][1], Value::Text("SQL".into()));
        transaction
            .insert("receipts", vec![Value::Integer(1)])
            .unwrap();
        assert_eq!(transaction.commit().unwrap(), base + 1);
        assert_eq!(database.last_transaction(), base + 1);
        database.checkpoint().unwrap();
        drop(database);
        let mut database = Database::open(directory.path().join("db")).unwrap();
        assert_eq!(database.last_transaction(), base + 1);
        assert_eq!(rows(&database).len(), 2);
        assert_eq!(rows(&database)[0][1], Value::Text("SQL".into()));
        assert!(
            database
                .view()
                .unwrap()
                .get("next", &Key::Integer(9))
                .unwrap()
                .is_some()
        );
        assert!(
            database
                .view()
                .unwrap()
                .get("receipts", &Key::Integer(1))
                .unwrap()
                .is_some()
        );
        let archive = directory.path().join("synthetic.backup");
        emilybase_backup::create(&mut database, &archive).unwrap();
        let restored = directory.path().join("restored");
        emilybase_backup::restore(&archive, &restored).unwrap();
        let copy = Database::open(restored).unwrap();
        assert_eq!(rows(&copy), rows(&database));
        assert_eq!(copy.last_transaction(), base + 1);
        assert_eq!(copy.view().unwrap().scan("receipts", 128).unwrap().len(), 1);
    }
}

#[test]
fn every_error_discards_sql_and_the_callers_preceding_typed_writes() {
    for version in [1, 2] {
        let (directory, mut database) = fixture(version);
        let before = database.committed_wal().unwrap();
        let base = database.last_transaction();
        for tail in [
            "INSERT INTO t VALUES(0,'duplicate')",
            "SELECT missing FROM t",
            "SELECT * FROM t WHERE id='wrong type'",
            "UPDATE t SET value=FALSE WHERE FALSE",
            "UPDATE t SET id=9",
            "SELECT * FROM t LIMIT $1",
            "INSERT INTO t VALUES(3,$1)",
            "CREATE TABLE t(id INT PRIMARY KEY)",
            "DROP TABLE missing",
        ] {
            let mut transaction = database.begin().unwrap();
            transaction
                .insert("receipts", vec![Value::Integer(1)])
                .unwrap();
            let sql = format!("INSERT INTO t VALUES(2,'before error'); {tail}");
            assert!(stage(transaction, &sql, &[]).is_err(), "{tail}");
            assert_eq!(database.last_transaction(), base);
            assert_eq!(database.committed_wal().unwrap(), before);
            assert_eq!(rows(&database), original());
            assert_eq!(
                database
                    .view()
                    .unwrap()
                    .scan("receipts", 128)
                    .unwrap()
                    .len(),
                0
            );
        }
        drop(database);
        let database = Database::open(directory.path().join("db")).unwrap();
        assert_eq!(rows(&database), original());
        assert_eq!(database.last_transaction(), base);
    }
}

#[test]
fn syntax_controls_and_invalid_bindings_consume_the_transaction_before_execution() {
    let (_directory, mut database) = fixture(1);
    let before = database.committed_wal().unwrap();
    for sql in [
        "",
        ";;;",
        "BEGIN; INSERT INTO t VALUES(2,'bad'); COMMIT",
        "BEGIN; INSERT INTO t VALUES(2,'bad'); ROLLBACK",
        "COMMIT",
        "ROLLBACK",
        "SELECT * FROM t; COMMIT",
        "INSERT INTO t VALUES(2,'bad'); malformed syntax",
    ] {
        let mut transaction = database.begin().unwrap();
        transaction
            .insert("receipts", vec![Value::Integer(1)])
            .unwrap();
        assert!(stage(transaction, sql, &[]).is_err(), "{sql}");
        assert_eq!(database.committed_wal().unwrap(), before);
    }
    for parameters in [
        vec![Value::Null; 257],
        vec![Value::Float(f64::NAN)],
        vec![Value::Float(f64::INFINITY)],
        vec![Value::Text("a".repeat(3073))],
        vec![Value::Bytes(vec![0; 3073])],
    ] {
        let mut transaction = database.begin().unwrap();
        transaction
            .insert("receipts", vec![Value::Integer(1)])
            .unwrap();
        assert!(stage(transaction, "SELECT * FROM t", &parameters).is_err());
        assert_eq!(database.committed_wal().unwrap(), before);
    }
    assert_eq!(rows(&database), original());
    assert_eq!(
        database
            .view()
            .unwrap()
            .scan("receipts", 128)
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn drop_rollback_and_failed_receipt_cannot_publish_a_successful_script() {
    for version in [1, 2] {
        let (_directory, mut database) = fixture(version);
        let before = database.committed_wal().unwrap();
        let sql = "INSERT INTO t VALUES(1,'staged'); DROP TABLE receipts";
        let staged = stage(database.begin().unwrap(), sql, &[]).unwrap();
        assert_eq!(staged.results()[0].affected, 1);
        drop(staged);
        assert_eq!(database.committed_wal().unwrap(), before);
        let (transaction, _) = stage(database.begin().unwrap(), sql, &[])
            .unwrap()
            .into_parts();
        transaction.rollback();
        assert_eq!(database.committed_wal().unwrap(), before);
        let (mut transaction, _) = stage(database.begin().unwrap(), sql, &[])
            .unwrap()
            .into_parts();
        assert!(
            transaction
                .insert("receipts", vec![Value::Integer(1)])
                .is_err()
        );
        assert!(matches!(
            transaction.commit(),
            Err(emilybase_transactions::Error::Aborted)
        ));
        assert_eq!(database.committed_wal().unwrap(), before);
        assert_eq!(rows(&database), original());
        assert!(database.view().unwrap().schema("receipts").is_ok());
    }
}

#[test]
fn composed_calls_share_event_capacity_and_last_write_failure_aborts_everything() {
    let (_directory, mut database) = fixture(1);
    let base = database.last_transaction();
    let before = database.committed_wal().unwrap();
    for fill in [255, 256] {
        let mut transaction = database.begin().unwrap();
        transaction
            .insert("receipts", vec![Value::Integer(1)])
            .unwrap();
        let sql = format!(
            "INSERT INTO t VALUES {}",
            (1..=fill)
                .map(|id| format!("({id},'staged')"))
                .collect::<Vec<_>>()
                .join(",")
        );
        let result = stage(transaction, &sql, &[]);
        if fill == 255 {
            let (transaction, results) = result.unwrap().into_parts();
            assert_eq!(results[0].affected, 255);
            assert_eq!(transaction.remaining_events().unwrap(), 0);
            // Read-only SQL is permitted even when the event budget is exhausted.
            let staged = stage(
                transaction,
                "SELECT id FROM t ORDER BY id DESC LIMIT 1",
                &[],
            )
            .unwrap();
            assert_eq!(staged.results()[0].rows, vec![vec![Value::Integer(255)]]);
            let (mut transaction, _) = staged.into_parts();
            assert!(
                transaction
                    .insert("receipts", vec![Value::Integer(2)])
                    .is_err()
            );
            assert!(matches!(
                transaction.commit(),
                Err(emilybase_transactions::Error::Aborted)
            ));
        } else {
            assert!(result.is_err());
        }
        assert_eq!(database.committed_wal().unwrap(), before);
        assert_eq!(database.last_transaction(), base);
        assert_eq!(rows(&database), original());
    }
}

#[test]
fn next_stage_observes_prior_sql_and_a_later_planning_error_discards_both() {
    let (_directory, mut database) = fixture(1);
    let before = database.committed_wal().unwrap();
    let (transaction, _) = stage(
        database.begin().unwrap(),
        "CREATE TABLE next(id INT PRIMARY KEY); INSERT INTO next VALUES(8)",
        &[],
    )
    .unwrap()
    .into_parts();
    let staged = stage(
        transaction,
        "SELECT id FROM next; UPDATE t SET value='changed'",
        &[],
    )
    .unwrap();
    assert_eq!(staged.results()[0].rows, vec![vec![Value::Integer(8)]]);
    let (transaction, _) = staged.into_parts();
    assert!(matches!(
        stage(transaction, "SELECT missing FROM next", &[]),
        Err(ExecutionError::Column)
    ));
    assert_eq!(database.committed_wal().unwrap(), before);
    assert!(database.view().unwrap().schema("next").is_err());
    assert_eq!(rows(&database), original());
}

#[test]
fn read_only_scripts_preserve_noop_commit_and_can_finish_a_typed_prefix() {
    let (_directory, mut database) = fixture(1);
    let before = database.committed_wal().unwrap();
    let base = database.last_transaction();
    for sql in ["SELECT * FROM t", "SELECT id FROM t LIMIT 0"] {
        let (transaction, _) = stage(database.begin().unwrap(), sql, &[])
            .unwrap()
            .into_parts();
        assert_eq!(transaction.commit().unwrap(), base);
        assert_eq!(database.committed_wal().unwrap(), before);
    }
    let mut transaction = database.begin().unwrap();
    transaction
        .insert("receipts", vec![Value::Integer(1)])
        .unwrap();
    let (transaction, _) = stage(transaction, "SELECT * FROM t", &[])
        .unwrap()
        .into_parts();
    assert_eq!(transaction.commit().unwrap(), base + 1);
}

#[test]
fn an_already_aborted_transaction_is_never_returned_as_success() {
    let (_directory, mut database) = fixture(1);
    let before = database.committed_wal().unwrap();
    for sql in ["SELECT * FROM t", "INSERT INTO t VALUES(2,'unused')"] {
        let mut transaction = database.begin().unwrap();
        assert!(
            transaction
                .insert("t", vec![Value::Integer(0), Value::Null])
                .is_err()
        );
        assert!(matches!(
            stage(transaction, sql, &[]),
            Err(ExecutionError::Transaction(
                emilybase_transactions::Error::Aborted
            ))
        ));
        assert_eq!(database.committed_wal().unwrap(), before);
    }
}

#[test]
fn query_work_and_output_budgets_still_abort_the_typed_prefix() {
    let (_directory, mut database) = fixture(1);
    let payload = [Value::Text("я".repeat(1536))];
    for start in [1, 201] {
        let sql = format!(
            "INSERT INTO t VALUES {}",
            (start..start + 200)
                .map(|id| format!("({id},$1)"))
                .collect::<Vec<_>>()
                .join(",")
        );
        execute(&mut database, &sql, &payload).unwrap();
    }
    let before = database.committed_wal().unwrap();
    for (sql, expected) in [
        (
            "SELECT a.id FROM t AS a JOIN t AS b ON FALSE".to_string(),
            "query work",
        ),
        ("SELECT value FROM t;".repeat(8), "output bytes"),
    ] {
        let mut transaction = database.begin().unwrap();
        transaction
            .insert("receipts", vec![Value::Integer(1)])
            .unwrap();
        assert!(
            matches!(stage(transaction, &sql, &[]), Err(ExecutionError::Limit(limit)) if limit == expected)
        );
        assert_eq!(database.committed_wal().unwrap(), before);
        assert_eq!(
            database
                .view()
                .unwrap()
                .scan("receipts", 128)
                .unwrap()
                .len(),
            0
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_composed_sql_matches_an_independent_committed_model(
        batches in prop::collection::vec((any::<bool>(), 0u8..4,
            prop::collection::vec((0u8..3,1i64..12,"[a-z' ;]{0,12}"),0..8)),1..10),
        compact in any::<bool>(),
    ) {
        let (directory, mut database) = fixture(if compact { 2 } else { 1 });
        let mut model = BTreeMap::from([(0i64, "original".to_string())]);
        let mut receipts = Vec::new();
        for (index,(commit, fault, operations)) in batches.into_iter().enumerate() {
            let before = database.committed_wal().unwrap();
            let mut expected = model.clone();
            let mut failed = false;
            let mut sql = String::new();
            let mut parameters = Vec::new();
            let mut transaction = database.begin().unwrap();
            transaction.insert("receipts", vec![Value::Integer(index as i64)]).unwrap();
            for (kind, id, text) in operations {
                match kind {
                    0 => {
                        parameters.push(Value::Text(text.clone()));
                        sql += &format!("INSERT INTO t VALUES({id},${});", parameters.len());
                        if expected.insert(id,text).is_some() { failed = true; }
                    }
                    1 => {
                        parameters.push(Value::Text(text.clone()));
                        sql += &format!("UPDATE t SET value=${} WHERE id={id};", parameters.len());
                        if let Some(value) = expected.get_mut(&id) { *value = text; }
                    }
                    _ => { sql += &format!("DELETE FROM t WHERE id={id};"); expected.remove(&id); }
                }
            }
            sql += match fault {
                1 => "SELECT missing FROM t",
                2 => "COMMIT",
                3 => "SELECT * FROM t LIMIT $256",
                _ => "SELECT * FROM t ORDER BY id",
            };
            failed |= fault != 0;
            let result = stage(transaction, &sql, &parameters);
            prop_assert_eq!(result.is_err(), failed);
            if let Ok(staged) = result {
                let expected_rows = expected.iter().map(|(id,text)|vec![Value::Integer(*id),Value::Text(text.clone())]).collect::<Vec<_>>();
                prop_assert_eq!(&staged.results().last().unwrap().rows, &expected_rows);
                let (transaction, _) = staged.into_parts();
                if commit {
                    transaction.commit().unwrap();
                    model = expected;
                    receipts.push(vec![Value::Integer(index as i64)]);
                } else { transaction.rollback(); }
            }
            if failed || !commit { prop_assert_eq!(database.committed_wal().unwrap(), before); }
            drop(database);
            database = Database::open(directory.path().join("db")).unwrap();
            let expected_rows = model.iter().map(|(id,text)|vec![Value::Integer(*id),Value::Text(text.clone())]).collect::<Vec<_>>();
            prop_assert_eq!(rows(&database), expected_rows);
            prop_assert_eq!(database.view().unwrap().scan("receipts", 128).unwrap(), receipts.clone());
        }
    }
}
