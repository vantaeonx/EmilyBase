use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::{Database, Error, MAX_TRANSACTION_EVENTS};

fn schema() -> Schema {
    Schema {
        name: "t".into(),
        primary_key: 0,
        columns: vec![Column {
            name: "id".into(),
            data_type: DataType::Integer,
            nullable: false,
        }],
    }
}

#[test]
fn remaining_event_capacity_counts_every_staged_write_without_reserving_it() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("source");
    let mut database = Database::create(&path).unwrap();
    let wal = database.committed_wal().unwrap();
    let mut transaction = database.begin().unwrap();
    assert_eq!(
        transaction.remaining_events().unwrap(),
        MAX_TRANSACTION_EVENTS
    );
    transaction.create_table(schema()).unwrap();
    assert_eq!(transaction.remaining_events().unwrap(), 255);
    transaction.insert("t", vec![Value::Integer(1)]).unwrap();
    assert_eq!(transaction.remaining_events().unwrap(), 254);
    transaction
        .view()
        .unwrap()
        .get("t", &Key::Integer(1))
        .unwrap();
    assert_eq!(transaction.remaining_events().unwrap(), 254);
    transaction
        .update("t", &Key::Integer(1), vec![Value::Integer(1)])
        .unwrap();
    assert_eq!(transaction.remaining_events().unwrap(), 253);
    transaction.delete("t", &Key::Integer(1)).unwrap();
    assert_eq!(transaction.remaining_events().unwrap(), 252);
    transaction.drop_table("t").unwrap();
    assert_eq!(transaction.remaining_events().unwrap(), 251);
    transaction.rollback();
    assert_eq!(database.committed_wal().unwrap(), wal);
    assert!(database.view().unwrap().schemas().is_empty());
    let transaction = database.begin().unwrap();
    assert_eq!(transaction.remaining_events().unwrap(), 256);
    transaction.commit().unwrap();
    assert_eq!(database.committed_wal().unwrap(), wal);
}

#[test]
fn zero_remaining_capacity_keeps_reads_valid_but_failed_writes_abort_the_view() {
    let temporary = tempfile::tempdir().unwrap();
    let mut database = Database::create(temporary.path().join("source")).unwrap();
    let mut transaction = database.begin().unwrap();
    transaction.create_table(schema()).unwrap();
    for id in 0..255 {
        transaction.insert("t", vec![Value::Integer(id)]).unwrap();
        assert_eq!(transaction.remaining_events().unwrap(), 254 - id as usize);
    }
    assert_eq!(transaction.remaining_events().unwrap(), 0);
    assert_eq!(transaction.view().unwrap().row_count(), 255);
    transaction.commit().unwrap();
    let wal = database.committed_wal().unwrap();
    let mut transaction = database.begin().unwrap();
    assert_eq!(transaction.remaining_events().unwrap(), 256);
    assert!(transaction.insert("t", vec![Value::Integer(0)]).is_err());
    assert!(matches!(
        transaction.remaining_events(),
        Err(Error::Aborted)
    ));
    assert!(matches!(transaction.view(), Err(Error::Aborted)));
    assert!(matches!(transaction.commit(), Err(Error::Aborted)));
    assert_eq!(database.committed_wal().unwrap(), wal);
    let mut transaction = database.begin().unwrap();
    for _ in 0..256 {
        transaction
            .update("t", &Key::Integer(1), vec![Value::Integer(1)])
            .unwrap();
    }
    assert_eq!(transaction.remaining_events().unwrap(), 0);
    assert!(matches!(
        transaction.delete("t", &Key::Integer(1)),
        Err(Error::Limit)
    ));
    assert!(matches!(
        transaction.remaining_events(),
        Err(Error::Aborted)
    ));
    transaction.rollback();
    assert_eq!(database.committed_wal().unwrap(), wal);
}
