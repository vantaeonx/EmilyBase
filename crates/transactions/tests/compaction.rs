use std::fs;

use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::{Database, Error, recover_image};

fn schema(name: &str) -> Schema {
    Schema {
        name: name.into(),
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
    }
}

fn row(id: i64, text: &str) -> Vec<Value> {
    vec![Value::Integer(id), Value::Text(text.into())]
}

fn initialized(path: &std::path::Path) -> Database {
    let mut db = Database::create(path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(schema("items")).unwrap();
    tx.insert("items", row(7, "initial")).unwrap();
    tx.commit().unwrap();
    db
}

#[test]
fn compaction_removes_repeated_images_preserving_history_ids_and_future_commits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = initialized(&path);
    for index in 0..100 {
        let mut tx = db.begin().unwrap();
        tx.update(
            "items",
            &Key::Integer(7),
            row(7, &format!("revision {index}")),
        )
        .unwrap();
        tx.commit().unwrap();
    }
    let mut tx = db.begin().unwrap();
    tx.create_table(schema("temporary")).unwrap();
    tx.drop_table("temporary").unwrap();
    tx.commit().unwrap();
    let original = db.committed_wal().unwrap();
    let old_transaction = db.last_transaction();
    let old_events = db.view().unwrap().event_count();
    let old_pages = db.view().unwrap().pages().cloned().collect::<Vec<_>>();
    let id = db.database_id();
    let report = db.compact().unwrap();
    assert_eq!(report.previous_wal_bytes, original.len() as u64);
    assert!(report.compacted_wal_bytes < report.previous_wal_bytes / 10);
    assert_eq!(report.transaction, old_transaction);
    assert_eq!(report.pages, old_pages.len());
    assert_eq!(db.last_transaction(), old_transaction);
    let new = db.committed_wal().unwrap();
    assert_eq!(&new[8..10], &2u16.to_le_bytes());
    assert_eq!(
        recover_image(&original, Some(id)).unwrap().last_transaction,
        old_transaction
    );
    assert_eq!(recover_image(&new, Some(id)).unwrap().wal_version, 2);
    assert!(!path.join("redo-next.wal").exists());
    drop(db);
    let mut db = Database::open_bound(&path, Some(id)).unwrap();
    assert_eq!(db.view().unwrap().event_count(), old_events);
    assert_eq!(
        db.view().unwrap().pages().cloned().collect::<Vec<_>>(),
        old_pages
    );
    assert_eq!(
        db.view().unwrap().get("items", &Key::Integer(7)).unwrap(),
        Some(&row(7, "revision 99"))
    );
    let mut tx = db.begin().unwrap();
    assert_eq!(tx.create_table(schema("another")).unwrap(), 3);
    tx.update("items", &Key::Integer(7), row(7, "after compaction"))
        .unwrap();
    assert_eq!(tx.commit().unwrap(), old_transaction + 1);
    db.compact().unwrap();
    drop(db);
    let db = Database::open(path).unwrap();
    assert_eq!(db.last_transaction(), old_transaction + 1);
    assert_eq!(
        db.view().unwrap().get("items", &Key::Integer(7)).unwrap(),
        Some(&row(7, "after compaction"))
    );
}

#[test]
fn baseline_can_contain_more_than_one_transaction_page_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(schema("items")).unwrap();
    tx.commit().unwrap();
    for batch in 0..3 {
        let mut tx = db.begin().unwrap();
        for id in batch * 100..batch * 100 + 100 {
            tx.insert("items", row(id, &"x".repeat(3000))).unwrap();
        }
        tx.commit().unwrap();
    }
    let report = db.compact().unwrap();
    assert!(report.pages > 256);
    drop(db);
    let db = Database::open(path).unwrap();
    assert_eq!(db.view().unwrap().row_count(), 300);
    assert_eq!(db.last_transaction(), 5);
    for id in 0..300 {
        assert_eq!(
            db.view().unwrap().get("items", &Key::Integer(id)).unwrap(),
            Some(&row(id, &"x".repeat(3000)))
        );
    }
}

#[test]
fn stale_missing_and_damaged_checkpoints_cannot_replace_a_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = initialized(&path);
    db.checkpoint().unwrap();
    let stale = fs::read(path.join("checkpoint.emily")).unwrap();
    let mut tx = db.begin().unwrap();
    tx.update("items", &Key::Integer(7), row(7, "latest"))
        .unwrap();
    tx.commit().unwrap();
    db.compact().unwrap();
    assert_eq!(fs::read(path.join("checkpoint.emily")).unwrap(), stale);
    drop(db);
    for payload in [Some(&stale[..]), Some(&b"damaged"[..]), None] {
        if let Some(payload) = payload {
            fs::write(path.join("checkpoint.emily"), payload).unwrap();
        } else {
            fs::remove_file(path.join("checkpoint.emily")).unwrap();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(
            db.view().unwrap().get("items", &Key::Integer(7)).unwrap(),
            Some(&row(7, "latest"))
        );
    }
}

#[test]
fn damaged_source_is_preserved_and_cannot_be_compacted_into_valid_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = initialized(&path);
    let mut damaged = fs::read(path.join("redo.wal")).unwrap();
    damaged[100] ^= 1;
    fs::write(path.join("redo.wal"), &damaged).unwrap();
    assert!(db.compact().is_err());
    assert!(matches!(db.view(), Err(Error::Poisoned)));
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), damaged);
    assert!(!path.join("redo-next.wal").exists());
}

#[test]
fn prepublication_path_errors_preserve_source_and_allow_later_commits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = initialized(&path);
    let before = db.committed_wal().unwrap();
    fs::create_dir(path.join("redo-next.wal")).unwrap();
    assert!(db.compact().is_err());
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), before);
    assert_eq!(fs::read_dir(path.join("redo-next.wal")).unwrap().count(), 0);
    let mut tx = db.begin().unwrap();
    tx.insert("items", row(8, "after staging failure")).unwrap();
    tx.commit().unwrap();
    drop(db);
    assert_eq!(Database::open(path).unwrap().view().unwrap().row_count(), 2);
}
