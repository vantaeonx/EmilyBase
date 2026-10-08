use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_migrations::{Error, LEDGER_TABLE, apply, inspect, prepare};
use emilybase_transactions::Database;
use proptest::prelude::*;

const FIRST: &str =
    "CREATE TABLE t(id INT PRIMARY KEY,value TEXT); INSERT INTO t VALUES(0,'original')";

fn fixture(version: u16) -> (tempfile::TempDir, Database) {
    let directory = tempfile::tempdir().unwrap();
    let mut database = Database::create(directory.path().join("db")).unwrap();
    if version == 2 {
        database.compact().unwrap();
    }
    (directory, database)
}

fn apply_first(database: &mut Database) {
    apply(database, &prepare(1, "initial", FIRST).unwrap()).unwrap();
}

#[test]
fn ordered_receipts_and_schema_restore_together_with_exact_historical_noops() {
    for version in [1, 2] {
        let (directory, mut database) = fixture(version);
        let empty = database.committed_wal().unwrap();
        assert!(inspect(&database).unwrap().is_empty());
        assert_eq!(database.committed_wal().unwrap(), empty);
        let base = database.last_transaction();
        let first = apply(&mut database, &prepare(1, "initial", FIRST).unwrap()).unwrap();
        assert!(!first.already_applied);
        assert_eq!(first.receipt.transaction, base + 1);
        assert_eq!(inspect(&database).unwrap(), vec![first.receipt.clone()]);
        let second_sql =
            "UPDATE t SET value='migrated' WHERE id=0; CREATE TABLE next(id INT PRIMARY KEY)";
        let second = apply(
            &mut database,
            &prepare(2, "update-and-add", second_sql).unwrap(),
        )
        .unwrap();
        assert_eq!(second.receipt.transaction, base + 2);
        let before = database.committed_wal().unwrap();
        let repeated = apply(&mut database, &prepare(1, "initial", FIRST).unwrap()).unwrap();
        assert!(repeated.already_applied);
        assert_eq!(repeated.receipt, first.receipt);
        assert_eq!(database.committed_wal().unwrap(), before);
        assert_eq!(database.last_transaction(), base + 2);
        let receipts = inspect(&database).unwrap();
        assert_eq!(receipts, vec![first.receipt, second.receipt]);
        database.checkpoint().unwrap();
        let archive = directory.path().join("synthetic.backup");
        emilybase_backup::create(&mut database, &archive).unwrap();
        let restored = directory.path().join("restored");
        emilybase_backup::restore(&archive, &restored).unwrap();
        drop(database);
        let database = Database::open(directory.path().join("db")).unwrap();
        let mut copy = Database::open(restored).unwrap();
        assert_eq!(inspect(&database).unwrap(), receipts);
        assert_eq!(inspect(&copy).unwrap(), receipts);
        assert_eq!(
            copy.view()
                .unwrap()
                .get("t", &Key::Integer(0))
                .unwrap()
                .unwrap()[1],
            Value::Text("migrated".into())
        );
        assert!(copy.view().unwrap().schema("next").is_ok());
        apply(
            &mut copy,
            &prepare(3, "delete-data", "DELETE FROM t WHERE id=0").unwrap(),
        )
        .unwrap();
        assert!(
            copy.view()
                .unwrap()
                .get("t", &Key::Integer(0))
                .unwrap()
                .is_none()
        );
        assert!(
            database
                .view()
                .unwrap()
                .get("t", &Key::Integer(0))
                .unwrap()
                .is_some()
        );
    }
}

#[test]
fn skipped_changed_and_late_failed_migrations_leave_exact_committed_history() {
    for version in [1, 2] {
        let (_directory, mut database) = fixture(version);
        let empty = database.committed_wal().unwrap();
        assert!(matches!(
            apply(&mut database, &prepare(2, "skipped", FIRST).unwrap()),
            Err(Error::Order)
        ));
        assert_eq!(database.committed_wal().unwrap(), empty);
        assert!(matches!(
            apply(
                &mut database,
                &prepare(
                    1,
                    "failed",
                    "CREATE TABLE t(id INT PRIMARY KEY); INSERT INTO t VALUES(NULL)"
                )
                .unwrap()
            ),
            Err(Error::Execution(_))
        ));
        assert_eq!(database.committed_wal().unwrap(), empty);
        assert!(database.view().unwrap().schema(LEDGER_TABLE).is_err());
        apply_first(&mut database);
        let before = database.committed_wal().unwrap();
        for (label, sql) in [
            ("renamed", FIRST),
            ("initial", "CREATE TABLE t(id INT PRIMARY KEY)"),
            ("initial", &format!("{FIRST};")),
        ] {
            assert!(matches!(
                apply(&mut database, &prepare(1, label, sql).unwrap()),
                Err(Error::Conflict)
            ));
            assert_eq!(database.committed_wal().unwrap(), before);
        }
        for sql in [
            "UPDATE t SET value='partial'; INSERT INTO t VALUES(0,'duplicate')",
            "CREATE TABLE next(id INT PRIMARY KEY); INSERT INTO t VALUES(9,$1)",
            "DROP TABLE t; DROP TABLE absent",
            "UPDATE t SET id=9",
        ] {
            assert!(apply(&mut database, &prepare(2, "failed", sql).unwrap()).is_err());
            assert_eq!(database.committed_wal().unwrap(), before);
            assert_eq!(inspect(&database).unwrap().len(), 1);
            assert!(database.view().unwrap().schema("next").is_err());
            assert_eq!(
                database
                    .view()
                    .unwrap()
                    .get("t", &Key::Integer(0))
                    .unwrap()
                    .unwrap()[1],
                Value::Text("original".into())
            );
        }
    }
}

#[test]
fn definitions_reject_controls_queries_reserved_names_and_all_input_bounds() {
    for sql in [
        "SELECT * FROM t",
        "BEGIN; CREATE TABLE t(id INT PRIMARY KEY); COMMIT",
        "ROLLBACK",
        "DROP TABLE _emilybase_migrations_v1",
        "DELETE FROM \"_EMILYBASE_MIGRATIONS_V1\"",
        "CREATE TABLE _emilybase_migrations_v1(id INT PRIMARY KEY)",
        "INSERT INTO _emilybase_migrations_v1 VALUES(1)",
        "UPDATE _emilybase_migrations_v1 SET label='bad'",
    ] {
        assert!(matches!(prepare(1, "initial", sql), Err(Error::Script)));
    }
    for version in [0, 129, u32::MAX] {
        assert!(matches!(
            prepare(version, "initial", FIRST),
            Err(Error::Identity)
        ));
    }
    for label in [
        "",
        "-start",
        "a/b",
        "../outside",
        "héllo",
        "a\0",
        "two words",
        &"a".repeat(64),
    ] {
        assert!(matches!(prepare(1, label, FIRST), Err(Error::Identity)));
    }
    for sql in [
        "".to_string(),
        ";;;".into(),
        "a".repeat(16_385),
        "DROP TABLE t;".repeat(65),
    ] {
        assert!(matches!(prepare(1, "initial", &sql), Err(Error::Syntax(_))));
    }
    // A ledger-looking string is data; refusal examines parsed table targets.
    assert!(
        prepare(
            1,
            "valid-label_1",
            "INSERT INTO t VALUES(1,'DROP TABLE _emilybase_migrations_v1')"
        )
        .is_ok()
    );
}

#[test]
fn exact_sql_bytes_bind_receipts_including_comments_and_whitespace() {
    let (_directory, mut database) = fixture(1);
    apply_first(&mut database);
    let before = database.committed_wal().unwrap();
    for sql in [
        format!(" {FIRST}"),
        format!("{FIRST}\n"),
        format!("{FIRST} -- comment\n"),
    ] {
        assert!(matches!(
            apply(&mut database, &prepare(1, "initial", &sql).unwrap()),
            Err(Error::Conflict)
        ));
        assert_eq!(database.committed_wal().unwrap(), before);
    }
}

#[test]
fn metadata_capacity_is_part_of_the_same_transaction_and_never_leaves_an_empty_ledger() {
    let (_directory, mut database) = fixture(1);
    let empty = database.committed_wal().unwrap();
    // First migration needs ledger-create, user-table-create and receipt events.
    for count in [254, 255] {
        let sql = format!(
            "CREATE TABLE t(id INT PRIMARY KEY); INSERT INTO t VALUES {}",
            (0..count)
                .map(|id| format!("({id})"))
                .collect::<Vec<_>>()
                .join(",")
        );
        assert!(apply(&mut database, &prepare(1, "too-many", &sql).unwrap()).is_err());
        assert_eq!(database.committed_wal().unwrap(), empty);
        assert!(database.view().unwrap().schemas().is_empty());
    }
    let sql = format!(
        "CREATE TABLE t(id INT PRIMARY KEY); INSERT INTO t VALUES {}",
        (0..253)
            .map(|id| format!("({id})"))
            .collect::<Vec<_>>()
            .join(",")
    );
    apply(&mut database, &prepare(1, "full-first", &sql).unwrap()).unwrap();
    assert_eq!(database.view().unwrap().scan("t", 1000).unwrap().len(), 253);
    let before = database.committed_wal().unwrap();
    let sql = format!(
        "INSERT INTO t VALUES {}",
        (253..509)
            .map(|id| format!("({id})"))
            .collect::<Vec<_>>()
            .join(",")
    );
    assert!(apply(&mut database, &prepare(2, "too-many", &sql).unwrap()).is_err());
    assert_eq!(database.committed_wal().unwrap(), before);
    let sql = format!(
        "INSERT INTO t VALUES {}",
        (253..508)
            .map(|id| format!("({id})"))
            .collect::<Vec<_>>()
            .join(",")
    );
    apply(&mut database, &prepare(2, "full-next", &sql).unwrap()).unwrap();
    assert_eq!(inspect(&database).unwrap().len(), 2);
}

#[test]
fn malformed_or_tampered_ledgers_refuse_read_and_apply_without_repair() {
    let (_directory, mut database) = fixture(1);
    let mut tx = database.begin().unwrap();
    tx.create_table(Schema {
        name: LEDGER_TABLE.into(),
        primary_key: 0,
        columns: vec![Column {
            name: "id".into(),
            data_type: DataType::Integer,
            nullable: false,
        }],
    })
    .unwrap();
    tx.commit().unwrap();
    let before = database.committed_wal().unwrap();
    assert!(matches!(inspect(&database), Err(Error::History)));
    assert!(matches!(
        apply(&mut database, &prepare(1, "initial", FIRST).unwrap()),
        Err(Error::History)
    ));
    assert_eq!(database.committed_wal().unwrap(), before);
    for kind in 0..9 {
        let (_directory, mut database) = fixture(1);
        apply_first(&mut database);
        let mut tx = database.begin().unwrap();
        let mut row = tx
            .view()
            .unwrap()
            .get(LEDGER_TABLE, &Key::Integer(1))
            .unwrap()
            .unwrap()
            .clone();
        match kind {
            0 => row[1] = Value::Text("../invalid".into()),
            1 => row[2] = Value::Bytes(vec![0; 31]),
            2 => row[3] = Value::Text("0".into()),
            3 => row[3] = Value::Text("02".into()),
            4 => row[3] = Value::Text("18446744073709551616".into()),
            5 => row[3] = Value::Text("999999".into()),
            6 => {
                tx.delete(LEDGER_TABLE, &Key::Integer(1)).unwrap();
                row[0] = Value::Integer(2);
            }
            8 => row[3] = Value::Text("1".into()),
            _ => {
                tx.delete(LEDGER_TABLE, &Key::Integer(1)).unwrap();
            }
        }
        if kind < 6 || kind == 8 {
            tx.update(LEDGER_TABLE, &Key::Integer(1), row).unwrap();
        } else if kind == 6 {
            tx.insert(LEDGER_TABLE, row).unwrap();
        }
        tx.commit().unwrap();
        let before = database.committed_wal().unwrap();
        assert!(
            matches!(inspect(&database), Err(Error::History)),
            "kind {kind}"
        );
        assert!(matches!(
            apply(&mut database, &prepare(2, "next", "DELETE FROM t").unwrap()),
            Err(Error::History)
        ));
        assert_eq!(database.committed_wal().unwrap(), before);
    }
}

#[test]
fn bounded_history_accepts_128_and_refuses_extra_or_nonmonotonic_receipts() {
    let (_directory, mut database) = fixture(1);
    apply_first(&mut database);
    let mut latest = 0;
    for version in 2..=128 {
        let report = apply(
            &mut database,
            &prepare(version, "noop", "DELETE FROM t WHERE id=-1").unwrap(),
        )
        .unwrap();
        assert!(report.receipt.transaction > latest);
        latest = report.receipt.transaction;
    }
    assert_eq!(inspect(&database).unwrap().len(), 128);
    assert!(matches!(
        prepare(129, "overflow", "DELETE FROM t"),
        Err(Error::Identity)
    ));
    let mut tx = database.begin().unwrap();
    let row = vec![
        Value::Integer(129),
        Value::Text("extra".into()),
        Value::Bytes(vec![0; 32]),
        Value::Text((latest + 1).to_string()),
    ];
    tx.insert(LEDGER_TABLE, row).unwrap();
    tx.commit().unwrap();
    let before = database.committed_wal().unwrap();
    assert!(matches!(inspect(&database), Err(Error::History)));
    assert_eq!(database.committed_wal().unwrap(), before);
    let (_directory, mut database) = fixture(1);
    apply_first(&mut database);
    apply(
        &mut database,
        &prepare(2, "next", "DELETE FROM t WHERE id=-1").unwrap(),
    )
    .unwrap();
    let mut tx = database.begin().unwrap();
    let mut row = tx
        .view()
        .unwrap()
        .get(LEDGER_TABLE, &Key::Integer(2))
        .unwrap()
        .unwrap()
        .clone();
    row[3] = Value::Text("2".into());
    tx.update(LEDGER_TABLE, &Key::Integer(2), row).unwrap();
    tx.commit().unwrap();
    assert!(matches!(inspect(&database), Err(Error::History)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_apply_retry_and_failure_histories_match_a_separate_version_model(
        steps in prop::collection::vec((0u8..4,0u16..1000),1..16), compact in any::<bool>(),
    ) {
        let (directory, mut database)=fixture(if compact {2}else{1});apply_first(&mut database);
        let mut scripts=vec![FIRST.to_string()];let mut expected="original".to_string();
        for (action,value) in steps {
            let before=database.committed_wal().unwrap();
            match action {
                0=> { let sql=format!("UPDATE t SET value='{value}' WHERE id=0");let version=scripts.len() as u32+1;let report=apply(&mut database,&prepare(version,"step",&sql).unwrap()).unwrap();prop_assert!(!report.already_applied);scripts.push(sql);expected=value.to_string(); },
                1=> { let i=usize::from(value)%scripts.len();let label=if i==0 {"initial"}else{"step"};prop_assert!(apply(&mut database,&prepare(i as u32+1,label,&scripts[i]).unwrap()).unwrap().already_applied);prop_assert_eq!(database.committed_wal().unwrap(),before); },
                2=> { let sql="UPDATE t SET value='partial'; INSERT INTO t VALUES(0,'duplicate')";prop_assert!(apply(&mut database,&prepare(scripts.len() as u32+1,"failed",sql).unwrap()).is_err());prop_assert_eq!(database.committed_wal().unwrap(),before); },
                _=> { prop_assert!(matches!(apply(&mut database,&prepare(scripts.len() as u32+2,"skipped","DELETE FROM t").unwrap()),Err(Error::Order)));prop_assert_eq!(database.committed_wal().unwrap(),before); },
            }
            drop(database);database=Database::open(directory.path().join("db")).unwrap();
            let receipts=inspect(&database).unwrap();prop_assert_eq!(receipts.len(),scripts.len());
            prop_assert_eq!(&database.view().unwrap().get("t",&Key::Integer(0)).unwrap().unwrap()[1],&Value::Text(expected.clone()));
            for (i,receipt) in receipts.iter().enumerate() {prop_assert_eq!(receipt.version,i as u32+1);}
        }
    }
}

#[test]
fn receipt_digest_matches_an_independently_constructed_sha256_vector() {
    // Frozen from a separate Python hashlib/explicit-big-endian construction.
    let (_directory, mut database) = fixture(1);
    let report = apply(&mut database, &prepare(1, "initial", FIRST).unwrap()).unwrap();
    assert_eq!(
        report.receipt.sha256,
        [
            75, 116, 201, 179, 175, 84, 254, 9, 251, 16, 26, 193, 205, 79, 232, 129, 155, 243, 163,
            152, 148, 5, 30, 36, 149, 186, 170, 192, 64, 110, 174, 41
        ]
    );
}

#[test]
fn existing_data_normal_writes_and_compaction_preserve_receipts_without_reapplying_sql() {
    for format in [1, 2] {
        let (directory, mut database) = fixture(format);
        emilybase_query::execute(&mut database, FIRST, &[]).unwrap();
        let old = database.view().unwrap().clone();
        let base = database.last_transaction();
        let migration = prepare(1, "adopt", "UPDATE t SET value='migration' WHERE id=0").unwrap();
        let first = apply(&mut database, &migration).unwrap().receipt;
        assert_eq!(first.transaction, base + 1);
        emilybase_query::execute(
            &mut database,
            "UPDATE t SET value='ordinary' WHERE id=0; INSERT INTO t VALUES(1,'later')",
            &[],
        )
        .unwrap();
        let second = apply(
            &mut database,
            &prepare(2, "next", "CREATE TABLE next(id INT PRIMARY KEY)").unwrap(),
        )
        .unwrap()
        .receipt;
        assert_eq!(second.transaction, base + 3);
        let before = database.committed_wal().unwrap();
        assert!(apply(&mut database, &migration).unwrap().already_applied);
        assert_eq!(database.committed_wal().unwrap(), before);
        assert_eq!(
            database
                .view()
                .unwrap()
                .get("t", &Key::Integer(0))
                .unwrap()
                .unwrap()[1],
            Value::Text("ordinary".into())
        );
        assert_eq!(
            old.get("t", &Key::Integer(0)).unwrap().unwrap()[1],
            Value::Text("original".into())
        );
        assert!(old.schema(LEDGER_TABLE).is_err());
        database.compact().unwrap();
        assert_eq!(database.last_transaction(), base + 3);
        assert_eq!(
            inspect(&database).unwrap(),
            vec![first.clone(), second.clone()]
        );
        drop(database);
        let mut database = Database::open(directory.path().join("db")).unwrap();
        assert_eq!(inspect(&database).unwrap(), vec![first, second]);
        assert!(apply(&mut database, &migration).unwrap().already_applied);
        assert_eq!(database.last_transaction(), base + 3);
        assert_eq!(database.view().unwrap().scan("t", 10).unwrap().len(), 2);
    }
}
