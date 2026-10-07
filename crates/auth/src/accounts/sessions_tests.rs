use super::*;

fn opened(path: &Path) -> AccountStore {
    tests::raw_store(
        path,
        vec![tests::fixture_record("synthetic", [7; 16], 2).encode()],
    );
    let mut store =
        AccountStore::open(path, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
    store.enable_session_clock(100).unwrap();
    store
}
#[test]
fn issued_sessions_authenticate_rotate_and_logout_with_real_durable_rows() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic");
    let mut store = opened(&path);
    let first = store
        .sign_in("synthetic", b"synthetic-password", 100)
        .unwrap();
    assert_eq!(store.session_family_count().unwrap(), 1);
    {
        let principal = store.verify_access(first.access.expose(), 100).unwrap();
        assert_eq!(principal.account().id, [7; 16]);
        assert_eq!(principal.project(), tests::PROJECT);
        assert_eq!(principal.generation(), 1);
        assert_eq!(principal.family(), &first.metadata.family);
    }
    assert!(store.verify_access(first.refresh.expose(), 100).is_err());
    assert!(store.refresh_session(first.access.expose(), 100).is_err());
    let next = store.refresh_session(first.refresh.expose(), 101).unwrap();
    assert_eq!(next.metadata.generation, 2);
    assert_eq!(next.metadata.created, 100);
    assert_eq!(next.metadata.issued, 101);
    assert_eq!(first.metadata.family, next.metadata.family);
    assert!(store.verify_access(first.access.expose(), 101).is_err());
    assert!(store.refresh_session(first.refresh.expose(), 101).is_err());
    assert!(store.verify_access(next.access.expose(), 101).is_ok());
    drop(store);
    let mut store =
        AccountStore::open(&path, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
    assert!(store.verify_access(next.access.expose(), 101).is_ok());
    store.logout_session(next.refresh.expose(), 101).unwrap();
    assert!(store.verify_access(next.access.expose(), 101).is_err());
    assert!(store.refresh_session(next.refresh.expose(), 101).is_err());
    assert_eq!(store.prune_session_families(101, 1).unwrap(), 1);
    assert_eq!(store.session_family_count().unwrap(), 0);
}

#[test]
fn cleanup_must_not_treat_corrupt_account_state_as_an_inactive_session() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut store = opened(&dir.path().join("synthetic"));
    store
        .sign_in("synthetic", b"synthetic-password", 100)
        .unwrap();
    let mut row = store
        .database
        .view()
        .unwrap()
        .get(USERS, &Key::Text("synthetic".into()))
        .unwrap()
        .unwrap()
        .clone();
    row[3] = Value::Integer(0);
    let mut tx = store.database.begin().unwrap();
    tx.update(USERS, &Key::Text("synthetic".into()), row)
        .unwrap();
    tx.commit().unwrap();
    let before = store.database.committed_wal().unwrap();
    assert!(matches!(
        store.prune_session_families(100, 1),
        Err(Error::Corrupt)
    ));
    assert_eq!(store.database.committed_wal().unwrap(), before);
    assert_eq!(store.session_family_count().unwrap(), 1);
}

#[test]
fn expiration_observes_durable_time_before_denial_and_absolute_lifetime_is_clipped() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic");
    let mut store = opened(&path);
    let first = store
        .sign_in("synthetic", b"synthetic-password", 100)
        .unwrap();
    assert!(store.verify_access(first.access.expose(), 999).is_ok());
    assert!(matches!(
        store.verify_access(first.access.expose(), 1000),
        Err(Error::Denied)
    ));
    assert!(matches!(
        store.verify_access(first.access.expose(), 999),
        Err(Error::Clock)
    ));
    drop(store);
    let mut store =
        AccountStore::open(&path, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
    assert!(matches!(
        store.verify_access(first.access.expose(), 999),
        Err(Error::Clock)
    ));
    let mut current = store.refresh_session(first.refresh.expose(), 1000).unwrap();
    while current.metadata.refresh_until < current.metadata.absolute_until {
        current = store
            .refresh_session(current.refresh.expose(), current.metadata.refresh_until - 1)
            .unwrap();
    }
    let last = current.metadata.absolute_until - 1;
    current = store
        .refresh_session(current.refresh.expose(), last)
        .unwrap();
    assert_eq!(
        current.metadata.access_until,
        current.metadata.absolute_until
    );
    assert_eq!(
        current.metadata.refresh_until,
        current.metadata.absolute_until
    );
    assert!(store.verify_access(current.access.expose(), last).is_ok());
    let end = current.metadata.absolute_until;
    assert!(matches!(
        store.refresh_session(current.refresh.expose(), end),
        Err(Error::Denied)
    ));
    assert!(matches!(
        store.verify_access(current.access.expose(), end),
        Err(Error::Denied)
    ));
    assert_eq!(store.session_clock_floor().unwrap(), Some(end));
    assert_eq!(store.prune_session_families(end, 1).unwrap(), 1);
}

#[test]
fn every_check_uses_current_user_epoch_disable_identity_and_incarnation() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut store = opened(&dir.path().join("synthetic"));
    assert!(matches!(
        store.sign_in("absent", b"synthetic-password", 100),
        Err(Error::Denied)
    ));
    assert!(matches!(
        store.sign_in("synthetic", b"wrong", 100),
        Err(Error::Denied)
    ));
    let first = store
        .sign_in("synthetic", b"synthetic-password", 100)
        .unwrap();
    store
        .change_password("synthetic", b"synthetic-password", b"replacement")
        .unwrap();
    assert!(store.verify_access(first.access.expose(), 100).is_err());
    assert!(store.refresh_session(first.refresh.expose(), 100).is_err());
    let second = store.sign_in("synthetic", b"replacement", 100).unwrap();
    store.set_disabled("synthetic", true).unwrap();
    assert!(store.verify_access(second.access.expose(), 100).is_err());
    assert!(store.sign_in("synthetic", b"replacement", 100).is_err());
    store.set_disabled("synthetic", false).unwrap();
    assert!(store.verify_access(second.access.expose(), 100).is_err());
    let third = store.sign_in("synthetic", b"replacement", 100).unwrap();
    store.reset_session_clock(0).unwrap();
    assert!(store.verify_access(third.access.expose(), 0).is_err());
    assert!(store.refresh_session(third.refresh.expose(), 0).is_err());
    assert_eq!(store.prune_session_families(0, 128).unwrap(), 3);
    assert_eq!(store.session_family_count().unwrap(), 0);
}

#[test]
fn two_concurrent_refresh_attempts_have_one_durable_winner() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut store = opened(&dir.path().join("synthetic"));
    let original = store
        .sign_in("synthetic", b"synthetic-password", 100)
        .unwrap();
    let owner = std::sync::Mutex::new(store);
    let barrier = std::sync::Barrier::new(3);
    let results = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            barrier.wait();
            owner
                .lock()
                .unwrap()
                .refresh_session(original.refresh.expose(), 100)
        });
        let b = scope.spawn(|| {
            barrier.wait();
            owner
                .lock()
                .unwrap()
                .refresh_session(original.refresh.expose(), 100)
        });
        barrier.wait();
        [a.join().unwrap(), b.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(Error::Denied)))
            .count(),
        1
    );
    let winner = results.into_iter().find_map(Result::ok).unwrap();
    let mut store = owner.into_inner().unwrap();
    assert_eq!(winner.metadata.generation, 2);
    assert_eq!(store.session_family_count().unwrap(), 1);
    assert!(store.verify_access(winner.access.expose(), 100).is_ok());
    assert!(store.verify_access(original.access.expose(), 100).is_err());
}

#[test]
fn both_wals_restore_real_sessions_and_explicit_reset_prevents_old_restore_authority() {
    let _io = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic");
        let mut store = opened(&path);
        if compacted {
            store.compact().unwrap();
        }
        let first = store
            .sign_in("synthetic", b"synthetic-password", 100)
            .unwrap();
        let old = store.database.view().unwrap().clone();
        let next = store.refresh_session(first.refresh.expose(), 101).unwrap();
        let key = Key::Text(
            first
                .metadata
                .family
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        );
        assert_eq!(
            old.get(super::session_schema::FAMILIES, &key)
                .unwrap()
                .unwrap()[5],
            Value::Integer(1)
        );
        let wal = store.database.committed_wal().unwrap();
        for text in [
            first.access.expose(),
            first.refresh.expose(),
            next.access.expose(),
            next.refresh.expose(),
        ] {
            assert!(!wal.windows(text.len()).any(|part| part == text.as_bytes()));
        }
        let archive = dir.path().join("synthetic.backup");
        let report = store.backup(&archive).unwrap();
        assert_eq!(report.wal_version, if compacted { 2 } else { 1 });
        assert_eq!(report.tables, 5);
        assert_eq!(report.rows, 5);
        let destination = dir.path().join("restored");
        emilybase_backup::restore(&archive, &destination).unwrap();
        let mut restored =
            AccountStore::open(&destination, tests::PROJECT, PasswordPool::new(1).unwrap())
                .unwrap();
        assert!(restored.verify_access(next.access.expose(), 101).is_ok());
        assert!(
            restored
                .refresh_session(first.refresh.expose(), 101)
                .is_err()
        );
        restored.reset_session_clock(0).unwrap();
        assert!(restored.verify_access(next.access.expose(), 0).is_err());
        drop(store);
        let mut reopened =
            AccountStore::open(&path, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
        assert!(reopened.verify_access(next.access.expose(), 101).is_ok());
        assert_eq!(reopened.database.committed_wal().unwrap(), wal);
    }
}

#[test]
fn capacity_includes_history_and_cleanup_respects_the_128_record_bound() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic");
    let rows = (0..MAX_SESSION_FAMILIES)
        .map(|i| {
            let mut id = [0; 16];
            id[..8].copy_from_slice(&(i as u64).to_le_bytes());
            super::session_schema_tests::family_row(id, [7; 16], 2, [9; 16])
        })
        .collect();
    super::session_schema_tests::raw_v2(
        &path,
        rows,
        vec![super::session_schema_tests::meta([9; 16])],
    );
    let mut store =
        AccountStore::open(&path, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
    store.enable_session_clock(100).unwrap();
    let before = store.database.committed_wal().unwrap();
    assert!(matches!(
        store.sign_in("synthetic", &[], 100),
        Err(Error::SessionCapacity)
    ));
    assert_eq!(store.database.committed_wal().unwrap(), before);
    for limit in [0, 129, usize::MAX] {
        assert!(matches!(
            store.prune_session_families(100, limit),
            Err(Error::Cleanup)
        ));
        assert_eq!(store.database.committed_wal().unwrap(), before);
    }
    assert_eq!(store.prune_session_families(100, 128).unwrap(), 128);
    assert_eq!(
        store.session_family_count().unwrap(),
        MAX_SESSION_FAMILIES - 128
    );
    let issued = store
        .sign_in("synthetic", b"synthetic-password", 100)
        .unwrap();
    assert!(store.verify_access(issued.access.expose(), 100).is_ok());
}

#[test]
fn generation_exhaustion_malformed_credentials_and_noop_revoke_preserve_family_state() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut store = opened(&dir.path().join("synthetic"));
    let token = store
        .sign_in("synthetic", b"synthetic-password", 100)
        .unwrap();
    let key = Key::Text(
        token
            .metadata
            .family
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    );
    let mut row = store
        .database
        .view()
        .unwrap()
        .get(super::session_schema::FAMILIES, &key)
        .unwrap()
        .unwrap()
        .clone();
    row[5] = Value::Integer(i64::MAX);
    let mut tx = store.database.begin().unwrap();
    tx.update(super::session_schema::FAMILIES, &key, row)
        .unwrap();
    tx.commit().unwrap();
    let before = store.database.committed_wal().unwrap();
    assert!(matches!(
        store.refresh_session(token.refresh.expose(), 100),
        Err(Error::Generation)
    ));
    assert_eq!(store.database.committed_wal().unwrap(), before);
    for text in ["", "../synthetic", &"a".repeat(1024 * 1024)] {
        assert!(store.verify_access(text, 100).is_err());
        assert!(store.refresh_session(text, 100).is_err());
        assert_eq!(store.database.committed_wal().unwrap(), before);
    }
    store
        .revoke_session_family(&token.metadata.family, 100)
        .unwrap();
    let after = store.database.committed_wal().unwrap();
    store
        .revoke_session_family(&token.metadata.family, 100)
        .unwrap();
    assert_eq!(store.database.committed_wal().unwrap(), after);
    assert!(store.verify_access(token.access.expose(), 100).is_err());
}

#[test]
fn sessions_require_clock_activation_and_project_bound_token_records() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic");
    tests::raw_store(
        &path,
        vec![tests::fixture_record("synthetic", [7; 16], 2).encode()],
    );
    let mut store =
        AccountStore::open(&path, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
    let before = store.database.committed_wal().unwrap();
    assert!(matches!(
        store.sign_in("synthetic", b"synthetic-password", 100),
        Err(Error::ClockDisabled)
    ));
    assert_eq!(store.database.committed_wal().unwrap(), before);
    store.enable_session_clock(100).unwrap();
    let token = store
        .sign_in("synthetic", b"synthetic-password", 100)
        .unwrap();
    let other_project = "22222222222222222222222222222222";
    let mut other = AccountStore::create(
        dir.path().join("other"),
        other_project,
        PasswordPool::new(1).unwrap(),
    )
    .unwrap();
    other
        .create_user("synthetic", b"synthetic-password")
        .unwrap();
    other.enable_session_clock(100).unwrap();
    assert!(matches!(
        other.verify_access(token.access.expose(), 100),
        Err(Error::Denied)
    ));
    assert!(matches!(
        other.refresh_session(token.refresh.expose(), 100),
        Err(Error::Denied)
    ));
    let key = Key::Text(
        token
            .metadata
            .family
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    );
    let row = store
        .database
        .view()
        .unwrap()
        .get(super::session_schema::FAMILIES, &key)
        .unwrap()
        .unwrap()
        .clone();
    let mut tx = other.database.begin().unwrap();
    tx.insert(super::session_schema::FAMILIES, row).unwrap();
    tx.commit().unwrap();
    assert!(matches!(
        other.verify_access(token.access.expose(), 100),
        Err(Error::Corrupt)
    ));
}

#[test]
#[cfg(target_os = "linux")]
fn interrupted_refresh_and_logout_restore_exact_acknowledged_generation_and_revocation() {
    let _io = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for mode in [
            "session-refresh-stage",
            "session-refresh-commit",
            "session-logout-stage",
            "session-logout-commit",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("synthetic");
            let mut store = opened(&path);
            if compacted {
                store.compact().unwrap();
            }
            let token = store
                .sign_in("synthetic", b"synthetic-password", 100)
                .unwrap();
            let before = store.database.committed_wal().unwrap();
            drop(store);
            super::recovery_tests::kill_worker_with_input(&path, mode, token.refresh.expose());
            let mut restored =
                AccountStore::open(&path, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
            let committed = mode.ends_with("commit");
            let refreshing = mode.starts_with("session-refresh");
            let key = Key::Text(
                token
                    .metadata
                    .family
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
            );
            let row = restored
                .database
                .view()
                .unwrap()
                .get(super::session_schema::FAMILIES, &key)
                .unwrap()
                .unwrap();
            let info = inspect_session_record(row, tests::PROJECT).unwrap();
            assert_eq!(info.generation, if committed && refreshing { 2 } else { 1 });
            assert_eq!(info.revoked, committed && !refreshing);
            assert_eq!(
                restored.verify_access(token.access.expose(), 100).is_ok(),
                !committed
            );
            if !committed {
                assert_eq!(restored.database.committed_wal().unwrap(), before);
            }
            let archive = dir.path().join("recovered.backup");
            let report = restored.backup(&archive).unwrap();
            assert_eq!(report.wal_version, if compacted { 2 } else { 1 });
            let destination = dir.path().join("verified");
            emilybase_backup::restore(&archive, &destination).unwrap();
            let verified =
                AccountStore::open(&destination, tests::PROJECT, PasswordPool::new(1).unwrap())
                    .unwrap();
            let row = verified
                .database
                .view()
                .unwrap()
                .get(super::session_schema::FAMILIES, &key)
                .unwrap()
                .unwrap();
            assert_eq!(inspect_session_record(row, tests::PROJECT).unwrap(), info);
        }
    }
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn lifecycle_sequences_match_independent_epoch_revocation_generation_model(
        events in prop::collection::vec(0..6_u8, 0..20)
    ) {
        let _io = TEST_IO.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic");
        let mut store = opened(&path);
        let mut token = store.sign_in("synthetic", b"synthetic-password", 100).unwrap();
        let mut active = true;
        let mut disabled = false;
        let mut epoch = 2;
        let mut generation = 1;
        for (step, event) in events.into_iter().enumerate() {
            let now = 101 + step as u64;
            match event {
                0 => {
                    let result = store.refresh_session(token.refresh.expose(), now);
                    prop_assert_eq!(result.is_ok(), active);
                    if let Ok(next) = result {
                        generation += 1;
                        prop_assert_eq!(next.metadata.generation, generation);
                        token = next;
                    }
                }
                1 => {
                    let result = store.logout_session(token.refresh.expose(), now);
                    prop_assert_eq!(result.is_ok(), active);
                    active = false;
                }
                2 => {
                    disabled = !disabled;
                    epoch += 1;
                    store.set_disabled("synthetic", disabled).unwrap();
                    active = false;
                }
                3 => {
                    let result = store.sign_in("synthetic", b"synthetic-password", now);
                    prop_assert_eq!(result.is_ok(), !disabled);
                    if let Ok(next) = result {
                        generation = 1;
                        prop_assert_eq!(next.metadata.credential_epoch, epoch);
                        token = next;
                        active = true;
                    }
                }
                4 => {
                    store.reset_session_clock(now).unwrap();
                    active = false;
                }
                _ => { store.prune_session_families(now, 128).unwrap(); }
            }
            prop_assert_eq!(store.verify_access(token.access.expose(), now).is_ok(), active);
        }
        let now = store.session_clock_floor().unwrap().unwrap();
        store.compact().unwrap();
        let archive = dir.path().join("synthetic.backup");
        store.backup(&archive).unwrap();
        let destination = dir.path().join("verified");
        emilybase_backup::restore(&archive, &destination).unwrap();
        let mut restored = AccountStore::open(&destination, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
        prop_assert_eq!(restored.verify_access(token.access.expose(), now).is_ok(), active);
        drop(store);
        let mut reopened = AccountStore::open(&path, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
        prop_assert_eq!(reopened.verify_access(token.access.expose(), now).is_ok(), active);
    }
}
