use super::session_schema::*;
use super::*;
use crate::tokens::{TokenDigest, TokenKind};
use emilybase_catalog::Row;
use proptest::prelude::*;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn verifier(
    kind: TokenKind,
    project: &str,
    incarnation: [u8; 16],
    family: [u8; 16],
) -> TokenDigest {
    let mut bytes = [0; 92];
    bytes[..8].copy_from_slice(b"EBSK\0\0\0\0");
    bytes[8..12].copy_from_slice(&[1, 0, if kind == TokenKind::Access { 1 } else { 2 }, 0]);
    for (i, pair) in project.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        bytes[12 + i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
    }
    bytes[28..44].copy_from_slice(&incarnation);
    bytes[44..60].copy_from_slice(&family);
    bytes[60..].fill(0x44);
    TokenDigest::decode(&bytes).unwrap()
}
fn family_row(family: [u8; 16], user: [u8; 16], epoch: i64, incarnation: [u8; 16]) -> Row {
    let project = tests::PROJECT;
    vec![
        Value::Text(hex(&family)),
        Value::Bytes(incarnation.to_vec()),
        Value::Text("synthetic".into()),
        Value::Bytes(user.to_vec()),
        Value::Integer(epoch),
        Value::Integer(1),
        Value::Integer(0),
        Value::Integer(0),
        Value::Integer(ACCESS_SECONDS),
        Value::Integer(REFRESH_SECONDS),
        Value::Integer(ABSOLUTE_SECONDS),
        Value::Bytes(
            verifier(TokenKind::Access, project, incarnation, family)
                .encode()
                .to_vec(),
        ),
        Value::Bytes(
            verifier(TokenKind::Refresh, project, incarnation, family)
                .encode()
                .to_vec(),
        ),
        Value::Boolean(false),
    ]
}
fn raw_v2(path: &Path, rows: Vec<Row>, meta: Vec<Row>) {
    tests::raw_store(
        path,
        vec![tests::fixture_record("synthetic", [7; 16], 2).encode()],
    );
    let mut db = Database::open(path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(meta_schema()).unwrap();
    tx.create_table(family_schema()).unwrap();
    for row in meta {
        tx.insert(META, row).unwrap();
    }
    let mut scope = tx
        .view()
        .unwrap()
        .get(SCOPE, &Key::Integer(1))
        .unwrap()
        .unwrap()
        .clone();
    scope[1] = Value::Integer(2);
    tx.update(SCOPE, &Key::Integer(1), scope).unwrap();
    tx.commit().unwrap();
    // Capacity fixtures obey the engine's independent transaction-event bound.
    for chunk in rows.chunks(256) {
        let mut tx = db.begin().unwrap();
        for row in chunk {
            tx.insert(FAMILIES, row.clone()).unwrap();
        }
        tx.commit().unwrap();
    }
}
fn meta(incarnation: [u8; 16]) -> Row {
    vec![
        Value::Integer(1),
        Value::Integer(1),
        Value::Bytes(incarnation.to_vec()),
    ]
}

#[test]
fn explicit_migration_is_one_commit_preserves_users_and_noop_history() {
    let _io = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic");
        tests::raw_store(
            &path,
            vec![tests::fixture_record("synthetic", [7; 16], 2).encode()],
        );
        let pool = PasswordPool::new(1).unwrap();
        let mut store = AccountStore::open(&path, tests::PROJECT, pool.clone()).unwrap();
        assert_eq!(store.session_storage_scope().unwrap(), None);
        if compacted {
            store.compact().unwrap();
        }
        let before = store.database.committed_wal().unwrap();
        let old = store.database.view().unwrap().clone();
        let scope = store.enable_session_storage().unwrap();
        assert_eq!(store.count().unwrap(), 1);
        assert_eq!(store.database.view().unwrap().table_count(), 4);
        assert_eq!(old.table_count(), 2);
        assert_eq!(old.row_count(), 2);
        assert_eq!(
            old.get(SCOPE, &Key::Integer(1)).unwrap().unwrap()[1],
            Value::Integer(1)
        );
        let after = store.database.committed_wal().unwrap();
        assert!(after.starts_with(&before));
        assert_ne!(after, before);
        assert_eq!(store.enable_session_storage().unwrap(), scope);
        assert_eq!(store.database.committed_wal().unwrap(), after);
        assert_eq!(
            store
                .check_password("synthetic", b"synthetic-password")
                .unwrap()
                .unwrap()
                .credential_epoch,
            2
        );
        drop(store);
        let mut reopened = AccountStore::open(&path, tests::PROJECT, pool.clone()).unwrap();
        assert_eq!(
            reopened.session_storage_scope().unwrap(),
            Some(scope.clone())
        );
        assert_eq!(reopened.count().unwrap(), 1);
        let new = reopened.create_user("second", b"synthetic-new").unwrap();
        assert_eq!(new.credential_epoch, 1);
        assert_eq!(reopened.count().unwrap(), 2);
        let archive = dir.path().join("synthetic.backup");
        let report = reopened.backup(&archive).unwrap();
        assert_eq!(report.tables, 4);
        assert_eq!(report.rows, 4);
        assert_eq!(report.wal_version, if compacted { 2 } else { 1 });
        let destination = dir.path().join("restored");
        assert_eq!(
            emilybase_backup::restore(&archive, &destination).unwrap(),
            report
        );
        let restored = AccountStore::open(&destination, tests::PROJECT, pool).unwrap();
        assert_eq!(restored.session_storage_scope().unwrap(), Some(scope));
        assert_eq!(restored.count().unwrap(), 2);
    }
}

#[test]
fn bounded_session_info_checks_complete_lifecycle_and_both_verifier_contexts() {
    let row = family_row([3; 16], [7; 16], 2, [9; 16]);
    let info = inspect_session_record(&row, tests::PROJECT).unwrap();
    assert_eq!(info.family, [3; 16]);
    assert_eq!(info.user, [7; 16]);
    assert_eq!(info.incarnation, [9; 16]);
    assert_eq!(info.credential_epoch, 2);
    assert_eq!(info.generation, 1);
    assert_eq!(info.created, 0);
    assert_eq!(info.issued, 0);
    assert_eq!(info.access_until, 900);
    assert_eq!(info.refresh_until, 604800);
    assert_eq!(info.absolute_until, 2592000);
    assert!(!info.revoked);
    assert_eq!(info.login, "synthetic");
    assert!(!format!("{info:?}").contains("synthetic"));
    for index in 0..row.len() {
        let mut bad = row.clone();
        bad[index] = Value::Null;
        assert!(inspect_session_record(&bad, tests::PROJECT).is_err());
    }
    for length in 0..row.len() {
        assert!(inspect_session_record(&row[..length], tests::PROJECT).is_err());
    }
    let mut bad = row.clone();
    bad.push(Value::Integer(1));
    assert!(inspect_session_record(&bad, tests::PROJECT).is_err());
    for index in [4, 5] {
        for value in [0, -1, i64::MIN] {
            let mut bad = row.clone();
            bad[index] = Value::Integer(value);
            assert!(inspect_session_record(&bad, tests::PROJECT).is_err());
        }
    }
    for index in [6, 7, 8, 9, 10] {
        let mut bad = row.clone();
        bad[index] = Value::Integer(i64::MAX);
        assert!(inspect_session_record(&bad, tests::PROJECT).is_err());
    }
    for index in [1, 3, 11, 12] {
        let mut bad = row.clone();
        bad[index] = Value::Bytes(vec![0; 1024]);
        assert!(inspect_session_record(&bad, tests::PROJECT).is_err());
    }
    for login in ["", "A", "../synthetic", "синтетический"] {
        let mut bad = row.clone();
        bad[2] = Value::Text(login.into());
        assert!(inspect_session_record(&bad, tests::PROJECT).is_err());
    }
    for family in ["", &"AA".repeat(16), &"11".repeat(17)] {
        let mut bad = row.clone();
        bad[0] = Value::Text(family.into());
        assert!(inspect_session_record(&bad, tests::PROJECT).is_err());
    }
    assert!(inspect_session_record(&row, "22222222222222222222222222222222").is_err());
    let mut bad = row.clone();
    bad.swap(11, 12);
    assert!(inspect_session_record(&bad, tests::PROJECT).is_err());
    for index in [11, 12] {
        for offset in [0, 8, 10, 11, 12, 28, 44] {
            let mut bad = row.clone();
            let Value::Bytes(bytes) = &mut bad[index] else {
                unreachable!()
            };
            bytes[offset] ^= 1;
            assert!(inspect_session_record(&bad, tests::PROJECT).is_err());
        }
    }
    // Opaque hash bytes are not a standalone MAC; no secret is checked here.
    let mut changed = row;
    let Value::Bytes(bytes) = &mut changed[11] else {
        unreachable!()
    };
    bytes[60] ^= 1;
    assert!(inspect_session_record(&changed, tests::PROJECT).is_ok());
}

#[test]
fn exact_expiry_clipping_near_i64_max_is_valid_without_overflow() {
    let mut row = family_row([3; 16], [7; 16], 1, [9; 16]);
    row[6] = Value::Integer(i64::MAX - ABSOLUTE_SECONDS);
    row[7] = Value::Integer(i64::MAX - 1);
    for index in [8, 9, 10] {
        row[index] = Value::Integer(i64::MAX);
    }
    assert!(inspect_session_record(&row, tests::PROJECT).is_ok());
}

#[test]
fn version_two_validates_metadata_references_and_permits_invalidated_history() {
    let _io = TEST_IO.lock().unwrap();
    let pool = PasswordPool::new(1).unwrap();
    let current = [9; 16];
    for old_epoch in [1, 2] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic");
        // Older incarnation/epoch records remain history, never current authority.
        raw_v2(
            &path,
            vec![family_row([3; 16], [7; 16], old_epoch, [8; 16])],
            vec![meta(current)],
        );
        let store = AccountStore::open(&path, tests::PROJECT, pool.clone()).unwrap();
        assert_eq!(store.count().unwrap(), 1);
        assert_eq!(store.database.view().unwrap().row_count(), 4);
        assert_eq!(
            store.session_storage_scope().unwrap(),
            Some(crate::tokens::TokenScope::new(tests::PROJECT, current).unwrap())
        );
    }
    for variant in 0..8 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic");
        let mut row = family_row([3; 16], [7; 16], 2, current);
        match variant {
            0 => row[2] = Value::Text("absent".into()),
            1 => row[3] = Value::Bytes(vec![8; 16]),
            2 => row[4] = Value::Integer(3),
            3 => row[5] = Value::Integer(0),
            4 => row[8] = Value::Integer(901),
            5 => row[10] = Value::Integer(2591999),
            6 => {
                row[11] = Value::Bytes(
                    verifier(TokenKind::Refresh, tests::PROJECT, current, [3; 16])
                        .encode()
                        .to_vec(),
                )
            }
            _ => {
                row[12] = Value::Bytes(
                    verifier(TokenKind::Refresh, tests::PROJECT, current, [4; 16])
                        .encode()
                        .to_vec(),
                )
            }
        }
        raw_v2(&path, vec![row], vec![meta(current)]);
        assert!(
            matches!(
                AccountStore::open(&path, tests::PROJECT, pool.clone()),
                Err(Error::Corrupt)
            ),
            "variant {variant}"
        );
    }
}

#[test]
fn version_two_requires_exact_metadata_schema_and_one_fixed_row() {
    let _io = TEST_IO.lock().unwrap();
    let pool = PasswordPool::new(1).unwrap();
    for variant in 0..9 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic");
        let mut rows = vec![meta([9; 16])];
        match variant {
            0 => rows.clear(),
            1 => rows[0][0] = Value::Integer(2),
            2 => rows[0][1] = Value::Integer(0),
            3 => rows[0][1] = Value::Integer(2),
            4 => rows[0][2] = Value::Bytes(vec![0; 15]),
            5 => rows[0][2] = Value::Bytes(vec![0; 17]),
            6 => {
                let mut second = meta([8; 16]);
                second[0] = Value::Integer(2);
                rows.push(second);
            }
            _ => {}
        }
        raw_v2(&path, Vec::new(), rows);
        if variant >= 7 {
            let mut db = Database::open(&path).unwrap();
            let mut tx = db.begin().unwrap();
            if variant == 7 {
                tx.drop_table(FAMILIES).unwrap();
            } else {
                tx.create_table(super::records::schema(
                    "foreign",
                    &[("id", emilybase_catalog::DataType::Integer)],
                ))
                .unwrap();
            }
            tx.commit().unwrap();
        }
        assert!(
            matches!(
                AccountStore::open(&path, tests::PROJECT, pool.clone()),
                Err(Error::Corrupt)
            ),
            "variant {variant}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn bounded_lifecycle_matches_independent_clipped_deadline_model(created in 0..=i64::MAX-ABSOLUTE_SECONDS, elapsed in 0..ABSOLUTE_SECONDS, epoch in 1..=i64::MAX,generation in 1..=i64::MAX,revoked in any::<bool>()) {
        let issued=created+elapsed;
        let absolute=created+ABSOLUTE_SECONDS;
        let access=issued+ACCESS_SECONDS.min(absolute-issued);
        let refresh=issued+REFRESH_SECONDS.min(absolute-issued);
        let mut row=family_row([3;16],[7;16],epoch,[9;16]);
        for (index,value) in [(5,generation),(6,created),(7,issued),(8,access),(9,refresh),(10,absolute)] {row[index]=Value::Integer(value);}
        row[13]=Value::Boolean(revoked);
        let info=inspect_session_record(&row,tests::PROJECT).unwrap();
        prop_assert_eq!(info.issued,issued as u64);prop_assert_eq!(info.absolute_until,absolute as u64);
        prop_assert_eq!(info.access_until,access as u64);prop_assert_eq!(info.refresh_until,refresh as u64);
        prop_assert_eq!(info.generation,generation as u64);prop_assert_eq!(info.credential_epoch,epoch as u64);prop_assert_eq!(info.revoked,revoked);
        let mut bad=row.clone();bad[8]=Value::Integer(access-1);prop_assert!(inspect_session_record(&bad,tests::PROJECT).is_err());
        let mut bad=row;bad[9]=Value::Integer(refresh-1);prop_assert!(inspect_session_record(&bad,tests::PROJECT).is_err());
    }
}

#[test]
#[cfg(target_os = "linux")]
fn killed_migration_is_either_original_version_or_fully_acknowledged_version() {
    let _io = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for mode in ["migration-stage", "migration-commit"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("synthetic");
            tests::raw_store(
                &path,
                vec![tests::fixture_record("synthetic", [7; 16], 2).encode()],
            );
            let pool = PasswordPool::new(1).unwrap();
            let mut before = AccountStore::open(&path, tests::PROJECT, pool.clone()).unwrap();
            if compacted {
                before.compact().unwrap();
            }
            let wal = before.database.committed_wal().unwrap();
            drop(before);
            super::recovery_tests::kill_worker_at(&path, mode);
            let mut restored = AccountStore::open(&path, tests::PROJECT, pool).unwrap();
            let committed = mode == "migration-commit";
            assert_eq!(
                restored.session_storage_scope().unwrap().is_some(),
                committed
            );
            assert_eq!(
                restored.database.view().unwrap().table_count(),
                if committed { 4 } else { 2 }
            );
            assert_eq!(
                restored.database.view().unwrap().row_count(),
                if committed { 3 } else { 2 }
            );
            assert_eq!(restored.count().unwrap(), 1);
            if !committed {
                assert_eq!(restored.database.committed_wal().unwrap(), wal);
            }
            assert_eq!(
                restored
                    .record("synthetic")
                    .unwrap()
                    .unwrap()
                    .info
                    .credential_epoch,
                2
            );
            let archive = dir.path().join("recovered.backup");
            restored.backup(&archive).unwrap();
            let destination = dir.path().join("verified");
            emilybase_backup::restore(&archive, &destination).unwrap();
            let verified =
                AccountStore::open(&destination, tests::PROJECT, PasswordPool::new(1).unwrap())
                    .unwrap();
            assert_eq!(
                verified.session_storage_scope().unwrap(),
                restored.session_storage_scope().unwrap()
            );
            assert_eq!(verified.count().unwrap(), 1);
        }
    }
}

#[test]
fn actual_family_capacity_and_restored_inventory_do_not_inflate_account_count() {
    let _io = TEST_IO.lock().unwrap();
    let pool = PasswordPool::new(1).unwrap();
    for count in [MAX_SESSION_FAMILIES, MAX_SESSION_FAMILIES + 1] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic");
        let rows = (0..count)
            .map(|i| {
                let mut family = [0; 16];
                family[..8].copy_from_slice(&(i as u64).to_le_bytes());
                family_row(family, [7; 16], 2, [9; 16])
            })
            .collect();
        raw_v2(&path, rows, vec![meta([9; 16])]);
        let opened = AccountStore::open(&path, tests::PROJECT, pool.clone());
        if count > MAX_SESSION_FAMILIES {
            assert!(matches!(opened, Err(Error::Corrupt)));
            continue;
        }
        let mut store = opened.unwrap();
        assert_eq!(store.count().unwrap(), 1);
        let original = store
            .database
            .view()
            .unwrap()
            .primary_rows(FAMILIES, None, None)
            .unwrap()
            .map(|r| r.unwrap().clone())
            .collect::<Vec<_>>();
        assert_eq!(original.len(), count);
        store.set_disabled("synthetic", true).unwrap();
        assert_eq!(store.count().unwrap(), 1);
        store.compact().unwrap();
        let archive = dir.path().join("synthetic.backup");
        let report = store.backup(&archive).unwrap();
        assert_eq!(report.rows, count + 3);
        let destination = dir.path().join("restored");
        emilybase_backup::restore(&archive, &destination).unwrap();
        let restored = AccountStore::open(&destination, tests::PROJECT, pool.clone()).unwrap();
        assert_eq!(restored.count().unwrap(), 1);
        let rows = restored
            .database
            .view()
            .unwrap()
            .primary_rows(FAMILIES, None, None)
            .unwrap()
            .map(|r| r.unwrap().clone())
            .collect::<Vec<_>>();
        assert_eq!(rows, original);
    }
}
