use std::fs;

use emilybase_backup::{Error, create, inspect};
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_transactions::Database;
use emilybase_wal::Wal;

fn table() -> Schema {
    Schema {
        name: "items".into(),
        columns: vec![Column {
            name: "id".into(),
            data_type: DataType::Integer,
            nullable: false,
        }],
        primary_key: 0,
    }
}

#[test]
fn file_backup_captures_one_commit_boundary_and_preserves_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("db");
    let backup = dir.path().join("snapshot.backup");
    let mut db = Database::create(&source).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(table()).unwrap();
    tx.insert("items", vec![Value::Integer(1)]).unwrap();
    tx.commit().unwrap();
    let source_before = fs::read(source.join("redo.wal")).unwrap();
    let report = create(&mut db, &backup).unwrap();
    assert_eq!(inspect(&backup).unwrap(), report);
    assert_eq!(fs::read(source.join("redo.wal")).unwrap(), source_before);
    let mut tx = db.begin().unwrap();
    tx.insert("items", vec![Value::Integer(2)]).unwrap();
    tx.commit().unwrap();
    assert_eq!(inspect(&backup).unwrap().rows, 1);
    assert_eq!(db.view().unwrap().row_count(), 2);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn overwrite_files_directories_and_symlinks_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::create(dir.path().join("db")).unwrap();
    let existing = dir.path().join("existing.backup");
    fs::write(&existing, b"preserve synthetic existing file").unwrap();
    assert!(create(&mut db, &existing).is_err());
    assert_eq!(
        fs::read(&existing).unwrap(),
        b"preserve synthetic existing file"
    );
    let existing_dir = dir.path().join("existing-directory");
    fs::create_dir(&existing_dir).unwrap();
    assert!(create(&mut db, &existing_dir).is_err());
    #[cfg(unix)]
    {
        let link = dir.path().join("alias.backup");
        std::os::unix::fs::symlink(&existing, &link).unwrap();
        assert!(create(&mut db, &link).is_err());
        assert_eq!(
            fs::read(existing).unwrap(),
            b"preserve synthetic existing file"
        );
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
fn abandoned_journal_tail_is_excluded_without_mutating_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("db");
    let db = Database::create(&source).unwrap();
    let id = db.database_id();
    drop(db);
    let log = source.join("redo.wal");
    let (mut wal, recovered) = Wal::open(&log, Some(id)).unwrap();
    {
        let mut pending = wal.begin(&recovered.committed[0].pages).unwrap();
        pending.sync_uncommitted().unwrap();
    }
    drop(wal);
    let before = fs::read(&log).unwrap();
    let mut db = Database::open(&source).unwrap();
    let report = create(&mut db, dir.path().join("clean.backup")).unwrap();
    assert_eq!(before.len() - report.wal_bytes, emilybase_wal::FRAME_SIZE);
    assert_eq!(fs::read(log).unwrap(), before);
}

#[test]
fn oversized_files_are_refused_before_reading_contents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oversized.backup");
    let file = fs::File::create_new(&path).unwrap();
    file.set_len(emilybase_backup::MAX_BACKUP_BYTES as u64 + 1)
        .unwrap();
    assert!(matches!(inspect(&path), Err(Error::Limit)));
}

#[cfg(unix)]
#[test]
fn backup_permissions_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private.backup");
    let mut db = Database::create(dir.path().join("db")).unwrap();
    create(&mut db, &path).unwrap();
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn external_journal_damage_prevents_export_and_poisons_the_owner() {
    for truncate in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("db");
        let target = dir.path().join("must-not-exist.backup");
        let mut db = Database::create(&source).unwrap();
        let log = source.join("redo.wal");
        let mut damaged = fs::read(&log).unwrap();
        if truncate {
            damaged.truncate(emilybase_wal::HEADER_SIZE);
        } else {
            damaged[emilybase_wal::HEADER_SIZE + 100] ^= 1;
        }
        fs::write(&log, &damaged).unwrap();
        assert!(create(&mut db, &target).is_err());
        assert!(!target.exists());
        assert_eq!(fs::read(log).unwrap(), damaged);
        assert!(matches!(
            db.view(),
            Err(emilybase_transactions::Error::Poisoned)
        ));
        assert!(matches!(
            db.begin(),
            Err(emilybase_transactions::Error::Poisoned)
        ));
    }
}
