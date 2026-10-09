use super::public_admission::{TABLE, schema, validate};
use super::tests::{PROJECT, fixture_record, raw_store};
use super::*;
use proptest::prelude::*;

fn create(path: &Path, version: u16, compacted: bool) -> AccountStore {
    raw_store(path, vec![fixture_record("synthetic", [7; 16], 2).encode()]);
    let mut store = AccountStore::open(path, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
    if version == 2 {
        store.enable_session_storage().unwrap();
    }
    if version >= 3 {
        store.enable_session_clock(100).unwrap();
    }
    if version >= 4 {
        store.enable_row_policy_catalog().unwrap();
    }
    if version == 5 {
        store.enable_public_admission_catalog().unwrap();
    }
    if compacted {
        store.compact().unwrap();
    }
    store
}
#[test]
fn v4_migration_is_explicit_closed_and_preserves_current_users_policies_and_sessions() {
    let _io = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private");
        let mut store = create(&path, 4, compacted);
        let schema = super::policy_catalog_tests::schema();
        let installed = store
            .install_row_policy(
                super::policy_catalog_tests::context(&schema),
                0,
                super::policy_catalog_tests::OWN,
            )
            .unwrap();
        let token = store
            .sign_in("synthetic", b"synthetic-password", 100)
            .unwrap();
        let scope = store.session_storage_scope().unwrap();
        let next = store.database.last_transaction() + 1;
        let receipt = store.enable_public_admission_catalog().unwrap();
        assert_eq!(
            receipt,
            PublicAdmissionReceipt {
                enabled: false,
                revision: next,
                previous: 0
            }
        );
        assert_eq!(store.database.last_transaction(), next);
        assert_eq!(store.session_storage_scope().unwrap(), scope);
        assert_eq!(store.session_clock_floor().unwrap(), Some(100));
        assert_eq!(store.row_policy_receipts().unwrap(), vec![installed]);
        assert!(store.verify_access(token.access.expose(), 100).is_ok());
        let before = store.database.committed_wal().unwrap();
        assert_eq!(store.enable_public_admission_catalog().unwrap(), receipt);
        store.enable_row_policy_catalog().unwrap();
        store.enable_session_clock(100).unwrap();
        store.enable_session_storage().unwrap();
        assert_eq!(store.database.committed_wal().unwrap(), before);
        let bytes = store.backup_image().unwrap();
        let report = inspect_private_account_backup_bytes(&bytes, PROJECT).unwrap();
        assert_eq!(report.private_version, 5);
        assert_eq!(report.database.tables, 8);
        assert_eq!(report.database.wal_version, if compacted { 2 } else { 1 });
        drop(store);
        let mut store = AccountStore::open(&path, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
        assert_eq!(store.public_admission().unwrap(), receipt);
        assert!(store.verify_access(token.access.expose(), 100).is_ok());
        assert!(
            store
                .verify_row_policy_access(token.access.expose(), 100, 7)
                .is_ok()
        );
        assert!(AccountStore::open(&path, PROJECT, PasswordPool::new(1).unwrap()).is_err());
    }
}
#[test]
fn old_versions_refuse_admission_without_writes_or_automatic_migration() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    for version in 1..=3 {
        let mut store = create(&dir.path().join(version.to_string()), version, false);
        let before = store.database.committed_wal().unwrap();
        assert!(matches!(
            store.public_admission(),
            Err(Error::AdmissionSchema)
        ));
        assert!(matches!(
            store.set_public_admission(0, true),
            Err(Error::AdmissionSchema)
        ));
        assert!(matches!(
            store.enable_public_admission_catalog(),
            Err(Error::AdmissionSchema)
        ));
        assert_eq!(store.database.committed_wal().unwrap(), before);
        let bytes = store.backup_image().unwrap();
        assert_eq!(
            inspect_private_account_backup_bytes(&bytes, PROJECT)
                .unwrap()
                .private_version,
            version
        );
    }
}
#[test]
fn exact_retry_cas_and_aba_never_reopen_from_a_stale_receipt() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut store = create(&dir.path().join("private"), 5, false);
    let closed = store.public_admission().unwrap();
    let before = store.database.committed_wal().unwrap();
    assert_eq!(store.set_public_admission(0, false).unwrap(), closed);
    assert!(matches!(
        store.set_public_admission(0, true),
        Err(Error::AdmissionConflict)
    ));
    assert_eq!(store.database.committed_wal().unwrap(), before);
    store.advance_session_clock(101).unwrap();
    let next = store.database.last_transaction() + 1;
    let open = store.set_public_admission(closed.revision, true).unwrap();
    assert_eq!(open.revision, next);
    assert_eq!(open.previous, closed.revision);
    store.set_disabled("synthetic", true).unwrap();
    let before = store.database.committed_wal().unwrap();
    assert_eq!(
        store.set_public_admission(closed.revision, true).unwrap(),
        open
    );
    assert_eq!(
        store.set_public_admission(open.revision, true).unwrap(),
        open
    );
    assert_eq!(store.database.committed_wal().unwrap(), before);
    let closed_again = store.set_public_admission(open.revision, false).unwrap();
    assert!(matches!(
        store.set_public_admission(closed.revision, true),
        Err(Error::AdmissionConflict)
    ));
    assert!(matches!(
        store.set_public_admission(open.revision, true),
        Err(Error::AdmissionConflict)
    ));
    let open_again = store
        .set_public_admission(closed_again.revision, true)
        .unwrap();
    let before = store.database.committed_wal().unwrap();
    assert!(matches!(
        store.set_public_admission(open.revision, true),
        Err(Error::AdmissionConflict)
    ));
    assert_eq!(store.database.committed_wal().unwrap(), before);
    assert_eq!(store.enable_public_admission_catalog().unwrap(), open_again);
    assert_eq!(store.database.committed_wal().unwrap(), before);
}
#[test]
fn restore_and_operator_reset_close_enabled_admission_in_the_session_reset_commit() {
    let _io = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for from_bytes in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("private");
            let mut store = create(&path, 5, compacted);
            let schema = super::policy_catalog_tests::schema();
            let policy = store
                .install_row_policy(
                    super::policy_catalog_tests::context(&schema),
                    0,
                    super::policy_catalog_tests::OWN,
                )
                .unwrap();
            let token = store
                .sign_in("synthetic", b"synthetic-password", 100)
                .unwrap();
            let receipt = store
                .set_public_admission(store.public_admission().unwrap().revision, true)
                .unwrap();
            let old_scope = store.session_storage_scope().unwrap();
            let backup = dir.path().join("private.backup");
            let source = store.backup(&backup).unwrap();
            let bytes = std::fs::read(&backup).unwrap();
            let wal = store.database.committed_wal().unwrap();
            let target = dir.path().join("restored");
            let prepared = |path: &Path| {
                assert!(!target.exists());
                let mut copy =
                    AccountStore::open(path, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
                assert_eq!(
                    copy.public_admission().unwrap(),
                    PublicAdmissionReceipt {
                        enabled: false,
                        revision: source.last_transaction + 1,
                        previous: receipt.revision
                    }
                );
                assert_ne!(copy.session_storage_scope().unwrap(), old_scope);
                assert_eq!(copy.session_clock_floor().unwrap(), Some(50));
                assert!(copy.verify_access(token.access.expose(), 50).is_err());
            };
            let result = if from_bytes {
                restore::restore_private_bytes_with(
                    &bytes,
                    &target,
                    PROJECT,
                    PasswordPool::new(1).unwrap(),
                    50,
                    prepared,
                )
            } else {
                restore::restore_private_with(
                    &backup,
                    &target,
                    PROJECT,
                    PasswordPool::new(1).unwrap(),
                    50,
                    prepared,
                )
            }
            .unwrap();
            assert_eq!(result.last_transaction, source.last_transaction + 1);
            let mut copy =
                AccountStore::open(&target, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
            assert_eq!(copy.row_policy_receipts().unwrap(), vec![policy]);
            assert_eq!(
                copy.check_password("synthetic", b"synthetic-password")
                    .unwrap()
                    .unwrap()
                    .id,
                [7; 16]
            );
            assert!(copy.refresh_session(token.refresh.expose(), 50).is_err());
            let fresh = copy
                .sign_in("synthetic", b"synthetic-password", 50)
                .unwrap();
            assert!(
                copy.verify_row_policy_access(fresh.access.expose(), 50, 7)
                    .is_ok()
            );
            assert!(!copy.public_admission().unwrap().enabled);
            assert_eq!(store.database.committed_wal().unwrap(), wal);
            assert_eq!(std::fs::read(&backup).unwrap(), bytes);
            assert!(store.verify_access(token.access.expose(), 100).is_ok());
            let before = store.database.last_transaction();
            store.reset_session_clock(50).unwrap();
            assert_eq!(store.database.last_transaction(), before + 1);
            let closed = store.public_admission().unwrap();
            assert!(!closed.enabled);
            assert_eq!(closed.revision, before + 1);
            assert_eq!(closed.previous, receipt.revision);
            store.reset_session_clock(50).unwrap();
            assert_eq!(store.public_admission().unwrap(), closed);
            assert!(matches!(
                store.set_public_admission(receipt.revision, true),
                Err(Error::AdmissionConflict)
            ));
        }
    }
}
#[test]
fn valid_engine_images_with_bad_admission_rows_or_inventory_are_rejected_before_restore() {
    let _io = TEST_IO.lock().unwrap();
    for defect in 0..15 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private");
        let store = create(&path, 5, false);
        let mut bad = store
            .database
            .view()
            .unwrap()
            .get(TABLE, &Key::Integer(1))
            .unwrap()
            .unwrap()
            .clone();
        match defect {
            0 => bad[1] = Value::Integer(2),
            1 => bad[3] = Value::Text("0".into()),
            2 => bad[3] = Value::Text("01".into()),
            3 => bad[3] = Value::Text("+1".into()),
            4 => bad[3] = Value::Text(u64::MAX.to_string()),
            5 => bad[4] = bad[3].clone(),
            6 => bad[4] = Value::Text("00".into()),
            7 => bad[4] = Value::Text("18446744073709551616".into()),
            8 => bad[2] = Value::Boolean(true),
            _ => {}
        }
        drop(store);
        let mut raw = Database::open(&path).unwrap();
        let mut tx = raw.begin().unwrap();
        match defect {
            0..=8 => tx.update(TABLE, &Key::Integer(1), bad).unwrap(),
            9 => {
                tx.delete(TABLE, &Key::Integer(1)).unwrap();
            }
            10 => {
                bad[0] = Value::Integer(2);
                tx.insert(TABLE, bad).unwrap();
            }
            11 => tx.drop_table(TABLE).unwrap(),
            12 => {
                let mut wrong = schema();
                wrong.name = "synthetic_extra".into();
                tx.create_table(wrong).unwrap();
            }
            13 => {
                tx.drop_table(TABLE).unwrap();
                let mut wrong = schema();
                wrong.columns[2].nullable = true;
                tx.create_table(wrong).unwrap();
            }
            _ => {
                let mut scope = tx
                    .view()
                    .unwrap()
                    .get(SCOPE, &Key::Integer(1))
                    .unwrap()
                    .unwrap()
                    .clone();
                scope[1] = Value::Integer(4);
                tx.update(SCOPE, &Key::Integer(1), scope).unwrap();
            }
        }
        tx.commit().unwrap();
        let bytes = emilybase_backup::encode(&raw.committed_wal().unwrap()).unwrap();
        assert!(emilybase_backup::decode_verified(&bytes).is_ok());
        assert!(matches!(
            inspect_private_account_backup_bytes(&bytes, PROJECT),
            Err(Error::Corrupt)
        ));
        drop(raw);
        assert!(matches!(
            AccountStore::open(&path, PROJECT, PasswordPool::new(1).unwrap()),
            Err(Error::Corrupt)
        ));
        let target = dir.path().join("copy");
        assert!(
            restore_private_account_bytes(
                &bytes,
                &target,
                PROJECT,
                PasswordPool::new(1).unwrap(),
                50
            )
            .is_err()
        );
        assert!(!target.exists());
    }
}
#[test]
fn admission_revisions_preserve_u64_beyond_signed_range_and_refuse_overflow() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut store = create(&dir.path().join("private"), 5, false);
    let mut snapshot = store.database.view().unwrap().clone();
    let table_id = snapshot.table_id(TABLE).unwrap();
    for revision in [i64::MAX as u64 + 1, u64::MAX - 1] {
        snapshot
            .apply(emilybase_database::Event {
                table_id,
                kind: emilybase_database::EventKind::Replace(vec![
                    Value::Integer(1),
                    Value::Integer(1),
                    Value::Boolean(false),
                    Value::Text(revision.to_string()),
                    Value::Text("1".into()),
                ]),
            })
            .unwrap();
        assert_eq!(validate(&snapshot, revision).unwrap().revision, revision);
        let pages = snapshot.pages().cloned().collect::<Vec<_>>();
        let wal = emilybase_wal_for_test(&store, revision, &pages);
        let bytes = emilybase_backup::encode(&wal).unwrap();
        assert_eq!(
            inspect_private_account_backup_bytes(&bytes, PROJECT)
                .unwrap()
                .database
                .last_transaction,
            revision
        );
        let target = dir.path().join(revision.to_string());
        emilybase_backup::restore_bytes(&bytes, &target).unwrap();
        let mut high = AccountStore::open(&target, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
        if revision == u64::MAX - 1 {
            let before = high.database.committed_wal().unwrap();
            assert!(matches!(
                high.set_public_admission(revision, true),
                Err(Error::AdmissionCapacity)
            ));
            assert_eq!(
                high.set_public_admission(revision, false).unwrap().revision,
                revision
            );
            assert_eq!(high.database.committed_wal().unwrap(), before);
        } else {
            assert_eq!(
                high.set_public_admission(revision, true).unwrap().revision,
                revision + 1
            );
        }
    }
    assert!(store.backup_image().is_ok());
}
fn emilybase_wal_for_test(
    store: &AccountStore,
    revision: u64,
    pages: &[emilybase_storage::Page],
) -> Vec<u8> {
    emilybase_wal::encode_snapshot(store.database.database_id(), revision, pages).unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn independent_flag_model_matches_cas_retries_and_unrelated_commits(commands in prop::collection::vec((any::<bool>(),0_u8..4),1..32),compacted in any::<bool>()) {
        let _io=TEST_IO.lock().unwrap();
        let dir=tempfile::tempdir().unwrap();
        let path=dir.path().join("private");
        let mut store=create(&path,5,compacted);
        let mut model=(false,store.database.last_transaction(),0);
        let mut clock=100;
        for (requested,selector) in commands {
            let expected=match selector {0=>model.1,1=>model.2,2=>0,_=>u64::MAX};
            let last=store.database.last_transaction();
            let wal=store.database.committed_wal().unwrap();
            let result=store.set_public_admission(expected,requested);
            if requested==model.0 && (expected==model.1 || expected==model.2) {
                prop_assert!(result.is_ok());
                prop_assert_eq!(store.database.committed_wal().unwrap(),wal);
            } else if expected==model.1 {
                model=(requested,last+1,model.1);
                prop_assert!(result.is_ok());
            } else {
                prop_assert!(matches!(result,Err(Error::AdmissionConflict)));
                prop_assert_eq!(store.database.committed_wal().unwrap(),wal);
            }
            let current=store.public_admission().unwrap();
            prop_assert_eq!((current.enabled,current.revision,current.previous),model);
            clock+=1;store.advance_session_clock(clock).unwrap();
            prop_assert_eq!(store.public_admission().unwrap(),current);
            drop(store);
            store=AccountStore::open(&path,PROJECT,PasswordPool::new(1).unwrap()).unwrap();
            let bytes=store.backup_image().unwrap();
            prop_assert_eq!(inspect_private_account_backup_bytes(&bytes,PROJECT).unwrap().private_version,5);
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn staged_and_caller_received_migration_open_close_and_reset_survive_process_kills() {
    let _io = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for action in ["migrate", "open", "close", "reset"] {
            for phase in ["stage", "commit"] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("private");
                let mut store = create(&path, if action == "migrate" { 4 } else { 5 }, compacted);
                if matches!(action, "close" | "reset") {
                    store
                        .set_public_admission(store.public_admission().unwrap().revision, true)
                        .unwrap();
                }
                let old = store.public_admission().ok();
                let old_scope = store.session_storage_scope().unwrap();
                let before = store.database.committed_wal().unwrap();
                drop(store);
                super::recovery_tests::kill_worker_at(
                    &path,
                    &format!("public-admission-{action}-{phase}"),
                );
                let mut recovered =
                    AccountStore::open(&path, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
                if phase == "stage" {
                    assert_eq!(recovered.database.committed_wal().unwrap(), before);
                    assert_eq!(recovered.public_admission().ok(), old);
                    assert_eq!(recovered.session_storage_scope().unwrap(), old_scope);
                } else {
                    let receipt = recovered.public_admission().unwrap();
                    assert_eq!(receipt.enabled, action == "open");
                    assert_eq!(receipt.previous, old.map_or(0, |value| value.revision));
                    if action == "reset" {
                        assert_ne!(recovered.session_storage_scope().unwrap(), old_scope);
                        assert_eq!(recovered.session_clock_floor().unwrap(), Some(50));
                    }
                }
                let image = recovered.backup_image().unwrap();
                assert!(inspect_private_account_backup_bytes(&image, PROJECT).is_ok());
            }
        }
    }
}
