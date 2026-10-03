use emilybase_catalog::{Column, DataType, Key, Row, Schema, Value};
use emilybase_transactions::{BoundRowLocation, Database, Error};
use std::fs;

fn row(text: &str) -> Row {
    vec![Value::Integer(7), Value::Text(text.into())]
}

fn initialized(path: &std::path::Path) -> Database {
    let mut database = Database::create(path).unwrap();
    let mut transaction = database.begin().unwrap();
    transaction
        .create_table(Schema {
            name: "items".into(),
            columns: vec![
                Column {
                    name: "id".into(),
                    data_type: DataType::Integer,
                    nullable: false,
                },
                Column {
                    name: "text".into(),
                    data_type: DataType::Text,
                    nullable: false,
                },
            ],
            primary_key: 0,
        })
        .unwrap();
    transaction.insert("items", row("committed")).unwrap();
    transaction.commit().unwrap();
    database
}

fn location(database: &Database) -> BoundRowLocation {
    database
        .row_location("items", &Key::Integer(7))
        .unwrap()
        .unwrap()
}

#[test]
fn identical_physical_rows_in_independent_databases_require_their_persistent_identity() {
    let dir = tempfile::tempdir().unwrap();
    let a = initialized(&dir.path().join("a"));
    let b = initialized(&dir.path().join("b"));
    let left = location(&a);
    let right = location(&b);
    assert_eq!(left.row, right.row);
    assert_ne!(left.database_id, right.database_id);
    assert!(matches!(
        b.resolve_row_location("items", &Key::Integer(7), left),
        Err(Error::LocationDatabase)
    ));
    assert!(matches!(
        a.resolve_row_location("items", &Key::Integer(7), right),
        Err(Error::LocationDatabase)
    ));
    assert_eq!(
        a.resolve_row_location("items", &Key::Integer(7), left)
            .unwrap(),
        &row("committed")
    );
}

#[test]
fn staged_rollback_and_drop_cannot_publish_locations_or_modify_wal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut database = initialized(&path);
    let original = location(&database);
    let wal = fs::read(path.join("redo.wal")).unwrap();
    let id = database.last_transaction();
    let mut discarded = Vec::new();
    for explicit in [false, true] {
        let mut transaction = database.begin().unwrap();
        transaction
            .update("items", &Key::Integer(7), row("discarded"))
            .unwrap();
        let speculative = transaction
            .row_location("items", &Key::Integer(7))
            .unwrap()
            .unwrap();
        assert!(
            transaction
                .resolve_row_location("items", &Key::Integer(7), original)
                .is_err()
        );
        assert_eq!(
            transaction
                .resolve_row_location("items", &Key::Integer(7), speculative)
                .unwrap(),
            &row("discarded")
        );
        discarded.push(speculative);
        if explicit {
            transaction.rollback();
        } else {
            drop(transaction);
        }
        assert_eq!(location(&database), original);
        assert_eq!(fs::read(path.join("redo.wal")).unwrap(), wal);
        assert_eq!(database.last_transaction(), id);
    }
    let mut transaction = database.begin().unwrap();
    transaction
        .update("items", &Key::Integer(7), row("different committed image"))
        .unwrap();
    let committed = transaction
        .row_location("items", &Key::Integer(7))
        .unwrap()
        .unwrap();
    for old in discarded {
        assert_eq!(
            (old.row.page_id, old.row.slot_id),
            (committed.row.page_id, committed.row.slot_id)
        );
        assert_ne!(old.row.fingerprint, committed.row.fingerprint);
        assert!(
            transaction
                .resolve_row_location("items", &Key::Integer(7), old)
                .is_err()
        );
    }
    transaction.commit().unwrap();
    assert_eq!(location(&database), committed);
    assert!(
        database
            .resolve_row_location("items", &Key::Integer(7), original)
            .is_err()
    );
    drop(database);
    let database = Database::open(&path).unwrap();
    assert_eq!(location(&database), committed);
    assert_eq!(
        database
            .resolve_row_location("items", &Key::Integer(7), committed)
            .unwrap(),
        &row("different committed image")
    );
}

#[test]
fn checkpoint_reopen_and_both_wal_versions_preserve_current_physical_locations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut database = initialized(&path);
    let expected = location(&database);
    let transaction = database.last_transaction();
    let before = database
        .view()
        .unwrap()
        .pages()
        .map(|page| page.encode())
        .collect::<Vec<_>>();
    for compacted in [false, true] {
        if compacted {
            database.compact().unwrap();
        }
        database.checkpoint().unwrap();
        assert_eq!(database.last_transaction(), transaction);
        assert_eq!(location(&database), expected);
        assert_eq!(
            database
                .view()
                .unwrap()
                .pages()
                .map(|page| page.encode())
                .collect::<Vec<_>>(),
            before
        );
        drop(database);
        // The cache is optional; rebuilding the location map must use committed WAL.
        fs::write(path.join("checkpoint.emily"), b"synthetic damaged cache").unwrap();
        database = Database::open(&path).unwrap();
        assert_eq!(location(&database), expected);
        assert_eq!(
            database
                .resolve_row_location("items", &Key::Integer(7), expected)
                .unwrap(),
            &row("committed")
        );
        let bytes = fs::read(path.join("redo.wal")).unwrap();
        assert_eq!(
            u16::from_le_bytes(bytes[8..10].try_into().unwrap()),
            if compacted { 2 } else { 1 }
        );
    }
}

#[test]
fn failed_transaction_refuses_even_previously_valid_and_foreign_location_reads() {
    let dir = tempfile::tempdir().unwrap();
    let mut database = initialized(&dir.path().join("db"));
    let expected = location(&database);
    let mut transaction = database.begin().unwrap();
    transaction
        .update("items", &Key::Integer(7), row("will abort"))
        .unwrap();
    assert!(transaction.insert("items", row("duplicate")).is_err());
    assert!(matches!(
        transaction.row_location("items", &Key::Integer(7)),
        Err(Error::Aborted)
    ));
    let mut foreign = expected;
    foreign.database_id[0] ^= 1;
    for value in [expected, foreign] {
        assert!(matches!(
            transaction.resolve_row_location("items", &Key::Integer(7), value),
            Err(Error::Aborted)
        ));
    }
    assert!(matches!(transaction.commit(), Err(Error::Aborted)));
    assert_eq!(location(&database), expected);
}
