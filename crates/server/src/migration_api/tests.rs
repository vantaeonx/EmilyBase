use super::*;
use axum::body::to_bytes;
use emilybase_migrations::Error as M;
use serde_json::{Value, json};

async fn value(response: Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), crate::http::MAX_BODY)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn migration_wire_has_exact_metadata_and_retry_and_failed_writes_keep_history() {
    let _serial = crate::durability::PROCESS_TESTS.lock().await;
    for format in [1, 2] {
        let directory = tempfile::tempdir().unwrap();
        let mut database = Database::create(directory.path().join("db")).unwrap();
        if format == 2 {
            database.compact().unwrap();
        }
        let before = database.committed_wal().unwrap();
        assert_eq!(
            value(list(&mut database).unwrap()).await,
            json!({"migrations":[]})
        );
        assert_eq!(database.committed_wal().unwrap(), before);
        let input = json!({"version":1,"label":"initial","sql":"CREATE TABLE t(id INT PRIMARY KEY); INSERT INTO t VALUES(1)"});
        let first = value(run(&mut database, &serde_json::to_vec(&input).unwrap()).unwrap()).await;
        assert_eq!(first["already_applied"], false);
        assert_eq!(first["receipt"]["version"], 1);
        assert_eq!(first["receipt"]["transaction"], "2");
        let digest = first["receipt"]["sha256"].as_str().unwrap();
        assert_eq!(digest.len(), 64);
        assert!(
            digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        assert!(!first.to_string().contains("CREATE"));
        let before = database.committed_wal().unwrap();
        let repeated =
            value(run(&mut database, &serde_json::to_vec(&input).unwrap()).unwrap()).await;
        assert_eq!(repeated["already_applied"], true);
        assert_eq!(repeated["receipt"], first["receipt"]);
        assert_eq!(
            value(list(&mut database).unwrap()).await,
            json!({"migrations":[first["receipt"].clone()]})
        );
        let failed = json!({"version":2,"label":"failed","sql":"CREATE TABLE next(id INT PRIMARY KEY); INSERT INTO t VALUES(1)"});
        assert!(run(&mut database, &serde_json::to_vec(&failed).unwrap()).is_err());
        assert_eq!(database.committed_wal().unwrap(), before);
        assert!(database.view().unwrap().schema("next").is_err());
    }
}

#[test]
fn bounded_wire_admission_is_strict_and_has_no_authority_or_schema_resolution() {
    for bytes in [
        b"{}".to_vec(),
        b"synthetic-sensitive-text".to_vec(),
        vec![b' '; 65537],
        vec![0xff],
        br#"{"version":1,"version":2,"label":"a","sql":"DROP TABLE t"}"#.to_vec(),
        br#"{"version":1,"label":"a","sql":"DROP TABLE t","path":"elsewhere"}"#.to_vec(),
    ] {
        assert!(validate_migration_request(&bytes).is_err());
    }
    for input in [
        json!({"version":0,"label":"a","sql":"DROP TABLE t"}),
        json!({"version":129,"label":"a","sql":"DROP TABLE t"}),
        json!({"version":1,"label":"../outside","sql":"DROP TABLE t"}),
        json!({"version":1,"label":"a","sql":"SELECT * FROM t"}),
        json!({"version":1,"label":"a","sql":"DROP TABLE _emilybase_migrations_v1"}),
        json!({"version":1,"label":"a","sql":"x".repeat(16385)}),
        json!({"version":1,"label":"a","sql":"DROP TABLE t","parameters":[]}),
    ] {
        assert!(validate_migration_request(&serde_json::to_vec(&input).unwrap()).is_err());
    }
    // Pure admission deliberately does not inspect existence or grant a project.
    assert!(
        validate_migration_request(
            br#"{"version":1,"label":"a","sql":"INSERT INTO missing VALUES(1)"}"#
        )
        .is_ok()
    );
}

#[test]
fn migration_failure_classification_never_marks_ambiguous_writes_as_safe_refusals() {
    use crate::http::Failure;
    use emilybase_query::ExecutionError as Q;
    use emilybase_transactions::Error as T;
    for error in [
        MigrationError::Document,
        MigrationError::Engine(M::Identity),
        MigrationError::Engine(M::Conflict),
        MigrationError::Engine(M::Order),
        MigrationError::Engine(M::Script),
        MigrationError::Engine(M::Execution(Q::Column)),
        MigrationError::Engine(M::Execution(Q::Transaction(T::Limit))),
        MigrationError::Engine(M::Database(emilybase_database::Error::DuplicateKey)),
    ] {
        let Failure(status, code) = Failure::from(crate::Error::Migrations(error));
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(code, "migration_rejected");
    }
    let unknown = || {
        T::Wal(emilybase_wal::Error::OutcomeUnknown {
            transaction: 3,
            source: std::io::Error::other("synthetic-sensitive-cause"),
        })
    };
    for error in [
        MigrationError::Response,
        MigrationError::Engine(M::Transaction(unknown())),
        MigrationError::Engine(M::Execution(Q::Transaction(unknown()))),
        MigrationError::Engine(M::Transaction(T::Poisoned)),
    ] {
        let Failure(status, code) = Failure::from(crate::Error::Migrations(error));
        assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(code, "transaction_outcome_requires_inspection");
        assert!(!code.contains("sensitive"));
    }
    let Failure(status, code) =
        Failure::from(crate::Error::Migrations(MigrationError::Engine(M::History)));
    assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(code, "migration_history_invalid");
}

#[tokio::test]
async fn maximum_receipt_inventory_and_full_u64_wire_fit_the_response_cap() {
    let _serial = crate::durability::PROCESS_TESTS.lock().await;
    let directory = tempfile::tempdir().unwrap();
    let mut database = Database::create(directory.path().join("db")).unwrap();
    emilybase_query::execute(&mut database, "CREATE TABLE t(id INT PRIMARY KEY)", &[]).unwrap();
    let label = "a".repeat(63);
    for version in 1..=128 {
        apply(
            &mut database,
            &prepare(version, &label, "DELETE FROM t WHERE id=-1").unwrap(),
        )
        .unwrap();
    }
    let before = database.committed_wal().unwrap();
    let response = list(&mut database).unwrap();
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    assert!(bytes.len() < 65536);
    let document: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(document["migrations"].as_array().unwrap().len(), 128);
    assert_eq!(database.committed_wal().unwrap(), before);
    let receipt = WireReceipt::from(Receipt {
        version: 128,
        label,
        sha256: [255; 32],
        transaction: u64::MAX,
    });
    let receipt = value(super::response(&receipt).unwrap()).await;
    assert_eq!(receipt["transaction"], "18446744073709551615");
    assert_eq!(receipt["sha256"], "ff".repeat(32));
}
