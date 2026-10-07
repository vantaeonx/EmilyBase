//! This tests verifier bytes in the original engine, not an account/login service.
use emilybase_auth::password::{PasswordDigest, PasswordError, PasswordPool};
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::Database;

const ORIGINAL: &[u8] = b"synthetic-original-password";
const REPLACEMENT: &[u8] = b"synthetic-replacement-password";

fn schema() -> Schema {
    Schema {
        name: "synthetic_verifiers".into(),
        columns: vec![
            Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
            Column {
                name: "digest".into(),
                data_type: DataType::Bytes,
                nullable: false,
            },
        ],
        primary_key: 0,
    }
}

fn row(digest: &PasswordDigest) -> Vec<Value> {
    vec![Value::Integer(1), Value::Bytes(digest.encode().to_vec())]
}
fn read(database: &Database) -> PasswordDigest {
    let row = database
        .view()
        .unwrap()
        .get("synthetic_verifiers", &Key::Integer(1))
        .unwrap()
        .unwrap();
    let Value::Bytes(bytes) = &row[1] else {
        panic!("synthetic verifier schema changed");
    };
    PasswordDigest::decode(bytes).unwrap()
}

#[test]
fn both_journals_keep_only_committed_verifiers_through_restart_and_verified_restore() {
    let pool = PasswordPool::new(1).unwrap();
    let original = pool.hash(ORIGINAL).unwrap();
    let replacement = pool.hash(REPLACEMENT).unwrap();
    for compacted in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic");
        let mut database = Database::create(&path).unwrap();
        let mut transaction = database.begin().unwrap();
        transaction.create_table(schema()).unwrap();
        transaction
            .insert("synthetic_verifiers", row(&original))
            .unwrap();
        transaction.commit().unwrap();
        if compacted {
            database.compact().unwrap();
        }
        let before = database.committed_wal().unwrap();
        let old = database.view().unwrap().clone();
        let mut transaction = database.begin().unwrap();
        transaction
            .update("synthetic_verifiers", &Key::Integer(1), row(&replacement))
            .unwrap();
        transaction.rollback();
        assert_eq!(database.committed_wal().unwrap(), before);
        assert_eq!(read(&database).encode(), original.encode());
        drop(database);
        let mut database = Database::open(&path).unwrap();
        assert!(pool.verify(ORIGINAL, &read(&database)).unwrap());
        assert!(!pool.verify(REPLACEMENT, &read(&database)).unwrap());
        let mut transaction = database.begin().unwrap();
        transaction
            .update("synthetic_verifiers", &Key::Integer(1), row(&replacement))
            .unwrap();
        transaction.commit().unwrap();
        assert_eq!(
            old.get("synthetic_verifiers", &Key::Integer(1)).unwrap(),
            Some(&row(&original))
        );
        let wal = database.committed_wal().unwrap();
        for cleartext in [ORIGINAL, REPLACEMENT] {
            assert!(!wal.windows(cleartext.len()).any(|part| part == cleartext));
        }
        let archive = directory.path().join("synthetic.backup");
        let report = emilybase_backup::create(&mut database, &archive).unwrap();
        assert_eq!(report.wal_version, if compacted { 2 } else { 1 });
        assert_eq!(report.rows, 1);
        assert_eq!(report, emilybase_backup::inspect(&archive).unwrap());
        let restored_path = directory.path().join("restored");
        assert_eq!(
            report,
            emilybase_backup::restore(&archive, &restored_path).unwrap()
        );
        let restored = Database::open(&restored_path).unwrap();
        assert_eq!(read(&restored).encode(), replacement.encode());
        assert!(pool.verify(REPLACEMENT, &read(&restored)).unwrap());
        assert!(!pool.verify(ORIGINAL, &read(&restored)).unwrap());
        drop(database);
        let mut reopened = Database::open(&path).unwrap();
        assert_eq!(reopened.committed_wal().unwrap(), wal);
        assert_eq!(read(&reopened).encode(), replacement.encode());
    }
}

#[test]
fn opaque_storage_does_not_make_tampered_costs_a_valid_verifier() {
    let directory = tempfile::tempdir().unwrap();
    let mut database = Database::create(directory.path().join("synthetic")).unwrap();
    let pool = PasswordPool::new(1).unwrap();
    let digest = pool.hash(ORIGINAL).unwrap();
    let mut malicious = digest.encode();
    malicious[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut transaction = database.begin().unwrap();
    transaction.create_table(schema()).unwrap();
    transaction
        .insert(
            "synthetic_verifiers",
            vec![Value::Integer(1), Value::Bytes(malicious.to_vec())],
        )
        .unwrap();
    transaction.commit().unwrap();
    let row = database
        .view()
        .unwrap()
        .get("synthetic_verifiers", &Key::Integer(1))
        .unwrap()
        .unwrap();
    let Value::Bytes(bytes) = &row[1] else {
        panic!("synthetic bytes changed")
    };
    assert!(matches!(
        PasswordDigest::decode(bytes),
        Err(PasswordError::Policy)
    ));
    assert_eq!(pool.usage().workspace_bytes, 0);
    let archive = directory.path().join("synthetic.backup");
    emilybase_backup::create(&mut database, &archive).unwrap();
    let mut corrupt = std::fs::read(&archive).unwrap();
    corrupt[emilybase_backup::HEADER_SIZE + 64] ^= 1;
    assert!(emilybase_backup::inspect_bytes(&corrupt).is_err());
}
