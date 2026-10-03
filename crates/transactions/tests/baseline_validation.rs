use std::fs;

use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{DATABASE_MARKER, Event, EventKind};
use emilybase_storage::Page;
use emilybase_transactions::{Database, recover_image};
use emilybase_wal::{FRAME_SIZE, Wal, encode_snapshot, recover};

fn schema() -> Schema {
    Schema {
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
    }
}

fn event(table_id: u64, kind: EventKind) -> Vec<u8> {
    Event { table_id, kind }.encode().unwrap()
}

fn row(id: i64) -> Vec<Value> {
    vec![Value::Integer(id), Value::Text("synthetic".into())]
}

#[test]
fn valid_baseline_checksums_cannot_bypass_catalog_primary_key_or_history_validation() {
    for mode in [
        "no_root",
        "deleted_slot",
        "empty_page",
        "unknown_table",
        "duplicate_key",
        "type",
        "reused_table",
    ] {
        let mut page = Page::new(1).unwrap();
        page.insert(&DATABASE_MARKER).unwrap();
        page.insert(&event(1, EventKind::Create(schema()))).unwrap();
        page.insert(&event(1, EventKind::Insert(row(1)))).unwrap();
        let mut pages = vec![page];
        match mode {
            "no_root" => {
                pages[0].update(0, b"not a root marker").unwrap();
            }
            "deleted_slot" => {
                pages[0].delete(2).unwrap();
            }
            "empty_page" => {
                pages.push(Page::new(2).unwrap());
            }
            "unknown_table" => {
                pages[0]
                    .insert(&event(2, EventKind::Insert(row(2))))
                    .unwrap();
            }
            "duplicate_key" => {
                pages[0]
                    .insert(&event(1, EventKind::Insert(row(1))))
                    .unwrap();
            }
            "type" => {
                pages[0]
                    .insert(&event(
                        1,
                        EventKind::Insert(vec![
                            Value::Boolean(true),
                            Value::Text("wrong type".into()),
                        ]),
                    ))
                    .unwrap();
            }
            "reused_table" => {
                pages[0].insert(&event(1, EventKind::Drop)).unwrap();
                pages[0]
                    .insert(&event(1, EventKind::Create(schema())))
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let bytes = encode_snapshot([7; 16], 42, &pages).unwrap();
        assert_eq!(recover(&bytes, None).unwrap().last_transaction(), 42);
        assert!(recover_image(&bytes, None).is_err(), "{mode}");
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("redo.wal"), &bytes).unwrap();
        assert!(Database::open(dir.path()).is_err(), "{mode}");
        assert_eq!(fs::read(dir.path().join("redo.wal")).unwrap(), bytes);
    }
}

#[test]
fn appended_version_two_frames_cannot_rewrite_baseline_or_skip_pages() {
    for mode in ["older", "rewrite", "gap", "no_progress", "invalid_row"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut db = Database::create(&path).unwrap();
        let mut tx = db.begin().unwrap();
        tx.create_table(schema()).unwrap();
        for id in [1, 2] {
            tx.insert(
                "items",
                vec![Value::Integer(id), Value::Text("x".repeat(3000))],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        db.checkpoint().unwrap();
        db.compact().unwrap();
        let id = db.database_id();
        drop(db);
        let (mut wal, recovered) = Wal::open(path.join("redo.wal"), Some(id)).unwrap();
        let mut pages = recovered.baseline.unwrap().pages;
        assert_eq!(pages.len(), 2);
        let mut target = match mode {
            "older" => pages.remove(0),
            "gap" => Page::new(4).unwrap(),
            _ => pages.pop().unwrap(),
        };
        match mode {
            "rewrite" => {
                target
                    .update(0, &event(1, EventKind::Insert(row(2))))
                    .unwrap();
                target.insert(&event(1, EventKind::Insert(row(3)))).unwrap();
            }
            "no_progress" => (),
            "invalid_row" => {
                target.insert(&event(1, EventKind::Insert(row(2)))).unwrap();
            }
            _ => {
                target.insert(&event(1, EventKind::Insert(row(3)))).unwrap();
            }
        }
        wal.append(&[target]).unwrap();
        drop(wal);
        let bytes = fs::read(path.join("redo.wal")).unwrap();
        assert_eq!(recover(&bytes, Some(id)).unwrap().last_transaction(), 3);
        assert!(Database::open(&path).is_err(), "{mode}");
        assert_eq!(fs::read(path.join("redo.wal")).unwrap(), bytes);
    }
}

#[test]
fn damaged_or_missing_selected_baseline_never_uses_a_good_staging_log_or_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(schema()).unwrap();
    tx.insert("items", row(1)).unwrap();
    tx.commit().unwrap();
    db.checkpoint().unwrap();
    db.compact().unwrap();
    let complete = db.committed_wal().unwrap();
    drop(db);
    fs::write(path.join("redo-next.wal"), &complete).unwrap();
    for cut in [64, complete.len() - FRAME_SIZE, complete.len() - 1] {
        fs::write(path.join("redo.wal"), &complete[..cut]).unwrap();
        assert!(Database::open(&path).is_err(), "cut {cut}");
        assert_eq!(fs::read(path.join("redo.wal")).unwrap(), complete[..cut]);
        assert_eq!(fs::read(path.join("redo-next.wal")).unwrap(), complete);
    }
    fs::remove_file(path.join("redo.wal")).unwrap();
    assert!(Database::open(&path).is_err());
    assert_eq!(fs::read(path.join("redo-next.wal")).unwrap(), complete);
}

#[test]
fn ignored_version_two_tail_cannot_appear_in_compacted_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(schema()).unwrap();
    tx.insert("items", row(1)).unwrap();
    tx.commit().unwrap();
    db.compact().unwrap();
    let id = db.database_id();
    let before = db.committed_wal().unwrap();
    let mut staged = db.view().unwrap().clone();
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(row(2)),
        })
        .unwrap();
    drop(db);
    let (mut wal, _) = Wal::open(path.join("redo.wal"), Some(id)).unwrap();
    {
        let mut pending = wal
            .begin(&staged.pages().cloned().collect::<Vec<_>>())
            .unwrap();
        pending.sync_uncommitted().unwrap();
    }
    drop(wal);
    let mut db = Database::open(&path).unwrap();
    assert_eq!(db.last_transaction(), 2);
    assert!(
        db.view()
            .unwrap()
            .get("items", &Key::Integer(2))
            .unwrap()
            .is_none()
    );
    db.compact().unwrap();
    assert_eq!(db.committed_wal().unwrap(), before);
    drop(db);
    assert_eq!(Database::open(path).unwrap().view().unwrap().row_count(), 1);
}
