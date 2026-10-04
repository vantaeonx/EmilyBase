use emilybase_transactions::{Database, Error};
use emilybase_wal::Wal;

#[test]
fn stable_directory_owner_excludes_reopen_after_wal_inode_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let owner = Database::create(&path).unwrap();
    let id = owner.database_id();
    let replacement = path.join("replacement.wal");
    let pages = owner.view().unwrap().pages().cloned().collect::<Vec<_>>();
    drop(Wal::create_snapshot(&replacement, id, 1, &pages).unwrap());
    std::fs::rename(replacement, path.join("redo.wal")).unwrap();
    assert!(matches!(
        Database::open(&path),
        Err(Error::Wal(emilybase_wal::Error::Busy))
    ));
    drop(owner);
    let reopened = Database::open_bound(path, Some(id)).unwrap();
    assert_eq!(reopened.last_transaction(), 1);
    assert_eq!(reopened.view().unwrap().row_count(), 0);
}

#[test]
fn rejected_identity_releases_directory_and_journal_ownership_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    let id = db.database_id();
    let before = db.committed_wal().unwrap();
    drop(db);
    let mut foreign = id;
    foreign[0] ^= 1;
    if foreign == [0; 16] {
        foreign[1] = 1;
    }
    for _ in 0..3 {
        assert!(matches!(
            Database::open_bound(&path, Some(foreign)),
            Err(Error::Wal(emilybase_wal::Error::Identity))
        ));
        assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
        let mut db = Database::open_bound(&path, Some(id)).unwrap();
        assert_eq!(db.committed_wal().unwrap(), before);
    }
}

#[test]
fn failed_relational_replay_releases_ownership_for_operator_repair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    let id = db.database_id();
    let before = db.committed_wal().unwrap();
    drop(db);
    let mut bad = emilybase_storage::Page::new(1).unwrap();
    bad.insert(b"invalid table root").unwrap();
    let bytes = emilybase_wal::encode_snapshot(id, 1, &[bad]).unwrap();
    std::fs::write(path.join("redo.wal"), &bytes).unwrap();
    assert!(Database::open(&path).is_err());
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), bytes);
    // Explicit synthetic repair; opening itself never substitutes another file.
    std::fs::write(path.join("redo.wal"), &before).unwrap();
    let mut db = Database::open_bound(path, Some(id)).unwrap();
    assert_eq!(db.committed_wal().unwrap(), before);
}

#[test]
fn checkpoint_cannot_remove_or_replace_files_in_a_substituted_database_directory() {
    use emilybase_catalog::{Column, DataType, Schema, Value};
    use std::fs;

    for compact in [false, true] {
        for replacement in [true, false] {
            let temporary = tempfile::tempdir().unwrap();
            let path = temporary.path().join("database");
            let moved = temporary.path().join("owned-database");
            let mut database = Database::create(&path).unwrap();
            let mut transaction = database.begin().unwrap();
            transaction
                .create_table(Schema {
                    name: "items".into(),
                    columns: vec![Column {
                        name: "id".into(),
                        data_type: DataType::Integer,
                        nullable: false,
                    }],
                    primary_key: 0,
                })
                .unwrap();
            transaction
                .insert("items", vec![Value::Integer(7)])
                .unwrap();
            transaction.commit().unwrap();
            if compact {
                database.compact().unwrap();
            }
            let id = database.database_id();
            let before = database.committed_wal().unwrap();
            let selected_pages = database
                .view()
                .unwrap()
                .pages()
                .cloned()
                .collect::<Vec<_>>();
            fs::rename(&path, &moved).unwrap();
            if replacement {
                fs::create_dir(&path).unwrap();
                fs::write(path.join("checkpoint-next.emily"), b"foreign pending file").unwrap();
                fs::write(path.join("checkpoint.emily"), b"foreign selected file").unwrap();
                fs::write(path.join("redo.wal"), b"foreign WAL").unwrap();
            }
            database.checkpoint().unwrap();
            assert_eq!(database.committed_wal().unwrap(), before);
            assert_eq!(fs::read(moved.join("redo.wal")).unwrap(), before);
            assert!(!moved.join("checkpoint-next.emily").exists());
            let mut cache = emilybase_storage::Pager::open(moved.join("checkpoint.emily")).unwrap();
            assert_eq!(cache.page_count(), selected_pages.len() as u64);
            for page in &selected_pages {
                assert_eq!(cache.read_page(page.id()).unwrap(), *page);
            }
            if replacement {
                assert_eq!(
                    fs::read(path.join("checkpoint-next.emily")).unwrap(),
                    b"foreign pending file"
                );
                assert_eq!(
                    fs::read(path.join("checkpoint.emily")).unwrap(),
                    b"foreign selected file"
                );
                assert_eq!(fs::read(path.join("redo.wal")).unwrap(), b"foreign WAL");
                assert_eq!(fs::read_dir(&path).unwrap().count(), 3);
            } else {
                assert!(!path.exists());
            }
            drop(cache);
            drop(database);
            let reopened = Database::open_bound(&moved, Some(id)).unwrap();
            assert_eq!(
                reopened
                    .view()
                    .unwrap()
                    .pages()
                    .cloned()
                    .collect::<Vec<_>>(),
                selected_pages
            );
            assert_eq!(reopened.last_transaction(), 2);
        }
    }
}
