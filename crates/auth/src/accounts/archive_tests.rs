use super::*;
use emilybase_transactions::Database;

fn original(path: &Path) -> AccountStore {
    tests::raw_store(
        path,
        vec![tests::fixture_record("synthetic", [7; 16], 2).encode()],
    );
    AccountStore::open(path, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap()
}

#[test]
fn pure_private_inspection_matches_export_file_and_each_supported_inventory() {
    let _io = TEST_IO.lock().unwrap();
    for version in [1, 2, 3, 4] {
        for compacted in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let source = dir.path().join("source");
            let mut store = original(&source);
            if version == 2 {
                store.enable_session_storage().unwrap();
            } else if version >= 3 {
                store.enable_session_clock(100).unwrap();
            }
            if version == 4 {
                store.enable_row_policy_catalog().unwrap();
            }
            let token = if version >= 3 {
                Some(
                    store
                        .sign_in("synthetic", b"synthetic-password", 100)
                        .unwrap(),
                )
            } else {
                None
            };
            if compacted {
                store.compact().unwrap();
            }
            let before = store.database.committed_wal().unwrap();
            let image = store.backup_image().unwrap();
            let report = inspect_private_account_backup_bytes(&image, tests::PROJECT).unwrap();
            assert_eq!(report.private_version, version);
            assert_eq!(report.accounts, 1);
            assert_eq!(report.session_families, usize::from(version >= 3));
            assert_eq!(
                report.clock_floor,
                if version >= 3 { Some(100) } else { None }
            );
            assert_eq!(report.database.wal_version, if compacted { 2 } else { 1 });
            assert_eq!(
                report.database.tables,
                match version {
                    1 => 2,
                    2 => 4,
                    3 => 5,
                    _ => 7,
                }
            );
            let file = dir.path().join("private.backup");
            assert_eq!(store.backup(&file).unwrap(), report.database);
            assert_eq!(std::fs::read(file).unwrap(), image);
            assert_eq!(store.database.committed_wal().unwrap(), before);
            if let Some(token) = token {
                assert!(store.verify_access(token.access.expose(), 100).is_ok());
            }
            assert!(matches!(
                inspect_private_account_backup_bytes(&image, "22222222222222222222222222222222"),
                Err(Error::ScopeMismatch)
            ));
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        }
    }
}

#[test]
fn complete_private_validation_is_shared_by_open_inspection_and_both_export_paths() {
    let _io = TEST_IO.lock().unwrap();
    for defect in 0..12 {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let mut store = original(&source);
        let family = if defect >= 9 {
            store.enable_session_clock(100).unwrap();
            Some(
                store
                    .sign_in("synthetic", b"synthetic-password", 100)
                    .unwrap(),
            )
        } else {
            None
        };
        let snapshot = store.database.view().unwrap();
        let mut scope = snapshot
            .get(SCOPE, &Key::Integer(1))
            .unwrap()
            .unwrap()
            .clone();
        let mut user = snapshot
            .get(USERS, &Key::Text("synthetic".into()))
            .unwrap()
            .unwrap()
            .clone();
        let session = family.as_ref().map(|t| {
            let key = Key::Text(
                t.metadata
                    .family
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
            );
            (
                key.clone(),
                snapshot
                    .get(super::session_schema::FAMILIES, &key)
                    .unwrap()
                    .unwrap()
                    .clone(),
            )
        });
        let mut tx = store.database.begin().unwrap();
        match defect {
            0 => {
                scope[1] = Value::Integer(4);
                tx.update(SCOPE, &Key::Integer(1), scope).unwrap();
            }
            1 => {
                scope[2] = Value::Text("22222222222222222222222222222222".into());
                tx.update(SCOPE, &Key::Integer(1), scope).unwrap();
            }
            2 => {
                scope[3] = Value::Bytes(vec![]);
                tx.update(SCOPE, &Key::Integer(1), scope).unwrap();
            }
            3 => {
                user[3] = Value::Integer(0);
                tx.update(USERS, &Key::Text("synthetic".into()), user)
                    .unwrap();
            }
            4 => {
                user[2] = Value::Bytes(vec![0; 72]);
                tx.update(USERS, &Key::Text("synthetic".into()), user)
                    .unwrap();
            }
            5 => {
                user[1] = Value::Bytes(vec![7; 15]);
                tx.update(USERS, &Key::Text("synthetic".into()), user)
                    .unwrap();
            }
            6 => {
                user[0] = Value::Text("other".into());
                tx.insert(USERS, user).unwrap();
            }
            7 => {
                let mut schema = user_schema();
                schema.name = "extra".into();
                tx.create_table(schema).unwrap();
            }
            8 => {
                tx.drop_table(USERS).unwrap();
            }
            9 => {
                tx.update(
                    super::session_clock::CLOCK,
                    &Key::Integer(1),
                    vec![Value::Integer(1), Value::Integer(1), Value::Integer(0)],
                )
                .unwrap();
            }
            _ => {
                let (key, mut row) = session.unwrap();
                if defect == 10 {
                    row[2] = Value::Text("missing".into());
                } else {
                    row[4] = Value::Integer(3);
                }
                tx.update(super::session_schema::FAMILIES, &key, row)
                    .unwrap();
            }
        }
        tx.commit().unwrap();
        // These are fully checksummed, ordinary-engine-valid archives.
        let wal = store.database.committed_wal().unwrap();
        let image = emilybase_backup::encode(&wal).unwrap();
        assert!(emilybase_backup::decode_verified(&image).is_ok());
        let error = inspect_private_account_backup_bytes(&image, tests::PROJECT).unwrap_err();
        if defect == 1 {
            assert!(matches!(error, Error::ScopeMismatch));
        } else {
            assert!(matches!(error, Error::Corrupt));
        }
        assert!(store.backup_image().is_err());
        let target = dir.path().join("refused.backup");
        assert!(store.backup(&target).is_err());
        assert!(!target.exists());
        assert_eq!(store.database.committed_wal().unwrap(), wal);
        drop(store);
        assert!(
            AccountStore::open(&source, tests::PROJECT, PasswordPool::new(1).unwrap()).is_err()
        );
    }
}

#[test]
fn verified_image_is_owned_and_remains_independent_of_live_mutation_and_directory_removal() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let mut store = original(&source);
    let bytes = store.backup_image().unwrap();
    let verified = emilybase_backup::decode_verified(&bytes).unwrap();
    let row = verified
        .image()
        .snapshot
        .get(USERS, &Key::Text("synthetic".into()))
        .unwrap()
        .unwrap();
    assert_eq!(row[3], Value::Integer(2));
    assert_eq!(
        verified.report(),
        &emilybase_backup::inspect_bytes(&bytes).unwrap()
    );
    assert_eq!(verified.image().discarded_bytes, 0);
    assert_eq!(format!("{verified:?}"), "VerifiedBackup(contents redacted)");
    store.set_disabled("synthetic", true).unwrap();
    assert_eq!(
        verified
            .image()
            .snapshot
            .get(USERS, &Key::Text("synthetic".into()))
            .unwrap()
            .unwrap()[3],
        Value::Integer(2)
    );
    drop(store);
    std::fs::remove_dir_all(source).unwrap();
    drop(bytes);
    assert_eq!(verified.image().snapshot.row_count(), 2);
    assert_eq!(verified.report().rows, 2);
}

#[test]
fn private_inspection_refuses_malformed_wire_input_scope_and_generic_databases() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut store = original(&dir.path().join("source"));
    let image = store.backup_image().unwrap();
    for bad in [
        vec![],
        vec![0; 128],
        {
            let mut b = image.clone();
            b.push(0);
            b
        },
        {
            let mut b = image.clone();
            b[128] ^= 1;
            b
        },
    ] {
        assert!(inspect_private_account_backup_bytes(&bad, tests::PROJECT).is_err());
    }
    assert!(matches!(
        inspect_private_account_backup_bytes(&image, "../synthetic"),
        Err(Error::Scope)
    ));
    let mut ordinary = Database::create(dir.path().join("ordinary")).unwrap();
    let generic = emilybase_backup::encode(&ordinary.committed_wal().unwrap()).unwrap();
    assert!(matches!(
        inspect_private_account_backup_bytes(&generic, tests::PROJECT),
        Err(Error::Corrupt)
    ));
    assert_eq!(
        store.database.committed_wal().unwrap(),
        image[emilybase_backup::HEADER_SIZE..]
    );
}

#[test]
fn pure_inspection_enforces_actual_account_capacity_before_returning_metadata() {
    let _io = TEST_IO.lock().unwrap();
    for count in [MAX_ACCOUNTS, MAX_ACCOUNTS + 1] {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let rows = (0..count)
            .map(|i| {
                let mut id = [0; 16];
                id[..8].copy_from_slice(&(i as u64).to_le_bytes());
                tests::fixture_record(&format!("u{i}"), id, 1).encode()
            })
            .collect();
        tests::raw_store(&source, rows);
        let mut database = Database::open(&source).unwrap();
        database.compact().unwrap();
        let image = emilybase_backup::encode(&database.committed_wal().unwrap()).unwrap();
        let result = inspect_private_account_backup_bytes(&image, tests::PROJECT);
        if count == MAX_ACCOUNTS {
            assert_eq!(result.unwrap().accounts, count);
        } else {
            assert!(matches!(result, Err(Error::Corrupt)));
        }
    }
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn pure_inventory_matches_independent_version_account_and_clock_model(
        accounts in 0..16_usize, prior in 1..=4_u16,
        clock in 0..100000_u64, compacted in any::<bool>()
    ) {
        let _io = TEST_IO.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let rows = (0..accounts).map(|i| {
            let mut id = [0;16];
            id[0] = i as u8;
            tests::fixture_record(&format!("u{i}"), id, 1).encode()
        }).collect();
        tests::raw_store(&source, rows);
        let mut store = AccountStore::open(&source, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
        if prior == 2 { store.enable_session_storage().unwrap(); }
        else if prior >= 3 { store.enable_session_clock(clock).unwrap(); }
        if prior == 4 {store.enable_row_policy_catalog().unwrap();}
        if compacted { store.compact().unwrap(); }
        let before = store.database.committed_wal().unwrap();
        let image = store.backup_image().unwrap();
        let report = inspect_private_account_backup_bytes(&image, tests::PROJECT).unwrap();
        prop_assert_eq!(report.accounts, accounts);
        prop_assert_eq!(report.private_version, prior);
        prop_assert_eq!(report.session_families, 0);
        prop_assert_eq!(report.clock_floor, if prior >= 3 {Some(clock)} else {None});
        prop_assert_eq!(report.database.wal_version, if compacted {2} else {1});
        prop_assert_eq!(store.database.committed_wal().unwrap(), before);
    }
}

#[test]
#[ignore = "synthetic corpus helper invoked explicitly by the fuzz campaign"]
fn private_archive_corpus() {
    let _io = TEST_IO.lock().unwrap();
    let output = std::env::var_os("EMILYBASE_PRIVATE_CORPUS").unwrap();
    let output = Path::new(&output);
    std::fs::create_dir_all(output).unwrap();
    for version in [1, 2, 3, 4] {
        for compacted in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut store = original(&dir.path().join("source"));
            if version == 2 {
                store.enable_session_storage().unwrap();
            } else if version >= 3 {
                store.enable_session_clock(100).unwrap();
            }
            if version == 4 {
                store.enable_row_policy_catalog().unwrap();
                let schema = super::policy_catalog_tests::schema();
                let mut document = super::policy_catalog_tests::OWN.to_vec();
                document.resize(16_384, b' ');
                store
                    .install_row_policy(super::policy_catalog_tests::context(&schema), 0, &document)
                    .unwrap();
            }
            if compacted {
                store.compact().unwrap();
            }
            let bytes = store.backup_image().unwrap();
            std::fs::write(
                output.join(format!(
                    "private-v{version}-wal{}",
                    if compacted { 2 } else { 1 }
                )),
                bytes,
            )
            .unwrap();
        }
    }
}

#[test]
fn pure_inspection_counts_all_retained_families_and_refuses_the_4097th() {
    let _io = TEST_IO.lock().unwrap();
    for count in [MAX_SESSION_FAMILIES, MAX_SESSION_FAMILIES + 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut store = original(&dir.path().join("source"));
        let scope = store.enable_session_clock(100).unwrap();
        let first = store
            .sign_in("synthetic", b"synthetic-password", 100)
            .unwrap();
        let key = Key::Text(
            first
                .metadata
                .family
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        );
        let template = store
            .database
            .view()
            .unwrap()
            .get(super::session_schema::FAMILIES, &key)
            .unwrap()
            .unwrap()
            .clone();
        let mut tx = store.database.begin().unwrap();
        tx.delete(super::session_schema::FAMILIES, &key).unwrap();
        tx.commit().unwrap();
        for begin in (0..count).step_by(256) {
            let mut tx = store.database.begin().unwrap();
            for index in begin..(begin + 256).min(count) {
                let mut id = [0; 16];
                id[..8].copy_from_slice(&(index as u64).to_le_bytes());
                let mut row = template.clone();
                row[0] = Value::Text(id.iter().map(|b| format!("{b:02x}")).collect());
                row[11] = Value::Bytes(
                    crate::tokens::issue(crate::tokens::TokenKind::Access, &scope, id)
                        .unwrap()
                        .1
                        .encode()
                        .to_vec(),
                );
                row[12] = Value::Bytes(
                    crate::tokens::issue(crate::tokens::TokenKind::Refresh, &scope, id)
                        .unwrap()
                        .1
                        .encode()
                        .to_vec(),
                );
                row[13] = Value::Boolean(index % 2 == 0);
                tx.insert(super::session_schema::FAMILIES, row).unwrap();
            }
            tx.commit().unwrap();
        }
        store.database.compact().unwrap();
        let image = emilybase_backup::encode(&store.database.committed_wal().unwrap()).unwrap();
        let report = inspect_private_account_backup_bytes(&image, tests::PROJECT);
        if count == MAX_SESSION_FAMILIES {
            assert_eq!(report.unwrap().session_families, count);
        } else {
            assert!(matches!(report, Err(Error::Corrupt)));
        }
    }
}

#[test]
fn inventory_counts_revoked_and_obsolete_history_without_granting_or_resetting_authority() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut store = original(&dir.path().join("source"));
    store.enable_session_clock(100).unwrap();
    let first = store
        .sign_in("synthetic", b"synthetic-password", 100)
        .unwrap();
    let second = store
        .sign_in("synthetic", b"synthetic-password", 100)
        .unwrap();
    store.logout_session(first.refresh.expose(), 100).unwrap();
    let before = store.database.committed_wal().unwrap();
    let image = store.backup_image().unwrap();
    let report = inspect_private_account_backup_bytes(&image, tests::PROJECT).unwrap();
    assert_eq!(
        (
            report.accounts,
            report.session_families,
            report.database.rows
        ),
        (1, 2, 6)
    );
    assert_eq!(store.database.committed_wal().unwrap(), before);
    assert!(store.verify_access(first.access.expose(), 100).is_err());
    assert!(store.verify_access(second.access.expose(), 100).is_ok());
    store.set_disabled("synthetic", true).unwrap();
    store.reset_session_clock(50).unwrap();
    let image = store.backup_image().unwrap();
    let report = inspect_private_account_backup_bytes(&image, tests::PROJECT).unwrap();
    assert_eq!(report.session_families, 2);
    assert_eq!(report.clock_floor, Some(50));
    assert!(store.verify_access(second.access.expose(), 50).is_err());
    assert_eq!(store.prune_session_families(50, 1).unwrap(), 1);
    let image = store.backup_image().unwrap();
    let report = inspect_private_account_backup_bytes(&image, tests::PROJECT).unwrap();
    assert_eq!(report.session_families, 1);
    assert_eq!(report.database.rows, 5);
    assert_eq!(store.prune_session_families(50, 1).unwrap(), 1);
    let image = store.backup_image().unwrap();
    assert_eq!(
        inspect_private_account_backup_bytes(&image, tests::PROJECT)
            .unwrap()
            .session_families,
        0
    );
}
