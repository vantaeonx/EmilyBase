use std::fs;

use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::{Database, Error};
use emilybase_wal::{FRAME_SIZE, MAX_WAL_BYTES};

#[test]
fn reaching_actual_journal_capacity_does_not_prevent_compaction_or_future_commits() {
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut db = Database::create(&path).unwrap();
        let mut tx = db.begin().unwrap();
        tx.create_table(Schema {
            name: "counter".into(),
            columns: vec![
                Column {
                    name: "id".into(),
                    data_type: DataType::Integer,
                    nullable: false,
                },
                Column {
                    name: "value".into(),
                    data_type: DataType::Integer,
                    nullable: false,
                },
            ],
            primary_key: 0,
        })
        .unwrap();
        tx.insert("counter", vec![Value::Integer(0), Value::Integer(0)])
            .unwrap();
        tx.commit().unwrap();
        if compacted {
            db.compact().unwrap();
        }
        let mut acknowledged = 0;
        let mut reached_limit = false;
        for value in 1..10000 {
            let before = fs::metadata(path.join("redo.wal")).unwrap().len();
            let transaction = db.last_transaction();
            let mut tx = db.begin().unwrap();
            tx.update(
                "counter",
                &Key::Integer(0),
                vec![Value::Integer(0), Value::Integer(value)],
            )
            .unwrap();
            match tx.commit() {
                Ok(next) => {
                    assert_eq!(next, transaction + 1);
                    acknowledged = value;
                }
                Err(Error::Wal(emilybase_wal::Error::Limit("journal bytes"))) => {
                    assert_eq!(fs::metadata(path.join("redo.wal")).unwrap().len(), before);
                    assert_eq!(db.last_transaction(), transaction);
                    assert!(MAX_WAL_BYTES as u64 - before < (2 * FRAME_SIZE) as u64);
                    reached_limit = true;
                    break;
                }
                Err(error) => panic!("unexpected capacity error: {error}"),
            }
        }
        assert!(reached_limit, "test must reach the actual 64 MiB bound");
        let transaction = db.last_transaction();
        let expected = vec![Value::Integer(0), Value::Integer(acknowledged)];
        assert_eq!(
            db.view().unwrap().get("counter", &Key::Integer(0)).unwrap(),
            Some(&expected)
        );
        let report = db.compact().unwrap();
        assert!(report.compacted_wal_bytes < report.previous_wal_bytes / 10);
        assert_eq!(report.transaction, transaction);
        let mut tx = db.begin().unwrap();
        tx.update(
            "counter",
            &Key::Integer(0),
            vec![Value::Integer(0), Value::Integer(acknowledged + 1)],
        )
        .unwrap();
        assert_eq!(tx.commit().unwrap(), transaction + 1);
        drop(db);
        let db = Database::open(path).unwrap();
        assert_eq!(db.last_transaction(), transaction + 1);
        assert_eq!(
            db.view().unwrap().get("counter", &Key::Integer(0)).unwrap(),
            Some(&vec![Value::Integer(0), Value::Integer(acknowledged + 1)])
        );
    }
}
