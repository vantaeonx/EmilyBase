use emilybase_catalog::{Column, DataType, Key, Row, Schema, Value};
use emilybase_database::{Database, Error, Event, EventKind};
use emilybase_storage::{Page, Pager};

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
                name: "title".into(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
        primary_key: 0,
    }
}

fn row(id: i64, text: &str) -> Row {
    vec![Value::Integer(id), Value::Text(text.into())]
}

#[test]
fn table_lifecycle_survives_reopen_and_names_can_be_reused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tables.emily");
    let mut db = Database::create(&path).unwrap();
    assert_eq!(db.create_table(schema("items")).unwrap(), 1);
    db.create_table(schema("other")).unwrap();
    db.insert("items", row(7, "first")).unwrap();
    db.insert("other", row(7, "separate table")).unwrap();
    db.update("items", &Key::Integer(7), row(7, "updated"))
        .unwrap();
    drop(db);
    let mut db = Database::open(&path).unwrap();
    assert_eq!(
        db.get("items", &Key::Integer(7)).unwrap(),
        Some(&row(7, "updated"))
    );
    assert_eq!(
        db.get("other", &Key::Integer(7)).unwrap(),
        Some(&row(7, "separate table"))
    );
    db.delete("items", &Key::Integer(7)).unwrap();
    db.drop_table("items").unwrap();
    assert_eq!(db.create_table(schema("items")).unwrap(), 3);
    assert!(db.scan("items", 100).unwrap().is_empty());
    drop(db);
    let db = Database::open(path).unwrap();
    assert_eq!(db.schemas().unwrap().len(), 2);
    assert_eq!(db.row_count(), 1);
}

#[test]
fn invalid_operations_leave_file_and_state_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("validation.emily");
    let mut db = Database::create(&path).unwrap();
    db.create_table(schema("items")).unwrap();
    db.insert("items", row(1, "original")).unwrap();
    let before = std::fs::read(&path).unwrap();
    let events = db.event_count();
    assert!(matches!(
        db.insert("items", row(1, "duplicate")),
        Err(Error::DuplicateKey)
    ));
    assert!(db.insert("items", vec![Value::Integer(2)]).is_err());
    assert!(db.insert("items", vec![Value::Null, Value::Null]).is_err());
    assert!(matches!(
        db.update("items", &Key::Integer(1), row(2, "changed key")),
        Err(Error::PrimaryKeyChange)
    ));
    assert!(matches!(
        db.update("items", &Key::Integer(9), row(9, "absent")),
        Err(Error::NoRow)
    ));
    assert!(db.delete("items", &Key::Text("wrong type".into())).is_err());
    assert!(db.delete("items", &Key::Integer(9)).is_err());
    assert!(db.drop_table("absent").is_err());
    assert!(db.create_table(schema("items")).is_err());
    assert!(db.insert("absent", row(1, "missing")).is_err());
    assert_eq!(db.event_count(), events);
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert_eq!(
        db.get("items", &Key::Integer(1)).unwrap(),
        Some(&row(1, "original"))
    );
}

#[test]
fn history_spans_pages_and_scan_is_ordered_and_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pages.emily");
    let mut db = Database::create(&path).unwrap();
    db.create_table(schema("items")).unwrap();
    for id in [-1, 20, 3] {
        db.insert("items", row(id, &"x".repeat(3000))).unwrap();
    }
    assert!(db.page_count() >= 3);
    drop(db);
    let db = Database::open(path).unwrap();
    let rows = db.scan("items", 2).unwrap();
    assert_eq!(rows[0][0], Value::Integer(-1));
    assert_eq!(rows[1][0], Value::Integer(3));
    assert!(db.scan("items", 0).unwrap().is_empty());
    assert!(db.scan("items", usize::MAX).is_err());
}

#[test]
fn corrupt_event_order_is_rejected_without_mutating_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("order.emily");
    drop(Database::create(&path).unwrap());
    let mut pager = Pager::open(&path).unwrap();
    let mut page = pager.read_page(1).unwrap();
    page.insert(
        &Event {
            table_id: 1,
            kind: EventKind::Insert(row(1, "no table")),
        }
        .encode()
        .unwrap(),
    )
    .unwrap();
    pager.write_page(&page).unwrap();
    drop(pager);
    let before = std::fs::read(&path).unwrap();
    assert!(Database::open(&path).is_err());
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn raw_files_and_deleted_history_slots_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let raw = dir.path().join("raw.emily");
    drop(Pager::create(&raw).unwrap());
    assert!(matches!(Database::open(&raw), Err(Error::NotTableFile)));
    let mut pager = Pager::open(&raw).unwrap();
    let mut page = Page::new(1).unwrap();
    page.insert(b"ordinary raw record").unwrap();
    pager.write_page(&page).unwrap();
    drop(pager);
    assert!(Database::open(raw).is_err());
    let path = dir.path().join("deleted.emily");
    drop(Database::create(&path).unwrap());
    let mut pager = Pager::open(&path).unwrap();
    let mut page = pager.read_page(1).unwrap();
    page.delete(0).unwrap();
    pager.write_page(&page).unwrap();
    drop(pager);
    assert!(Database::open(path).is_err());
}

#[test]
fn text_primary_keys_sort_and_keep_binary_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("types.emily");
    let schema = Schema {
        name: "typed".into(),
        columns: vec![
            Column {
                name: "key".into(),
                data_type: DataType::Text,
                nullable: false,
            },
            Column {
                name: "flag".into(),
                data_type: DataType::Boolean,
                nullable: false,
            },
            Column {
                name: "number".into(),
                data_type: DataType::Float,
                nullable: false,
            },
            Column {
                name: "data".into(),
                data_type: DataType::Bytes,
                nullable: false,
            },
        ],
        primary_key: 0,
    };
    let mut db = Database::create(&path).unwrap();
    db.create_table(schema).unwrap();
    let row = vec![
        Value::Text("ключ".into()),
        Value::Boolean(true),
        Value::Float(1.5),
        Value::Bytes(vec![0, 255]),
    ];
    db.insert("typed", row.clone()).unwrap();
    drop(db);
    assert_eq!(
        Database::open(path)
            .unwrap()
            .get("typed", &Key::Text("ключ".into()))
            .unwrap(),
        Some(&row)
    );
}

#[test]
fn valid_checksums_do_not_hide_invalid_history_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let invalid = [
        Event {
            table_id: 0,
            kind: EventKind::Root,
        },
        Event {
            table_id: 7,
            kind: EventKind::Create(schema("wrong_id")),
        },
        Event {
            table_id: 1,
            kind: EventKind::Insert(row(1, "duplicate")),
        },
        Event {
            table_id: 1,
            kind: EventKind::Replace(row(9, "missing")),
        },
        Event {
            table_id: 1,
            kind: EventKind::Delete(Key::Integer(9)),
        },
        Event {
            table_id: 7,
            kind: EventKind::Drop,
        },
    ];
    for (index, event) in invalid.iter().enumerate() {
        let path = dir.path().join(format!("invalid{index}.emily"));
        let mut db = Database::create(&path).unwrap();
        db.create_table(schema("items")).unwrap();
        db.insert("items", row(1, "original")).unwrap();
        drop(db);
        let mut pager = Pager::open(&path).unwrap();
        let mut page = pager.read_page(1).unwrap();
        page.insert(&event.encode().unwrap()).unwrap();
        pager.write_page(&page).unwrap();
        drop(pager);
        let before = std::fs::read(&path).unwrap();
        assert!(Database::open(&path).is_err());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}

#[test]
fn database_creation_cannot_overwrite_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    let raw = dir.path().join("raw.emily");
    std::fs::write(&raw, b"synthetic file to preserve").unwrap();
    assert!(Database::create(&raw).is_err());
    assert_eq!(std::fs::read(raw).unwrap(), b"synthetic file to preserve");
    let path = dir.path().join("managed.emily");
    let db = Database::create(&path).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(Database::create(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    drop(db);
    let reopened = Database::open(path).unwrap();
    assert!(reopened.schemas().unwrap().is_empty());
    assert_eq!(reopened.event_count(), 1);
}
