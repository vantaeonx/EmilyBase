use emilybase_backup::{Error, HEADER_SIZE, encode, inspect_bytes};
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_transactions::Database;
use sha2::{Digest, Sha256};

fn wal() -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::create(dir.path().join("db")).unwrap();
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
    db.committed_wal().unwrap()
}

fn fix_header(bytes: &mut [u8]) {
    let crc = crc32fast::hash(&bytes[..124]);
    bytes[124..128].copy_from_slice(&crc.to_le_bytes());
}

#[test]
fn archive_has_explicit_versions_identity_commit_boundary_and_hash() {
    let wal = wal();
    let bytes = encode(&wal).unwrap();
    assert_eq!(&bytes[..16], b"EMILYBAK\x01\0\x80\0\x01\0\x01\0");
    assert_eq!(&bytes[HEADER_SIZE..], wal);
    assert_eq!(&bytes[16..32], &wal[16..32]);
    assert_eq!(&bytes[48..80], Sha256::digest(&wal).as_slice());
    let report = inspect_bytes(&bytes).unwrap();
    assert_eq!(report.last_transaction, 2);
    assert_eq!(report.tables, 1);
    assert_eq!(report.rows, 1);
    assert_eq!(report.pages, 1);
    assert_eq!(report.wal_bytes, wal.len());
}

#[test]
fn every_single_byte_change_and_truncation_is_detected() {
    let original = encode(&wal()).unwrap();
    for offset in 0..original.len() {
        let mut changed = original.clone();
        changed[offset] ^= 1;
        assert!(inspect_bytes(&changed).is_err(), "byte {offset}");
    }
    for length in 0..original.len() {
        assert!(
            inspect_bytes(&original[..length]).is_err(),
            "length {length}"
        );
    }
    let mut trailing = original;
    trailing.push(0);
    assert!(inspect_bytes(&trailing).is_err());
}

#[test]
fn valid_header_crc_does_not_bypass_version_identity_or_boundary_checks() {
    let original = encode(&wal()).unwrap();
    for (offset, value) in [
        (8, 2),
        (10, 1),
        (12, 2),
        (14, 2),
        (16, 0),
        (32, 99),
        (40, 0),
        (48, 1),
        (80, 1),
    ] {
        let mut changed = original.clone();
        if offset == 16 {
            changed[16..32].fill(0);
        } else if offset == 40 {
            changed[40..48].fill(0);
        } else if offset == 48 {
            changed[offset] ^= 1;
        } else {
            changed[offset] = value;
        }
        fix_header(&mut changed);
        assert!(inspect_bytes(&changed).is_err(), "field {offset}");
    }
    let mut changed = original;
    changed[16] ^= 1;
    fix_header(&mut changed);
    assert!(inspect_bytes(&changed).is_err());
}

#[test]
fn recomputed_archive_hash_still_requires_valid_nested_journal_checksums() {
    let mut bytes = encode(&wal()).unwrap();
    bytes[HEADER_SIZE + 100] ^= 1;
    let digest = Sha256::digest(&bytes[HEADER_SIZE..]);
    bytes[48..80].copy_from_slice(&digest);
    fix_header(&mut bytes);
    assert!(matches!(inspect_bytes(&bytes), Err(Error::Transactions(_))));
}

#[test]
fn uncommitted_tail_is_not_an_acceptable_archive_payload() {
    let mut wal = wal();
    wal.extend_from_slice(b"partial uncommitted frame");
    assert!(matches!(
        encode(&wal),
        Err(Error::Format("backup contains an uncommitted tail"))
    ));
}
