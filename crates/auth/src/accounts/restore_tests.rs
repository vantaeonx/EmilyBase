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
fn each_private_version_and_wal_restores_accounts_with_new_scope_before_publication() {
    let _io = TEST_IO.lock().unwrap();
    for version in [1, 2, 3] {
        for compacted in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let source = dir.path().join("source");
            let mut store = original(&source);
            let old_scope = match version {
                2 => Some(store.enable_session_storage().unwrap()),
                3 => Some(store.enable_session_clock(100).unwrap()),
                _ => None,
            };
            let old_token = if version == 3 {
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
            let archive = dir.path().join("private.backup");
            let source_report = store.backup(&archive).unwrap();
            let source_wal = store.database.committed_wal().unwrap();
            let source_archive = std::fs::read(&archive).unwrap();
            let target = dir.path().join("installed");
            let report = restore::restore_private_with(
                &archive,
                &target,
                tests::PROJECT,
                PasswordPool::new(1).unwrap(),
                50,
                |path| {
                    assert!(!target.exists());
                    let prepared =
                        AccountStore::open(path, tests::PROJECT, PasswordPool::new(1).unwrap())
                            .unwrap();
                    assert_eq!(prepared.session_clock_floor().unwrap(), Some(50));
                    assert_ne!(prepared.session_storage_scope().unwrap(), old_scope);
                },
            )
            .unwrap();
            assert_eq!(report.database_id, source_report.database_id);
            assert_eq!(report.wal_version, source_report.wal_version);
            assert!(report.last_transaction > source_report.last_transaction);
            assert_eq!(report.tables, 5);
            let mut installed =
                AccountStore::open(&target, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
            assert_eq!(installed.session_clock_floor().unwrap(), Some(50));
            assert_eq!(
                installed
                    .check_password("synthetic", b"synthetic-password")
                    .unwrap()
                    .unwrap()
                    .id,
                [7; 16]
            );
            if let Some(token) = old_token {
                assert!(installed.verify_access(token.access.expose(), 50).is_err());
                assert!(
                    installed
                        .refresh_session(token.refresh.expose(), 50)
                        .is_err()
                );
            }
            let renewed = installed
                .sign_in("synthetic", b"synthetic-password", 50)
                .unwrap();
            assert!(installed.verify_access(renewed.access.expose(), 50).is_ok());
            assert_eq!(store.database.committed_wal().unwrap(), source_wal);
            assert_eq!(std::fs::read(archive).unwrap(), source_archive);
            drop(installed);
            let reopened =
                AccountStore::open(target, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
            assert_eq!(reopened.session_clock_floor().unwrap(), Some(50));
        }
    }
}

#[test]
fn valid_engine_archives_with_wrong_scope_or_corrupt_private_records_never_publish() {
    let _io = TEST_IO.lock().unwrap();
    for defect in 0..4 {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        if defect == 0 {
            drop(Database::create(&source).unwrap());
        } else {
            let mut row = tests::fixture_record("synthetic", [7; 16], 2).encode();
            if defect == 2 {
                row[2] = Value::Bytes(vec![]);
            }
            if defect == 3 {
                row[3] = Value::Integer(0);
            }
            tests::raw_store(&source, vec![row]);
        }
        let mut database = Database::open(&source).unwrap();
        let archive = dir.path().join("private.backup");
        emilybase_backup::create(&mut database, &archive).unwrap();
        let before = std::fs::read(&archive).unwrap();
        let target = dir.path().join("installed");
        let project = if defect == 1 {
            "22222222222222222222222222222222"
        } else {
            tests::PROJECT
        };
        let error = restore_private_accounts(
            &archive,
            &target,
            project,
            PasswordPool::new(1).unwrap(),
            50,
        )
        .unwrap_err();
        if defect == 1 {
            assert!(matches!(error, Error::ScopeMismatch));
        } else {
            assert!(matches!(error, Error::Corrupt));
        }
        assert!(!target.exists());
        assert_eq!(std::fs::read(archive).unwrap(), before);
        assert!(!std::fs::read_dir(dir.path()).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".emilybase-backup-")
        }));
    }
}

#[test]
fn invalid_trusted_time_scope_and_existing_destination_fail_without_replacement() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("installed");
    let missing = dir.path().join("absent.backup");
    assert!(matches!(
        restore_private_accounts(
            &missing,
            &target,
            "../synthetic",
            PasswordPool::new(1).unwrap(),
            50
        ),
        Err(Error::Scope)
    ));
    assert!(matches!(
        restore_private_accounts(
            &missing,
            &target,
            tests::PROJECT,
            PasswordPool::new(1).unwrap(),
            u64::MAX
        ),
        Err(Error::Clock)
    ));
    assert!(!target.exists());
    let mut store = original(&dir.path().join("source"));
    let archive = dir.path().join("private.backup");
    store.backup(&archive).unwrap();
    let before = std::fs::read(&archive).unwrap();
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("marker"), b"synthetic-existing").unwrap();
    assert!(
        restore_private_accounts(
            &archive,
            &target,
            tests::PROJECT,
            PasswordPool::new(1).unwrap(),
            50
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(target.join("marker")).unwrap(),
        b"synthetic-existing"
    );
    assert_eq!(std::fs::read(&archive).unwrap(), before);
    let alias = dir.path().join("alias");
    std::os::unix::fs::symlink(&archive, &alias).unwrap();
    assert!(
        restore_private_accounts(
            alias,
            dir.path().join("other"),
            tests::PROJECT,
            PasswordPool::new(1).unwrap(),
            50
        )
        .is_err()
    );
    assert!(!dir.path().join("other").exists());
}

#[test]
#[cfg(target_os = "linux")]
fn interrupted_private_restore_exposes_only_a_complete_invalidated_scope() {
    let _io = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for mode in ["private-restore-prepared", "private-restore-commit"] {
            let dir = tempfile::tempdir().unwrap();
            let source = dir.path().join("source");
            let mut store = original(&source);
            store.enable_session_clock(100).unwrap();
            let token = store
                .sign_in("synthetic", b"synthetic-password", 100)
                .unwrap();
            if compacted {
                store.compact().unwrap();
            }
            let archive = dir.path().join("private.backup");
            store.backup(&archive).unwrap();
            let before = std::fs::read(&archive).unwrap();
            let wal = store.database.committed_wal().unwrap();
            drop(store);
            super::recovery_tests::kill_worker_at(dir.path(), mode);
            let target = dir.path().join("restored");
            if mode.ends_with("commit") {
                let mut restored =
                    AccountStore::open(&target, tests::PROJECT, PasswordPool::new(1).unwrap())
                        .unwrap();
                assert_eq!(restored.session_clock_floor().unwrap(), Some(50));
                assert!(restored.verify_access(token.access.expose(), 50).is_err());
                assert!(
                    restored
                        .refresh_session(token.refresh.expose(), 50)
                        .is_err()
                );
                let verified = dir.path().join("verified.backup");
                restored.backup(&verified).unwrap();
                let independent = dir.path().join("independent");
                emilybase_backup::restore(verified, &independent).unwrap();
                let mut check =
                    AccountStore::open(independent, tests::PROJECT, PasswordPool::new(1).unwrap())
                        .unwrap();
                assert!(check.verify_access(token.access.expose(), 50).is_err());
            } else {
                assert!(!target.exists());
                restore_private_accounts(
                    &archive,
                    &target,
                    tests::PROJECT,
                    PasswordPool::new(1).unwrap(),
                    50,
                )
                .unwrap();
            }
            assert_eq!(std::fs::read(&archive).unwrap(), before);
            let mut unchanged =
                AccountStore::open(&source, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
            assert_eq!(unchanged.database.committed_wal().unwrap(), wal);
            assert!(unchanged.verify_access(token.access.expose(), 100).is_ok());
        }
    }
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn restored_account_state_matches_an_independent_disable_epoch_model(
        events in prop::collection::vec(any::<bool>(),0..12), prior in 1..=3_u8, now in 0..300_u64
    ) {
        let _io=TEST_IO.lock().unwrap();let dir=tempfile::tempdir().unwrap();let mut source=original(&dir.path().join("source"));
        if prior==2 {source.enable_session_storage().unwrap();}else if prior==3 {source.enable_session_clock(100).unwrap();}
        let mut disabled=false;let mut epoch=2;
        for next in events {if next!=disabled {epoch+=1;disabled=next;}source.set_disabled("synthetic",next).unwrap();}
        source.compact().unwrap();let archive=dir.path().join("private.backup");source.backup(&archive).unwrap();let target=dir.path().join("installed");
        let report=restore_private_accounts(&archive,&target,tests::PROJECT,PasswordPool::new(1).unwrap(),now).unwrap();prop_assert_eq!(report.tables,5);
        let mut restored=AccountStore::open(&target,tests::PROJECT,PasswordPool::new(1).unwrap()).unwrap();let record=restored.record("synthetic").unwrap().unwrap();
        prop_assert_eq!(record.info.disabled,disabled);prop_assert_eq!(record.info.credential_epoch,epoch);prop_assert_eq!(record.info.id,[7;16]);prop_assert_eq!(restored.session_clock_floor().unwrap(),Some(now));
        prop_assert_eq!(restored.sign_in("synthetic",b"synthetic-password",now).is_ok(),!disabled);
    }
}

#[test]
fn two_prepared_private_restorers_publish_one_complete_new_scope() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut source = original(&dir.path().join("source"));
    source.enable_session_clock(100).unwrap();
    let old = source
        .sign_in("synthetic", b"synthetic-password", 100)
        .unwrap();
    let archive = dir.path().join("private.backup");
    source.backup(&archive).unwrap();
    let before = std::fs::read(&archive).unwrap();
    let target = dir.path().join("installed");
    let (ready_scopes, results) = std::thread::scope(|scope| {
        let mut ready = Vec::new();
        let mut releases = Vec::new();
        let mut handles = Vec::new();
        for _ in 0..2 {
            let (signal, receive) = std::sync::mpsc::sync_channel(1);
            let (release, released) = std::sync::mpsc::sync_channel(1);
            let archive = &archive;
            let target = &target;
            ready.push(receive);
            releases.push(release);
            handles.push(scope.spawn(move || {
                restore::restore_private_with(
                    archive,
                    target,
                    tests::PROJECT,
                    PasswordPool::new(1).unwrap(),
                    50,
                    |path| {
                        let prepared =
                            AccountStore::open(path, tests::PROJECT, PasswordPool::new(1).unwrap())
                                .unwrap();
                        let selected = prepared.session_storage_scope().unwrap().unwrap();
                        drop(prepared);
                        signal.send(selected).unwrap();
                        released
                            .recv_timeout(std::time::Duration::from_secs(10))
                            .unwrap();
                    },
                )
            }));
        }
        let scopes: Vec<_> = ready
            .into_iter()
            .map(|r| r.recv_timeout(std::time::Duration::from_secs(10)).unwrap())
            .collect();
        for release in releases {
            release.send(()).unwrap();
        }
        (
            scopes,
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>(),
        )
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|r| r.is_err()).count(), 1);
    for scope in &ready_scopes {
        assert_ne!(Some(scope.clone()), source.session_storage_scope().unwrap());
    }
    let mut installed =
        AccountStore::open(&target, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
    assert!(ready_scopes.contains(&installed.session_storage_scope().unwrap().unwrap()));
    assert_eq!(installed.session_clock_floor().unwrap(), Some(50));
    assert!(installed.verify_access(old.access.expose(), 50).is_err());
    assert!(installed.refresh_session(old.refresh.expose(), 50).is_err());
    let result = results
        .into_iter()
        .find_map(std::result::Result::ok)
        .unwrap();
    assert_eq!(
        installed.database.last_transaction(),
        result.last_transaction
    );
    assert_eq!(std::fs::read(archive).unwrap(), before);
}
