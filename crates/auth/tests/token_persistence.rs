//! Verifier persistence only: no durable session registry, TTL or refresh protocol.
use emilybase_auth::tokens::{TokenDigest, TokenKind, TokenScope, issue};
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::Database;

fn schema() -> Schema {
    Schema {
        name: "synthetic_token_hashes".into(),
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
fn row(digest: &TokenDigest) -> Vec<Value> {
    vec![Value::Integer(1), Value::Bytes(digest.encode().to_vec())]
}
fn read(database: &Database) -> TokenDigest {
    let row = database
        .view()
        .unwrap()
        .get("synthetic_token_hashes", &Key::Integer(1))
        .unwrap()
        .unwrap();
    let Value::Bytes(bytes) = &row[1] else {
        panic!("synthetic record schema changed")
    };
    TokenDigest::decode(bytes).unwrap()
}

#[test]
fn both_wals_preserve_only_committed_hashes_and_exclude_plaintext_credentials() {
    let scope = TokenScope::new(&"11".repeat(16), [0x22; 16]).unwrap();
    let (original, original_digest) = issue(TokenKind::Refresh, &scope, [0x33; 16]).unwrap();
    let (replacement, replacement_digest) = issue(TokenKind::Refresh, &scope, [0x33; 16]).unwrap();
    for compacted in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic");
        let mut database = Database::create(&path).unwrap();
        let mut transaction = database.begin().unwrap();
        transaction.create_table(schema()).unwrap();
        transaction
            .insert("synthetic_token_hashes", row(&original_digest))
            .unwrap();
        transaction.commit().unwrap();
        if compacted {
            database.compact().unwrap();
        }
        let before = database.committed_wal().unwrap();
        let old = database.view().unwrap().clone();
        let mut transaction = database.begin().unwrap();
        transaction
            .update(
                "synthetic_token_hashes",
                &Key::Integer(1),
                row(&replacement_digest),
            )
            .unwrap();
        transaction.rollback();
        assert_eq!(database.committed_wal().unwrap(), before);
        drop(database);
        let mut database = Database::open(&path).unwrap();
        assert!(read(&database).matches(original.expose(), &scope).unwrap());
        assert!(
            !read(&database)
                .matches(replacement.expose(), &scope)
                .unwrap()
        );
        let mut transaction = database.begin().unwrap();
        transaction
            .update(
                "synthetic_token_hashes",
                &Key::Integer(1),
                row(&replacement_digest),
            )
            .unwrap();
        transaction.commit().unwrap();
        assert_eq!(
            old.get("synthetic_token_hashes", &Key::Integer(1)).unwrap(),
            Some(&row(&original_digest))
        );
        let wal = database.committed_wal().unwrap();
        for text in [original.expose(), replacement.expose()] {
            assert!(!wal.windows(text.len()).any(|part| part == text.as_bytes()));
            let secret_hex = &text.as_bytes()[38..];
            assert!(!wal.windows(secret_hex.len()).any(|part| part == secret_hex));
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
        assert_eq!(read(&restored).encode(), replacement_digest.encode());
        assert!(
            read(&restored)
                .matches(replacement.expose(), &scope)
                .unwrap()
        );
        assert!(!read(&restored).matches(original.expose(), &scope).unwrap());
        drop(database);
        let mut reopened = Database::open(&path).unwrap();
        assert_eq!(reopened.committed_wal().unwrap(), wal);
        assert!(
            read(&reopened)
                .matches(replacement.expose(), &scope)
                .unwrap()
        );
    }
}

#[test]
fn generic_restore_alone_is_not_revocation_and_new_incarnation_rejects_old_token() {
    let directory = tempfile::tempdir().unwrap();
    let scope = TokenScope::new(&"11".repeat(16), [0x22; 16]).unwrap();
    let next_scope = TokenScope::new(&"11".repeat(16), [0x23; 16]).unwrap();
    let (token, digest) = issue(TokenKind::Access, &scope, [0x33; 16]).unwrap();
    let mut database = Database::create(directory.path().join("synthetic")).unwrap();
    let mut transaction = database.begin().unwrap();
    transaction.create_table(schema()).unwrap();
    transaction
        .insert("synthetic_token_hashes", row(&digest))
        .unwrap();
    transaction.commit().unwrap();
    let archive = directory.path().join("synthetic.backup");
    emilybase_backup::create(&mut database, &archive).unwrap();
    let restored_path = directory.path().join("restored");
    emilybase_backup::restore(&archive, &restored_path).unwrap();
    let restored = Database::open(&restored_path).unwrap();
    assert!(read(&restored).matches(token.expose(), &scope).unwrap());
    assert!(
        !read(&restored)
            .matches(token.expose(), &next_scope)
            .unwrap()
    );
    // Future coordinated restore must persist this new incarnation before traffic.
    // This test demonstrates the gap; it does not implement that protocol.
    let mut corrupt = std::fs::read(&archive).unwrap();
    corrupt[emilybase_backup::HEADER_SIZE + 64] ^= 1;
    assert!(emilybase_backup::inspect_bytes(&corrupt).is_err());
}
