use super::session_clock::*;
use super::*;
use crate::tokens::{TokenKind, issue};
use proptest::prelude::*;

fn original(path: &Path) -> AccountStore {
    tests::raw_store(
        path,
        vec![tests::fixture_record("synthetic", [7; 16], 2).encode()],
    );
    AccountStore::open(path, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap()
}
#[test]
fn activation_is_atomic_from_both_prior_schemas_and_equal_time_is_a_noop() {
    let _io = TEST_IO.lock().unwrap();
    for prior in [1, 2] {
        for compacted in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("synthetic");
            let mut store = original(&path);
            let before_scope = if prior == 2 {
                Some(store.enable_session_storage().unwrap())
            } else {
                None
            };
            if compacted {
                store.compact().unwrap();
            }
            let old = store.database.view().unwrap().clone();
            let before = store.database.committed_wal().unwrap();
            assert_eq!(store.session_clock_floor().unwrap(), None);
            let scope = store.enable_session_clock(100).unwrap();
            assert_ne!(Some(scope.clone()), before_scope);
            assert_eq!(store.database.view().unwrap().table_count(), 5);
            assert_eq!(store.database.view().unwrap().row_count(), 4);
            assert_eq!(store.count().unwrap(), 1);
            assert_eq!(old.table_count(), if prior == 1 { 2 } else { 4 });
            assert_eq!(
                old.get(SCOPE, &Key::Integer(1)).unwrap().unwrap()[1],
                Value::Integer(prior)
            );
            let after = store.database.committed_wal().unwrap();
            assert_ne!(before, after);
            assert!(after.starts_with(&before));
            assert_eq!(store.enable_session_clock(100).unwrap(), scope);
            store.advance_session_clock(100).unwrap();
            assert_eq!(store.database.committed_wal().unwrap(), after);
            store.advance_session_clock(101).unwrap();
            assert_eq!(store.session_clock_floor().unwrap(), Some(101));
            assert!(matches!(
                store.advance_session_clock(100),
                Err(Error::Clock)
            ));
            let ack = store.database.committed_wal().unwrap();
            drop(store);
            let mut reopened =
                AccountStore::open(&path, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
            assert_eq!(reopened.session_storage_scope().unwrap(), Some(scope));
            assert_eq!(reopened.session_clock_floor().unwrap(), Some(101));
            assert!(matches!(
                reopened.advance_session_clock(100),
                Err(Error::Clock)
            ));
            assert_eq!(reopened.database.committed_wal().unwrap(), ack);
            let archive = dir.path().join("synthetic.backup");
            let report = reopened.backup(&archive).unwrap();
            assert_eq!(report.tables, 5);
            assert_eq!(report.rows, 4);
            assert_eq!(report.wal_version, if compacted { 2 } else { 1 });
            let destination = dir.path().join("restored");
            emilybase_backup::restore(&archive, &destination).unwrap();
            let restored =
                AccountStore::open(&destination, tests::PROJECT, PasswordPool::new(1).unwrap())
                    .unwrap();
            assert_eq!(restored.session_clock_floor().unwrap(), Some(101));
        }
    }
}

#[test]
fn reset_changes_time_and_incarnation_together_and_restored_old_tokens_fail_new_scope() {
    let _io = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic");
        let mut store = original(&path);
        let scope = store.enable_session_clock(200).unwrap();
        let (token, digest) = issue(TokenKind::Access, &scope, [3; 16]).unwrap();
        if compacted {
            store.compact().unwrap();
        }
        let old = store.database.view().unwrap().clone();
        let user = store.record("synthetic").unwrap().unwrap().info;
        let archive = dir.path().join("before.backup");
        store.backup(&archive).unwrap();
        let restored_path = dir.path().join("restored");
        emilybase_backup::restore(&archive, &restored_path).unwrap();
        let mut restored = AccountStore::open(
            &restored_path,
            tests::PROJECT,
            PasswordPool::new(1).unwrap(),
        )
        .unwrap();
        assert_eq!(restored.session_clock_floor().unwrap(), Some(200));
        assert!(
            digest
                .matches(
                    token.expose(),
                    &restored.session_storage_scope().unwrap().unwrap()
                )
                .unwrap()
        );
        let next = restored.reset_session_clock(0).unwrap();
        assert_ne!(next, scope);
        assert!(!digest.matches(token.expose(), &next).unwrap());
        assert_eq!(restored.session_clock_floor().unwrap(), Some(0));
        assert_eq!(restored.record("synthetic").unwrap().unwrap().info, user);
        assert_eq!(
            old.get(CLOCK, &Key::Integer(1)).unwrap().unwrap()[2],
            Value::Integer(200)
        );
        let final_wal = restored.database.committed_wal().unwrap();
        drop(restored);
        let mut reopened = AccountStore::open(
            &restored_path,
            tests::PROJECT,
            PasswordPool::new(1).unwrap(),
        )
        .unwrap();
        assert_eq!(reopened.session_storage_scope().unwrap(), Some(next));
        assert_eq!(reopened.session_clock_floor().unwrap(), Some(0));
        assert_eq!(reopened.database.committed_wal().unwrap(), final_wal);
    }
}

#[test]
fn unavailable_backward_and_out_of_range_times_preserve_exact_history() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut store = original(&dir.path().join("synthetic"));
    let before = store.database.committed_wal().unwrap();
    assert!(matches!(
        store.advance_session_clock(0),
        Err(Error::ClockDisabled)
    ));
    assert!(matches!(
        store.reset_session_clock(0),
        Err(Error::ClockDisabled)
    ));
    for invalid in [i64::MAX as u64 + 1, u64::MAX] {
        assert!(matches!(
            store.enable_session_clock(invalid),
            Err(Error::Clock)
        ));
    }
    assert_eq!(store.database.committed_wal().unwrap(), before);
    let scope = store.enable_session_clock(i64::MAX as u64).unwrap();
    let after = store.database.committed_wal().unwrap();
    for now in [0, 1, i64::MAX as u64 - 1, i64::MAX as u64 + 1, u64::MAX] {
        assert!(matches!(
            store.advance_session_clock(now),
            Err(Error::Clock)
        ));
        assert!(matches!(store.enable_session_clock(now), Err(Error::Clock)));
        assert_eq!(store.session_storage_scope().unwrap(), Some(scope.clone()));
        assert_eq!(store.database.committed_wal().unwrap(), after);
    }
    assert_eq!(store.enable_session_storage().unwrap(), scope);
    assert_eq!(store.database.committed_wal().unwrap(), after);
}

#[test]
fn new_incarnation_excludes_current_and_retained_history_with_four_bounded_attempts() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic");
    super::session_schema_tests::raw_v2(
        &path,
        vec![super::session_schema_tests::family_row(
            [3; 16], [7; 16], 1, [8; 16],
        )],
        vec![super::session_schema_tests::meta([9; 16])],
    );
    let mut store =
        AccountStore::open(&path, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
    let before = store.database.committed_wal().unwrap();
    assert_eq!(
        choose_for_test(&store, &[[9; 16], [8; 16], [10; 16]])
            .unwrap()
            .0,
        [10; 16]
    );
    assert!(matches!(
        choose_for_test(&store, &[[9; 16]; 4]),
        Err(Error::Randomness)
    ));
    assert!(matches!(
        choose_for_test(&store, &[[8; 16]; 4]),
        Err(Error::Randomness)
    ));
    assert!(matches!(
        choose_for_test(&store, &[]),
        Err(Error::Randomness)
    ));
    assert_eq!(store.database.committed_wal().unwrap(), before);
    let next = store.enable_session_clock(0).unwrap();
    assert_ne!(
        next,
        crate::tokens::TokenScope::new(tests::PROJECT, [8; 16]).unwrap()
    );
    assert_ne!(
        next,
        crate::tokens::TokenScope::new(tests::PROJECT, [9; 16]).unwrap()
    );
}

#[test]
fn persisted_clock_requires_exact_schema_one_row_and_nonnegative_integer() {
    let _io = TEST_IO.lock().unwrap();
    for variant in 0..10 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic");
        let mut store = original(&path);
        store.enable_session_clock(100).unwrap();
        drop(store);
        let mut db = Database::open(&path).unwrap();
        let mut tx = db.begin().unwrap();
        match variant {
            0 => {
                tx.delete(CLOCK, &Key::Integer(1)).unwrap();
            }
            1 => {
                tx.update(
                    CLOCK,
                    &Key::Integer(1),
                    vec![Value::Integer(1), Value::Integer(0), Value::Integer(100)],
                )
                .unwrap();
            }
            2 => {
                tx.update(
                    CLOCK,
                    &Key::Integer(1),
                    vec![Value::Integer(1), Value::Integer(2), Value::Integer(100)],
                )
                .unwrap();
            }
            3 => {
                tx.update(
                    CLOCK,
                    &Key::Integer(1),
                    vec![Value::Integer(1), Value::Integer(1), Value::Integer(-1)],
                )
                .unwrap();
            }
            4 => {
                tx.insert(
                    CLOCK,
                    vec![Value::Integer(2), Value::Integer(1), Value::Integer(100)],
                )
                .unwrap();
            }
            5 => {
                tx.delete(CLOCK, &Key::Integer(1)).unwrap();
                tx.insert(
                    CLOCK,
                    vec![Value::Integer(2), Value::Integer(1), Value::Integer(100)],
                )
                .unwrap();
            }
            6 => {
                tx.drop_table(CLOCK).unwrap();
            }
            7 => {
                tx.create_table(super::records::schema(
                    "foreign",
                    &[("id", emilybase_catalog::DataType::Integer)],
                ))
                .unwrap();
            }
            8 => {
                tx.drop_table(CLOCK).unwrap();
                tx.create_table(super::records::schema(
                    CLOCK,
                    &[
                        ("id", emilybase_catalog::DataType::Integer),
                        ("version", emilybase_catalog::DataType::Integer),
                        ("observed", emilybase_catalog::DataType::Boolean),
                    ],
                ))
                .unwrap();
            }
            _ => {
                let mut row = tx
                    .view()
                    .unwrap()
                    .get(SCOPE, &Key::Integer(1))
                    .unwrap()
                    .unwrap()
                    .clone();
                row[1] = Value::Integer(4);
                tx.update(SCOPE, &Key::Integer(1), row).unwrap();
            }
        }
        tx.commit().unwrap();
        drop(db);
        assert!(
            matches!(
                AccountStore::open(&path, tests::PROJECT, PasswordPool::new(1).unwrap()),
                Err(Error::Corrupt)
            ),
            "variant {variant}"
        );
    }
}

#[test]
fn current_incarnation_rows_cannot_claim_issue_times_above_committed_clock() {
    let _io = TEST_IO.lock().unwrap();
    for current in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic");
        let mut store = original(&path);
        store.enable_session_clock(100).unwrap();
        let incarnation = if current {
            let row = store
                .database
                .view()
                .unwrap()
                .get(super::session_schema::META, &Key::Integer(1))
                .unwrap()
                .unwrap();
            let Value::Bytes(bytes) = &row[2] else {
                unreachable!()
            };
            bytes.as_slice().try_into().unwrap()
        } else {
            [9; 16]
        };
        drop(store);
        let mut db = Database::open(&path).unwrap();
        let mut row = super::session_schema_tests::family_row([3; 16], [7; 16], 1, incarnation);
        row[7] = Value::Integer(101);
        row[8] = Value::Integer(1001);
        row[9] = Value::Integer(604901);
        let mut tx = db.begin().unwrap();
        tx.insert(super::session_schema::FAMILIES, row).unwrap();
        tx.commit().unwrap();
        drop(db);
        let opened = AccountStore::open(&path, tests::PROJECT, PasswordPool::new(1).unwrap());
        assert_eq!(opened.is_ok(), !current);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn committed_watermark_and_reset_match_independent_history_model(events in prop::collection::vec((any::<bool>(),0..500_u64),0..24)) {
        let _io=TEST_IO.lock().unwrap();let dir=tempfile::tempdir().unwrap();let path=dir.path().join("synthetic");let mut store=original(&path);
        store.enable_session_clock(0).unwrap();let mut floor=0;
        for (reset,time) in events {
            let before=store.database.committed_wal().unwrap();let old_scope=store.session_storage_scope().unwrap();
            if reset {store.reset_session_clock(time).unwrap();floor=time;prop_assert_ne!(store.session_storage_scope().unwrap(),old_scope);}
            else if time<floor {prop_assert!(matches!(store.advance_session_clock(time),Err(Error::Clock)));prop_assert_eq!(store.database.committed_wal().unwrap(),before);}
            else {store.advance_session_clock(time).unwrap();if time==floor {prop_assert_eq!(store.database.committed_wal().unwrap(),before);}floor=time;}
            prop_assert_eq!(store.session_clock_floor().unwrap(),Some(floor));prop_assert_eq!(store.count().unwrap(),1);
        }
        let scope=store.session_storage_scope().unwrap();store.compact().unwrap();let archive=dir.path().join("synthetic.backup");store.backup(&archive).unwrap();
        let destination=dir.path().join("restored");emilybase_backup::restore(&archive,&destination).unwrap();
        let restored=AccountStore::open(&destination,tests::PROJECT,PasswordPool::new(1).unwrap()).unwrap();prop_assert_eq!(restored.session_clock_floor().unwrap(),Some(floor));prop_assert_eq!(restored.session_storage_scope().unwrap(),scope);
        drop(store);let reopened=AccountStore::open(&path,tests::PROJECT,PasswordPool::new(1).unwrap()).unwrap();prop_assert_eq!(reopened.session_clock_floor().unwrap(),Some(floor));
    }
    #[test]
    fn pure_clock_record_matches_integer_shape_and_signed_bounds(id in prop_oneof![Just(1_i64),any::<i64>()],version in prop_oneof![Just(1_i64),any::<i64>()],observed in any::<i64>()) {
        let row=[Value::Integer(id),Value::Integer(version),Value::Integer(observed)];let result=inspect_session_clock_record(&row);
        prop_assert_eq!(result.is_ok(),id==1 && version==1 && observed>=0);
        if let Ok(time)=result {prop_assert_eq!(time,observed as u64);}
    }
}

#[test]
fn pure_clock_inspection_refuses_extra_fields_nulls_and_unknown_versions() {
    for time in [0, 1, i64::MAX] {
        assert_eq!(
            inspect_session_clock_record(&[
                Value::Integer(1),
                Value::Integer(1),
                Value::Integer(time)
            ])
            .unwrap(),
            time as u64
        );
    }
    let row = vec![Value::Integer(1), Value::Integer(1), Value::Integer(100)];
    for length in 0..3 {
        assert!(inspect_session_clock_record(&row[..length]).is_err());
    }
    for index in 0..3 {
        for value in [
            Value::Null,
            Value::Boolean(true),
            Value::Bytes(vec![0; 1024]),
            Value::Text("synthetic".into()),
            Value::Float(1.0),
        ] {
            let mut changed = row.clone();
            changed[index] = value;
            assert!(inspect_session_clock_record(&changed).is_err());
        }
    }
    let mut extra = row;
    extra.push(Value::Integer(0));
    assert!(inspect_session_clock_record(&extra).is_err());
}

#[test]
#[cfg(target_os = "linux")]
fn forced_kills_preserve_clock_activation_advance_and_atomic_reset_boundaries() {
    let _io = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for mode in [
            "clock-enable-stage",
            "clock-enable-commit",
            "clock-advance-stage",
            "clock-advance-commit",
            "clock-reset-stage",
            "clock-reset-commit",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("synthetic");
            let mut store = original(&path);
            if compacted {
                store.compact().unwrap();
            }
            let enabling = mode.starts_with("clock-enable");
            let resetting = mode.starts_with("clock-reset");
            let committed = mode.ends_with("commit");
            if !enabling {
                store.enable_session_clock(100).unwrap();
            }
            let before_scope = store.session_storage_scope().unwrap();
            let before = store.database.committed_wal().unwrap();
            drop(store);
            super::recovery_tests::kill_worker_at(&path, mode);
            let mut restored =
                AccountStore::open(&path, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
            let floor = if enabling {
                if committed { Some(100) } else { None }
            } else if committed {
                Some(if resetting { 0 } else { 200 })
            } else {
                Some(100)
            };
            assert_eq!(restored.session_clock_floor().unwrap(), floor);
            assert_eq!(restored.count().unwrap(), 1);
            assert_eq!(
                restored
                    .record("synthetic")
                    .unwrap()
                    .unwrap()
                    .info
                    .credential_epoch,
                2
            );
            if !committed {
                assert_eq!(restored.database.committed_wal().unwrap(), before);
                assert_eq!(restored.session_storage_scope().unwrap(), before_scope);
            } else if enabling || resetting {
                assert_ne!(restored.session_storage_scope().unwrap(), before_scope);
            } else {
                assert_eq!(restored.session_storage_scope().unwrap(), before_scope);
            }
            let archive = dir.path().join("recovered.backup");
            let report = restored.backup(&archive).unwrap();
            assert_eq!(report.wal_version, if compacted { 2 } else { 1 });
            let destination = dir.path().join("verified");
            emilybase_backup::restore(&archive, &destination).unwrap();
            let verified =
                AccountStore::open(&destination, tests::PROJECT, PasswordPool::new(1).unwrap())
                    .unwrap();
            assert_eq!(verified.session_clock_floor().unwrap(), floor);
            assert_eq!(
                verified.session_storage_scope().unwrap(),
                restored.session_storage_scope().unwrap()
            );
        }
    }
}
