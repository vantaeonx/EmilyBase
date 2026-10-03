use std::fs;

use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{DATABASE_MARKER, Event, EventKind};
use emilybase_storage::Page;
use emilybase_transactions::{Database, Error, recover_snapshot};
use emilybase_wal::{FRAME_SIZE, Wal};

fn table() -> Schema {
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

#[test]
fn every_byte_cut_before_integrated_commit_preserves_previous_tables() {
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut db = Database::create(&path).unwrap();
        let mut tx = db.begin().unwrap();
        tx.create_table(table()).unwrap();
        tx.insert(
            "items",
            vec![Value::Integer(0), Value::Text("baseline".into())],
        )
        .unwrap();
        tx.commit().unwrap();
        if compacted {
            db.compact().unwrap();
        }
        let boundary = fs::metadata(path.join("redo.wal")).unwrap().len() as usize;
        let mut tx = db.begin().unwrap();
        for id in [1, 2] {
            tx.insert(
                "items",
                vec![Value::Integer(id), Value::Text("x".repeat(3000))],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        drop(db);
        let bytes = fs::read(path.join("redo.wal")).unwrap();
        assert_eq!(bytes.len() - boundary, 3 * FRAME_SIZE);
        for cut in boundary..bytes.len() {
            let snapshot = recover_snapshot(&bytes[..cut], None).unwrap();
            assert_eq!(snapshot.row_count(), 1, "cut {cut}");
            assert!(snapshot.get("items", &Key::Integer(0)).unwrap().is_some());
            assert!(snapshot.get("items", &Key::Integer(1)).unwrap().is_none());
            assert!(snapshot.get("items", &Key::Integer(2)).unwrap().is_none());
        }
        assert_eq!(recover_snapshot(&bytes, None).unwrap().row_count(), 3);
    }
}

#[test]
fn redo_target_gaps_empty_roots_and_no_progress_are_rejected() {
    for mode in ["gap", "empty_root", "unchanged"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        drop(Database::create(&path).unwrap());
        let (mut wal, recovery) = Wal::open(path.join("redo.wal"), None).unwrap();
        let page = match mode {
            "gap" => {
                let mut page = Page::new(3).unwrap();
                page.insert(&DATABASE_MARKER).unwrap();
                page
            }
            "empty_root" => Page::new(2).unwrap(),
            _ => recovery.committed[0].pages[0].clone(),
        };
        wal.append(&[page]).unwrap();
        drop(wal);
        let before = fs::read(path.join("redo.wal")).unwrap();
        assert!(Database::open(&path).is_err(), "mode {mode}");
        assert_eq!(fs::read(path.join("redo.wal")).unwrap(), before);
    }
}

#[test]
fn missing_or_corrupt_wal_never_falls_back_to_an_older_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(table()).unwrap();
    tx.commit().unwrap();
    db.checkpoint().unwrap();
    let mut tx = db.begin().unwrap();
    tx.insert(
        "items",
        vec![
            Value::Integer(1),
            Value::Text("newer than checkpoint".into()),
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    drop(db);
    let log = path.join("redo.wal");
    let mut bytes = fs::read(&log).unwrap();
    let offset = bytes.len() - FRAME_SIZE + 100;
    bytes[offset] ^= 1;
    fs::write(&log, &bytes).unwrap();
    assert!(matches!(
        Database::open(&path),
        Err(Error::Wal(emilybase_wal::Error::Checksum))
    ));
    assert_eq!(fs::read(&log).unwrap(), bytes);
    fs::remove_file(log).unwrap();
    assert!(Database::open(path).is_err());
}

#[test]
fn noncontiguous_images_and_redo_of_older_pages_cannot_bypass_history_checks() {
    for mode in ["noncontiguous", "older"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut db = Database::create(&path).unwrap();
        let mut tx = db.begin().unwrap();
        tx.create_table(table()).unwrap();
        tx.insert(
            "items",
            vec![Value::Integer(1), Value::Text("x".repeat(3000))],
        )
        .unwrap();
        tx.insert(
            "items",
            vec![Value::Integer(2), Value::Text("y".repeat(3000))],
        )
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(db.view().unwrap().page_count(), 2);
        drop(db);
        let (mut wal, recovered) = Wal::open(path.join("redo.wal"), None).unwrap();
        if mode == "older" {
            let mut old = recovered.committed[1].pages[0].clone();
            old.insert(
                &Event {
                    table_id: 1,
                    kind: EventKind::Drop,
                }
                .encode()
                .unwrap(),
            )
            .unwrap();
            wal.append(&[old]).unwrap();
        } else {
            let mut a = Page::new(3).unwrap();
            a.insert(b"synthetic").unwrap();
            let mut b = Page::new(5).unwrap();
            b.insert(b"synthetic").unwrap();
            wal.append(&[a, b]).unwrap();
        }
        drop(wal);
        assert!(matches!(Database::open(path), Err(Error::History(_))));
    }
}
