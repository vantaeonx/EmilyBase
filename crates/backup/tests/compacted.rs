use std::fs;

use emilybase_backup::{HEADER_SIZE, create, inspect, inspect_bytes, restore};
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::Database;
use sha2::{Digest, Sha256};

fn initialized(path: &std::path::Path) -> Database {
    let mut db = Database::create(path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(Schema {
        name: "items".into(),
        columns: vec![Column {
            name: "id".into(),
            data_type: DataType::Integer,
            nullable: false,
        }],
        primary_key: 0,
    })
    .unwrap();
    tx.insert("items", vec![Value::Integer(7)]).unwrap();
    tx.commit().unwrap();
    db
}

#[test]
fn archives_of_both_wal_versions_restore_independently_after_explicit_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = initialized(&dir.path().join("source"));
    let old = dir.path().join("old.backup");
    let new = dir.path().join("new.backup");
    let old_report = create(&mut db, &old).unwrap();
    let old_bytes = fs::read(&old).unwrap();
    db.compact().unwrap();
    let mut tx = db.begin().unwrap();
    tx.insert("items", vec![Value::Integer(8)]).unwrap();
    tx.commit().unwrap();
    let new_report = create(&mut db, &new).unwrap();
    assert_eq!(old_report.wal_version, 1);
    assert_eq!(new_report.wal_version, 2);
    assert_eq!(old_report.database_id, new_report.database_id);
    assert_eq!(new_report.last_transaction, old_report.last_transaction + 1);
    for (archive, report, rows) in [(&old, &old_report, 1), (&new, &new_report, 2)] {
        let destination = dir.path().join(format!("restored-{rows}"));
        assert_eq!(&restore(archive, &destination).unwrap(), report);
        let mut restored = Database::open(destination).unwrap();
        assert_eq!(restored.view().unwrap().row_count(), rows);
        assert_eq!(restored.last_transaction(), report.last_transaction);
        let mut tx = restored.begin().unwrap();
        tx.insert("items", vec![Value::Integer(9)]).unwrap();
        assert_eq!(tx.commit().unwrap(), report.last_transaction + 1);
    }
    assert_eq!(inspect(old).unwrap(), old_report);
    assert_eq!(fs::read(dir.path().join("old.backup")).unwrap(), old_bytes);
    assert!(
        db.view()
            .unwrap()
            .get("items", &Key::Integer(9))
            .unwrap()
            .is_none()
    );
}

#[test]
fn truncating_a_baseline_and_rehashing_envelope_cannot_publish_restore() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = initialized(&dir.path().join("source"));
    db.compact().unwrap();
    let path = dir.path().join("new.backup");
    create(&mut db, &path).unwrap();
    let mut bytes = fs::read(&path).unwrap();
    bytes.pop();
    let payload_len = bytes.len() - HEADER_SIZE;
    bytes[40..48].copy_from_slice(&(payload_len as u64).to_le_bytes());
    let digest = Sha256::digest(&bytes[HEADER_SIZE..]);
    bytes[48..80].copy_from_slice(&digest);
    let checksum = crc32fast::hash(&bytes[..124]);
    bytes[124..128].copy_from_slice(&checksum.to_le_bytes());
    assert!(inspect_bytes(&bytes).is_err());
    fs::write(&path, &bytes).unwrap();
    let destination = dir.path().join("restored");
    assert!(restore(&path, &destination).is_err());
    assert!(!destination.exists());
}

#[test]
fn valid_envelope_crc_cannot_mislabel_embedded_wal_version() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = initialized(&dir.path().join("source"));
    db.compact().unwrap();
    let path = dir.path().join("new.backup");
    create(&mut db, &path).unwrap();
    let mut bytes = fs::read(path).unwrap();
    bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
    let checksum = crc32fast::hash(&bytes[..124]);
    bytes[124..128].copy_from_slice(&checksum.to_le_bytes());
    assert!(inspect_bytes(&bytes).is_err());
}
