use std::fs;

use emilybase_backup::{create, inspect, restore};
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::Database;

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

#[test]
fn restore_replays_all_tables_preserves_identity_and_accepts_new_commits() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let archive = dir.path().join("snapshot.backup");
    let target = dir.path().join("restored");
    let mut db = Database::create(&source).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(schema()).unwrap();
    for id in [7, -1, 3] {
        tx.insert(
            "items",
            vec![Value::Integer(id), Value::Text("x".repeat(3000))],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    let expected = db.view().unwrap().scan("items", 100).unwrap();
    let report = create(&mut db, &archive).unwrap();
    let archive_before = fs::read(&archive).unwrap();
    let source_before = fs::read(source.join("redo.wal")).unwrap();
    assert_eq!(restore(&archive, &target).unwrap(), report);
    let mut recovered = Database::open(&target).unwrap();
    assert_eq!(recovered.database_id(), db.database_id());
    assert_eq!(recovered.last_transaction(), db.last_transaction());
    assert_eq!(
        recovered.view().unwrap().scan("items", 100).unwrap(),
        expected
    );
    assert_eq!(
        recovered.view().unwrap().schemas(),
        db.view().unwrap().schemas()
    );
    assert!(target.join("checkpoint.emily").is_file());
    let mut tx = recovered.begin().unwrap();
    tx.insert(
        "items",
        vec![Value::Integer(99), Value::Text("new commit".into())],
    )
    .unwrap();
    tx.commit().unwrap();
    recovered.checkpoint().unwrap();
    drop(recovered);
    assert!(
        Database::open(&target)
            .unwrap()
            .view()
            .unwrap()
            .get("items", &Key::Integer(99))
            .unwrap()
            .is_some()
    );
    assert_eq!(fs::read(source.join("redo.wal")).unwrap(), source_before);
    assert_eq!(fs::read(&archive).unwrap(), archive_before);
    assert_eq!(inspect(archive).unwrap(), report);
}

#[test]
fn corrupt_archive_cannot_create_or_change_a_restore_destination() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("bad.backup");
    fs::write(&archive, b"invalid synthetic archive").unwrap();
    let target = dir.path().join("new-db");
    assert!(restore(&archive, &target).is_err());
    assert!(!target.exists());
    let existing = dir.path().join("existing");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("keep.txt"), b"preserve").unwrap();
    assert!(restore(&archive, &existing).is_err());
    assert_eq!(fs::read(existing.join("keep.txt")).unwrap(), b"preserve");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn no_replace_publication_preserves_existing_empty_dirs_files_and_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::create(dir.path().join("source")).unwrap();
    let archive = dir.path().join("snapshot.backup");
    create(&mut db, &archive).unwrap();
    let empty = dir.path().join("empty");
    fs::create_dir(&empty).unwrap();
    assert!(restore(&archive, &empty).is_err());
    assert_eq!(fs::read_dir(&empty).unwrap().count(), 0);
    let file = dir.path().join("keep.txt");
    fs::write(&file, b"preserve unrelated file").unwrap();
    assert!(restore(&archive, &file).is_err());
    assert_eq!(fs::read(&file).unwrap(), b"preserve unrelated file");
    #[cfg(unix)]
    {
        let link = dir.path().join("alias");
        std::os::unix::fs::symlink(&empty, &link).unwrap();
        assert!(restore(&archive, &link).is_err());
        assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
    }
    assert!(!fs::read_dir(dir.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".emilybase-backup-")
    }));
}

#[test]
fn restore_preserves_every_type_nullable_values_text_keys_and_schema_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::create(dir.path().join("source")).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(Schema {
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
                name: "integer".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
            Column {
                name: "float".into(),
                data_type: DataType::Float,
                nullable: false,
            },
            Column {
                name: "bytes".into(),
                data_type: DataType::Bytes,
                nullable: false,
            },
            Column {
                name: "optional".into(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
        primary_key: 0,
    })
    .unwrap();
    tx.create_table(schema()).unwrap();
    tx.insert(
        "typed",
        vec![
            Value::Text("ключ 🔑".into()),
            Value::Boolean(true),
            Value::Integer(i64::MIN),
            Value::Float(-1.25),
            Value::Bytes(vec![0, 1, 255]),
            Value::Null,
        ],
    )
    .unwrap();
    tx.drop_table("items").unwrap();
    tx.create_table(schema()).unwrap();
    tx.insert(
        "items",
        vec![Value::Integer(9), Value::Text("new table identity".into())],
    )
    .unwrap();
    tx.commit().unwrap();
    let archive = dir.path().join("typed.backup");
    let target = dir.path().join("restored");
    create(&mut db, &archive).unwrap();
    restore(&archive, &target).unwrap();
    let mut restored = Database::open(&target).unwrap();
    assert_eq!(
        restored.view().unwrap().schemas(),
        db.view().unwrap().schemas()
    );
    for name in ["typed", "items"] {
        assert_eq!(
            restored.view().unwrap().scan(name, 100).unwrap(),
            db.view().unwrap().scan(name, 100).unwrap()
        );
    }
    let mut tx = restored.begin().unwrap();
    let mut another = schema();
    another.name = "another".into();
    assert_eq!(tx.create_table(another).unwrap(), 4);
    tx.commit().unwrap();
}
