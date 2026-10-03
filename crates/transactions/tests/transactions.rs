use std::fs;

use emilybase_catalog::{Column, DataType, Key, Row, Schema, Value};
use emilybase_database::{DATABASE_MARKER, Event, EventKind};
use emilybase_storage::{Page, Pager};
use emilybase_transactions::{Database, Error, MAX_TRANSACTION_EVENTS};
use emilybase_wal::Wal;

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

fn row(id: i64, text: &str) -> Row {
    vec![Value::Integer(id), Value::Text(text.into())]
}

#[test]
fn committed_multitable_changes_replay_as_one_batch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    let id = db.database_id();
    let mut tx = db.begin().unwrap();
    assert_eq!(tx.create_table(schema("items")).unwrap(), 1);
    tx.create_table(schema("other")).unwrap();
    tx.insert("items", row(1, "initial")).unwrap();
    tx.insert("other", row(1, "separate")).unwrap();
    tx.update("items", &Key::Integer(1), row(1, "updated"))
        .unwrap();
    assert_eq!(
        tx.view().unwrap().get("items", &Key::Integer(1)).unwrap(),
        Some(&row(1, "updated"))
    );
    assert_eq!(tx.commit().unwrap(), 2);
    assert_eq!(db.view().unwrap().row_count(), 2);
    drop(db);
    let mut db = Database::open_bound(&path, Some(id)).unwrap();
    assert_eq!(db.last_transaction(), 2);
    assert_eq!(
        db.view().unwrap().get("other", &Key::Integer(1)).unwrap(),
        Some(&row(1, "separate"))
    );
    let mut tx = db.begin().unwrap();
    tx.delete("items", &Key::Integer(1)).unwrap();
    tx.drop_table("other").unwrap();
    tx.commit().unwrap();
    drop(db);
    let db = Database::open(path).unwrap();
    assert_eq!(db.view().unwrap().schemas().len(), 1);
    assert_eq!(db.view().unwrap().row_count(), 0);
}

#[test]
fn rollback_drop_and_failed_writes_discard_the_entire_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    let before = fs::read(path.join("redo.wal")).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(schema("rolled_back")).unwrap();
    tx.insert("rolled_back", row(1, "discarded")).unwrap();
    tx.rollback();
    {
        let mut tx = db.begin().unwrap();
        tx.create_table(schema("dropped_transaction")).unwrap();
    }
    let mut tx = db.begin().unwrap();
    tx.create_table(schema("items")).unwrap();
    tx.insert("items", row(1, "discarded on error")).unwrap();
    assert!(tx.insert("items", row(1, "duplicate")).is_err());
    assert!(matches!(tx.view(), Err(Error::Aborted)));
    assert!(matches!(
        tx.create_table(schema("blocked")),
        Err(Error::Aborted)
    ));
    assert!(matches!(tx.commit(), Err(Error::Aborted)));
    assert!(db.view().unwrap().schemas().is_empty());
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), before);
    assert_eq!(db.begin().unwrap().commit().unwrap(), 1);
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), before);
    drop(db);
    assert!(
        Database::open(path)
            .unwrap()
            .view()
            .unwrap()
            .schemas()
            .is_empty()
    );
}

#[test]
fn spilling_pages_preserves_history_and_checkpoints_match_current_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(schema("items")).unwrap();
    tx.commit().unwrap();
    for batch in 0..3 {
        let mut tx = db.begin().unwrap();
        for id in batch * 4..batch * 4 + 4 {
            tx.insert("items", row(id, &"x".repeat(3000))).unwrap();
        }
        tx.commit().unwrap();
        db.checkpoint().unwrap();
        drop(db);
        db = Database::open(&path).unwrap();
        assert_eq!(db.view().unwrap().row_count(), (batch + 1) as usize * 4);
    }
    assert!(db.view().unwrap().page_count() >= 12);
    let cached = emilybase_database::Database::open(path.join("checkpoint.emily")).unwrap();
    assert_eq!(
        cached.scan("items", 100).unwrap(),
        db.view().unwrap().scan("items", 100).unwrap()
    );
}

#[test]
fn damaged_and_incomplete_checkpoint_files_cannot_change_recovered_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(schema("items")).unwrap();
    tx.insert("items", row(7, "acknowledged")).unwrap();
    tx.commit().unwrap();
    db.checkpoint().unwrap();
    drop(db);
    for damage in [&b""[..], &b"torn checkpoint"[..], &vec![0u8; 8192][..]] {
        fs::write(path.join("checkpoint.emily"), damage).unwrap();
        fs::write(
            path.join("checkpoint-next.emily"),
            b"interrupted checkpoint",
        )
        .unwrap();
        let mut db = Database::open(&path).unwrap();
        assert_eq!(
            db.view().unwrap().get("items", &Key::Integer(7)).unwrap(),
            Some(&row(7, "acknowledged"))
        );
        db.checkpoint().unwrap();
        assert!(!path.join("checkpoint-next.emily").exists());
    }
}

#[test]
fn capacity_errors_abort_before_any_journal_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    let before = fs::read(path.join("redo.wal")).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(schema("items")).unwrap();
    for id in 0..MAX_TRANSACTION_EVENTS - 1 {
        tx.insert("items", row(id as i64, "bounded")).unwrap();
    }
    assert!(matches!(
        tx.insert("items", row(999, "too many")),
        Err(Error::Limit)
    ));
    assert!(matches!(tx.commit(), Err(Error::Aborted)));
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), before);
    assert_eq!(db.view().unwrap().row_count(), 0);
}

#[test]
fn existing_directories_and_foreign_identity_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let db = Database::create(&path).unwrap();
    let before = fs::read(path.join("redo.wal")).unwrap();
    assert!(Database::create(&path).is_err());
    assert!(matches!(
        Database::open(&path),
        Err(Error::Wal(emilybase_wal::Error::Busy))
    ));
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), before);
    drop(db);
    assert!(matches!(
        Database::open_bound(&path, Some([7; 16])),
        Err(Error::Wal(emilybase_wal::Error::Identity))
    ));
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), before);
}

#[test]
fn incomplete_creation_and_legacy_files_cannot_be_opened_as_durable_databases() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("incomplete");
    fs::create_dir(&path).unwrap();
    drop(Wal::create(path.join("redo.wal"), [7; 16]).unwrap());
    assert!(Database::open(&path).is_err());
    let legacy = dir.path().join("legacy.emily");
    drop(emilybase_database::Database::create(&legacy).unwrap());
    let before = fs::read(&legacy).unwrap();
    assert!(Database::open(&legacy).is_err());
    assert_eq!(fs::read(legacy).unwrap(), before);
}

#[test]
fn valid_wal_checksums_cannot_authorize_rewriting_old_events() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(schema("items")).unwrap();
    tx.commit().unwrap();
    drop(db);
    let (mut wal, _) = Wal::open(path.join("redo.wal"), None).unwrap();
    let mut page = Page::new(1).unwrap();
    page.insert(&DATABASE_MARKER).unwrap();
    page.insert(
        &Event {
            table_id: 1,
            kind: EventKind::Create(schema("rewritten")),
        }
        .encode()
        .unwrap(),
    )
    .unwrap();
    page.insert(
        &Event {
            table_id: 1,
            kind: EventKind::Insert(row(9, "malformed history")),
        }
        .encode()
        .unwrap(),
    )
    .unwrap();
    wal.append(&[page]).unwrap();
    drop(wal);
    let before = fs::read(path.join("redo.wal")).unwrap();
    assert!(matches!(
        Database::open(&path),
        Err(Error::History("redo rewrites committed history"))
    ));
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), before);
}

#[test]
fn checkpoint_replacement_cannot_follow_a_symlink_to_another_file() {
    #[cfg(unix)]
    {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let other = dir.path().join("unrelated.txt");
        fs::write(&other, b"preserve this unrelated synthetic file").unwrap();
        let mut db = Database::create(&path).unwrap();
        std::os::unix::fs::symlink(&other, path.join("checkpoint.emily")).unwrap();
        std::os::unix::fs::symlink(&other, path.join("checkpoint-next.emily")).unwrap();
        db.checkpoint().unwrap();
        assert_eq!(
            fs::read(other).unwrap(),
            b"preserve this unrelated synthetic file"
        );
        let mut pager = Pager::open(path.join("checkpoint.emily")).unwrap();
        pager.verify().unwrap();
    }
}

#[test]
fn committed_redo_still_requires_valid_relational_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    drop(Database::create(&path).unwrap());
    let (mut wal, recovery) = Wal::open(path.join("redo.wal"), None).unwrap();
    let mut page = recovery.committed[0].pages[0].clone();
    page.insert(
        &Event {
            table_id: 1,
            kind: EventKind::Insert(row(1, "no table exists")),
        }
        .encode()
        .unwrap(),
    )
    .unwrap();
    wal.append(&[page]).unwrap();
    drop(wal);
    let before = fs::read(path.join("redo.wal")).unwrap();
    assert!(Database::open(&path).is_err());
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn managed_directory_and_checkpoint_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    db.checkpoint().unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(path.join("checkpoint.emily"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
