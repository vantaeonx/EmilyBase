use super::*;
use crate::{UserRowsError, UserTableOperation as Op, UserTableResult as Out, UserWrite as Write};
use emilybase_auth::{
    accounts::{Error as A, IssuedSession},
    row_policy::PolicyError,
};
use emilybase_catalog::{Key, Row};
use std::collections::BTreeMap;
const OWN:&[u8]=br#"{"version":1,"select":{"kind":"owner","column":"owner"},"insert":{"kind":"owner","column":"owner"},"update_using":{"kind":"owner","column":"owner"},"update_check":{"kind":"owner","column":"owner"},"delete":{"kind":"owner","column":"owner"}}"#;
const DENY:&[u8]=br#"{"version":1,"select":{"kind":"deny"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}}"#;
struct Fixture {
    store: Restored,
    root: AccountRoot,
    first: IssuedSession,
    second: IssuedSession,
}
fn setup(parent: &Path, compacted: bool, enabled: bool) -> Fixture {
    let store = restored(parent, 2, compacted);
    let (id, key) = &store.credentials[0];
    let mut root = AccountRoot::open(&store.root, pool()).unwrap();
    root.create_user(id, key, "second_user", PASSWORD).unwrap();
    let first = root.sign_in(id, key, LOGIN, PASSWORD, 50).unwrap();
    let second = root.sign_in(id, key, "second_user", PASSWORD, 50).unwrap();
    root.execute(
        id,
        key,
        "CREATE TABLE owned(id INT PRIMARY KEY,owner BYTES,n INT)",
        &[],
    )
    .unwrap();
    root.enable_row_policy_catalog(id, key).unwrap();
    root.install_row_policy(id, key, "owned", 0, OWN).unwrap();
    let closed = root.enable_public_admission_catalog(id, key).unwrap();
    if enabled {
        root.set_public_admission(id, key, closed.revision, true)
            .unwrap();
    }
    Fixture {
        store,
        root,
        first,
        second,
    }
}
fn row(pk: i64, owner: &[u8], amount: i64) -> Row {
    vec![
        Value::Integer(pk),
        Value::Bytes(owner.to_vec()),
        Value::Integer(amount),
    ]
}
fn call(f: &mut Fixture, who: usize, operation: Op) -> crate::Result<Out> {
    let id = &f.store.credentials[0].0;
    let access = if who == 0 {
        f.first.access.expose()
    } else {
        f.second.access.expose()
    };
    f.root.public_user_table(id, "owned", access, 50, operation)
}
fn rows(f: &Fixture) -> Vec<Row> {
    Database::open(
        f.store
            .root
            .join("registry")
            .join(&f.store.credentials[0].0)
            .join("data"),
    )
    .unwrap()
    .view()
    .unwrap()
    .scan("owned", 10000)
    .unwrap()
}
fn denied_row(result: crate::Result<Out>) {
    assert!(matches!(
        result,
        Err(Error::UserRows(
            UserRowsError::Policy(PolicyError::Denied) | UserRowsError::Rejected
        ))
    ));
}
#[test]
fn closed_and_legacy_admission_refuse_before_password_clock_token_or_public_owner_work() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = restored(dir.path(), 1, compacted);
        let (id, key) = &f.credentials[0];
        let mut root = AccountRoot::open(&f.root, pool()).unwrap();
        let session = root.sign_in(id, key, LOGIN, PASSWORD, 50).unwrap();
        for version in [3, 4, 5] {
            if version == 4 {
                root.enable_row_policy_catalog(id, key).unwrap();
            }
            if version == 5 {
                root.enable_public_admission_catalog(id, key).unwrap();
            }
            let before = histories(&f);
            // Holding public data makes an accidental pre-authentication open observable.
            let _public = Database::open(f.root.join("registry").join(id).join("data")).unwrap();
            for project in [
                id.as_str(),
                "../../synthetic",
                "00000000000000000000000000000000",
            ] {
                assert!(matches!(
                    root.public_sign_in(project, "INVALID", b"", u64::MAX),
                    Err(Error::Denied)
                ));
                assert!(matches!(
                    root.public_refresh_session(project, "invalid", u64::MAX),
                    Err(Error::Denied)
                ));
                assert!(matches!(
                    root.public_logout_session(project, "invalid", u64::MAX),
                    Err(Error::Denied)
                ));
                assert!(matches!(
                    root.public_user(project, session.access.expose(), u64::MAX),
                    Err(Error::Denied)
                ));
                assert!(matches!(
                    root.public_user_table(
                        project,
                        "missing",
                        session.access.expose(),
                        u64::MAX,
                        Op::Get(Key::Integer(1))
                    ),
                    Err(Error::Denied)
                ));
            }
            assert_eq!(histories(&f), before);
        }
    }
}
#[test]
fn public_auth_current_metadata_single_use_refresh_logout_and_project_scope_are_original_durable_operations()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = setup(dir.path(), compacted, true);
        let (id, key) = f.store.credentials[0].clone();
        let (other, other_key) = f.store.credentials[1].clone();
        f.root
            .enable_row_policy_catalog(&other, &other_key)
            .unwrap();
        let closed = f
            .root
            .enable_public_admission_catalog(&other, &other_key)
            .unwrap();
        f.root
            .set_public_admission(&other, &other_key, closed.revision, true)
            .unwrap();
        for login in [LOGIN, "missing_user"] {
            assert!(matches!(
                f.root.public_sign_in(&id, login, b"wrong", 50),
                Err(Error::Accounts(A::Denied))
            ));
        }
        let pair = f.root.public_sign_in(&id, LOGIN, PASSWORD, 50).unwrap();
        let info = f.root.public_user(&id, pair.access.expose(), 50).unwrap();
        assert_eq!(info.id, pair.metadata.user);
        assert_eq!(info.login, LOGIN);
        for token in [key.as_str(), pair.refresh.expose()] {
            assert!(f.root.public_user(&id, token, 50).is_err());
        }
        assert!(
            f.root
                .public_user(&other, pair.access.expose(), 50)
                .is_err()
        );
        assert!(
            f.root
                .public_refresh_session(&other, pair.refresh.expose(), 50)
                .is_err()
        );
        let next = f
            .root
            .public_refresh_session(&id, pair.refresh.expose(), 50)
            .unwrap();
        assert!(f.root.public_user(&id, pair.access.expose(), 50).is_err());
        assert!(
            f.root
                .public_refresh_session(&id, pair.refresh.expose(), 50)
                .is_err()
        );
        assert!(
            f.root
                .public_logout_session(&id, next.access.expose(), 50)
                .is_err()
        );
        drop(f.root);
        f.root = AccountRoot::open(&f.store.root, pool()).unwrap();
        assert_eq!(
            f.root
                .public_user(&id, next.access.expose(), 50)
                .unwrap()
                .id,
            info.id
        );
        f.root
            .public_logout_session(&id, next.refresh.expose(), 50)
            .unwrap();
        assert!(f.root.public_user(&id, next.access.expose(), 50).is_err());
        assert!(
            f.root
                .public_refresh_session(&id, next.refresh.expose(), 50)
                .is_err()
        );
        assert!(f.root.public_user(&id, f.first.access.expose(), 50).is_ok());
    }
}
#[test]
fn keyless_owned_crud_pages_and_late_policy_failure_preserve_hidden_rows_and_atomic_packets() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = setup(dir.path(), compacted, true);
        let a = f.first.metadata.user;
        let b = f.second.metadata.user;
        let before = histories(&f.store);
        assert!(matches!(
            call(
                &mut f,
                0,
                Op::Write(vec![
                    Write::Insert(row(1, &a, 10)),
                    Write::Update {
                        key: Key::Integer(1),
                        row: row(1, &a, 20)
                    }
                ])
            )
            .unwrap(),
            Out::Committed { operations: 2, .. }
        ));
        assert_eq!(
            call(&mut f, 0, Op::Get(Key::Integer(1))).unwrap(),
            Out::Row(Some(row(1, &a, 20)))
        );
        assert_eq!(
            call(&mut f, 1, Op::Get(Key::Integer(1))).unwrap(),
            Out::Row(None)
        );
        assert_eq!(
            call(&mut f, 1, Op::Get(Key::Integer(999))).unwrap(),
            Out::Row(None)
        );
        call(&mut f, 1, Op::Write(vec![Write::Insert(row(2, &b, 30))])).unwrap();
        call(&mut f, 0, Op::Write(vec![Write::Insert(row(3, &a, 40))])).unwrap();
        assert_eq!(
            call(
                &mut f,
                0,
                Op::Page {
                    after: None,
                    limit: 1
                }
            )
            .unwrap(),
            Out::Page {
                rows: vec![row(1, &a, 20)],
                next: Some(Key::Integer(1))
            }
        );
        assert_eq!(
            call(
                &mut f,
                0,
                Op::Page {
                    after: Some(Key::Integer(1)),
                    limit: 1
                }
            )
            .unwrap(),
            Out::Page {
                rows: vec![row(3, &a, 40)],
                next: None
            }
        );
        let protected = histories(&f.store);
        denied_row(call(
            &mut f,
            0,
            Op::Write(vec![
                Write::Insert(row(4, &a, 10)),
                Write::Update {
                    key: Key::Integer(1),
                    row: row(1, &b, 99),
                },
            ]),
        ));
        denied_row(call(
            &mut f,
            1,
            Op::Write(vec![Write::Delete(Key::Integer(1))]),
        ));
        denied_row(call(
            &mut f,
            0,
            Op::Write(vec![
                Write::Insert(row(4, &a, 10)),
                Write::Insert(row(1, &a, 1)),
            ]),
        ));
        assert_eq!(histories(&f.store), protected);
        assert_eq!(
            rows(&f),
            vec![row(1, &a, 20), row(2, &b, 30), row(3, &a, 40)]
        );
        call(&mut f, 0, Op::Write(vec![Write::Delete(Key::Integer(1))])).unwrap();
        let after = histories(&f.store);
        assert_eq!(before[3], after[3]);
        assert_eq!(&before[4..], &after[4..]);
    }
}
#[test]
fn current_flag_suspend_resume_rotation_epoch_and_time_changes_apply_to_every_public_call() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = setup(dir.path(), compacted, true);
        let (id, key) = f.store.credentials[0].clone();
        let a = f.first.metadata.user;
        call(&mut f, 0, Op::Write(vec![Write::Insert(row(1, &a, 10))])).unwrap();
        let receipt = f.root.public_admission(&id, &key).unwrap();
        let closed = f
            .root
            .set_public_admission(&id, &key, receipt.revision, false)
            .unwrap();
        let before = histories(&f.store);
        assert!(matches!(
            call(&mut f, 0, Op::Get(Key::Integer(1))),
            Err(Error::Denied)
        ));
        assert!(matches!(
            f.root.public_sign_in(&id, LOGIN, PASSWORD, 100),
            Err(Error::Denied)
        ));
        assert!(matches!(
            f.root
                .public_refresh_session(&id, f.first.refresh.expose(), 100),
            Err(Error::Denied)
        ));
        assert_eq!(histories(&f.store), before);
        f.root
            .set_public_admission(&id, &key, closed.revision, true)
            .unwrap();
        assert!(f.root.public_user(&id, f.first.access.expose(), 50).is_ok());
        let rotated = f.root.rotate_project_key(&id).unwrap();
        assert!(matches!(
            f.root
                .with_access(&id, &key, f.first.access.expose(), 50, |_| ()),
            Err(Error::Denied)
        ));
        assert!(call(&mut f, 0, Op::Get(Key::Integer(1))).is_ok());
        f.root
            .set_disabled(&id, &rotated.api_key, LOGIN, true)
            .unwrap();
        assert!(
            f.root
                .public_user(&id, f.first.access.expose(), 50)
                .is_err()
        );
        f.root
            .set_disabled(&id, &rotated.api_key, LOGIN, false)
            .unwrap();
        assert!(
            f.root
                .public_user(&id, f.first.access.expose(), 50)
                .is_err()
        );
        let fresh = f.root.public_sign_in(&id, LOGIN, PASSWORD, 50).unwrap();
        assert!(f.root.public_user(&id, fresh.access.expose(), 50).is_ok());
        assert!(matches!(
            f.root.public_user(&id, fresh.access.expose(), 49),
            Err(Error::Accounts(A::Clock))
        ));
        assert!(matches!(
            f.root.public_user(&id, fresh.access.expose(), 950),
            Err(Error::Accounts(A::Denied))
        ));
    }
}

#[test]
fn public_rows_authenticate_before_data_lookup_and_keep_policy_schema_and_input_gates_current() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = setup(dir.path(), false, true);
    let (id, key) = f.store.credentials[0].clone();
    let access = f.first.access.expose().to_owned();
    let a = f.first.metadata.user;
    let before = histories(&f.store);
    let data = Database::open(f.store.root.join("registry").join(&id).join("data")).unwrap();
    for table in ["owned", "missing", "../../private"] {
        assert!(matches!(
            f.root
                .public_user_table(&id, table, "invalid", 50, Op::Get(Key::Integer(1))),
            Err(Error::Accounts(_))
        ));
    }
    assert!(matches!(
        call(&mut f, 0, Op::Get(Key::Integer(1))),
        Err(Error::Transaction(_))
    ));
    drop(data);
    for operation in [
        Op::Page {
            after: None,
            limit: 0,
        },
        Op::Page {
            after: None,
            limit: 129,
        },
        Op::Write(vec![]),
        Op::Get(Key::Text("wrong".into())),
    ] {
        assert!(matches!(
            call(&mut f, 0, operation),
            Err(Error::UserRows(UserRowsError::Input))
        ));
    }
    assert!(
        f.root
            .public_user_table(
                &id,
                emilybase_migrations::LEDGER_TABLE,
                &access,
                50,
                Op::Get(Key::Integer(1))
            )
            .is_err()
    );
    assert!(matches!(
        f.root
            .public_user_table(&id, "t", &access, 50, Op::Get(Key::Integer(1))),
        Err(Error::Accounts(A::PolicyDenied))
    ));
    assert_eq!(histories(&f.store), before);
    call(&mut f, 0, Op::Write(vec![Write::Insert(row(1, &a, 10))])).unwrap();
    let receipt = f.root.row_policy_receipts(&id, &key).unwrap()[0].clone();
    f.root
        .install_row_policy(&id, &key, "owned", receipt.revision, DENY)
        .unwrap();
    assert_eq!(
        call(&mut f, 0, Op::Get(Key::Integer(1))).unwrap(),
        Out::Row(None)
    );
    denied_row(call(
        &mut f,
        0,
        Op::Write(vec![Write::Delete(Key::Integer(1))]),
    ));
    f.root
        .execute(
            &id,
            &key,
            "DROP TABLE owned;CREATE TABLE owned(id INT PRIMARY KEY,owner BYTES,n INT)",
            &[],
        )
        .unwrap();
    assert!(matches!(
        call(&mut f, 0, Op::Get(Key::Integer(1))),
        Err(Error::Accounts(A::PolicyDenied))
    ));
    f.root
        .install_row_policy(&id, &key, "owned", 0, OWN)
        .unwrap();
    assert_eq!(
        call(&mut f, 0, Op::Get(Key::Integer(1))).unwrap(),
        Out::Row(None)
    );
    assert!(f.root.public_user(&id, &access, 50).is_ok());
}
#[test]
fn verified_nonempty_clone_requires_explicit_reopening_and_fresh_login_while_source_remains_active()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = setup(dir.path(), compacted, true);
        let (id, key) = f.store.credentials[0].clone();
        let access = f.first.access.expose().to_owned();
        let a = f.first.metadata.user;
        call(&mut f, 0, Op::Write(vec![Write::Insert(row(1, &a, 10))])).unwrap();
        drop(f.root);
        let before = histories(&f.store);
        let bytes = crate::capture_account_bundle_root(&f.store.root, pool()).unwrap();
        let target = dir.path().join("clone");
        restore_account_bundle_bytes(&bytes, &target, pool(), 40).unwrap();
        let mut copy = AccountRoot::open(&target, pool()).unwrap();
        let closed = copy.public_admission(&id, &key).unwrap();
        assert!(!closed.enabled);
        assert!(matches!(
            copy.public_user(&id, &access, 40),
            Err(Error::Denied)
        ));
        assert!(matches!(
            copy.public_sign_in(&id, LOGIN, PASSWORD, 40),
            Err(Error::Denied)
        ));
        copy.set_public_admission(&id, &key, closed.revision, true)
            .unwrap();
        assert!(copy.public_user(&id, &access, 40).is_err());
        let pair = copy.public_sign_in(&id, LOGIN, PASSWORD, 40).unwrap();
        assert_eq!(
            copy.public_user_table(
                &id,
                "owned",
                pair.access.expose(),
                40,
                Op::Get(Key::Integer(1))
            )
            .unwrap(),
            Out::Row(Some(row(1, &a, 10)))
        );
        assert_eq!(histories(&f.store), before);
        let mut source = AccountRoot::open(&f.store.root, pool()).unwrap();
        assert!(source.public_user(&id, &access, 50).is_ok());
        assert_eq!(
            source
                .public_user_table(&id, "owned", &access, 50, Op::Get(Key::Integer(1)))
                .unwrap(),
            Out::Row(Some(row(1, &a, 10)))
        );
    }
}
#[test]
fn concurrent_current_users_have_one_durable_primary_key_winner_without_a_service_key() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = setup(dir.path(), compacted, true);
        let id = f.store.credentials[0].0.clone();
        let inputs = [
            (f.first.access.expose().to_owned(), f.first.metadata.user),
            (f.second.access.expose().to_owned(), f.second.metadata.user),
        ];
        let root = Arc::new(Mutex::new(f.root));
        let barrier = Arc::new(Barrier::new(3));
        let handles = inputs
            .into_iter()
            .map(|(access, user)| {
                let root = root.clone();
                let barrier = barrier.clone();
                let id = id.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    root.lock().unwrap().public_user_table(
                        &id,
                        "owned",
                        &access,
                        50,
                        Op::Write(vec![Write::Insert(row(1, &user, 10))]),
                    )
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let results = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(Error::UserRows(UserRowsError::Rejected))))
                .count(),
            1
        );
        drop(root);
        let mut root = AccountRoot::open(&f.store.root, pool()).unwrap();
        let mut visible = 0;
        for access in [f.first.access.expose(), f.second.access.expose()] {
            if matches!(
                root.public_user_table(&id, "owned", access, 50, Op::Get(Key::Integer(1)))
                    .unwrap(),
                Out::Row(Some(_))
            ) {
                visible += 1;
            }
        }
        assert_eq!(visible, 1);
    }
}
#[test]
fn generated_user_operations_match_independent_owner_and_suspension_models_across_restart() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compacted in [false, true] {
        let strategy = prop::collection::vec(
            (
                0_u8..5,
                0_i64..6,
                any::<bool>(),
                any::<bool>(),
                any::<bool>(),
            ),
            1..16,
        );
        let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig::with_cases(12));
        runner
            .run(&strategy, |commands| {
                let dir = tempfile::tempdir().unwrap();
                let mut f = setup(dir.path(), compacted, true);
                let (id, key) = f.store.credentials[0].clone();
                let identities = [f.first.metadata.user, f.second.metadata.user];
                let mut model = BTreeMap::<i64, [u8; 16]>::new();
                let mut enabled = true;
                for (kind, pk, second, foreign, requested) in commands {
                    let who = usize::from(second);
                    let actor = identities[who];
                    let owner = identities[usize::from(second ^ foreign)];
                    if kind == 4 {
                        let receipt = f.root.public_admission(&id, &key).unwrap();
                        f.root
                            .set_public_admission(&id, &key, receipt.revision, requested)
                            .unwrap();
                        enabled = requested;
                    } else {
                        let before = histories(&f.store);
                        let operation = match kind {
                            0 => Op::Write(vec![Write::Insert(row(pk, &owner, 10))]),
                            1 => Op::Write(vec![Write::Update {
                                key: Key::Integer(pk),
                                row: row(pk, &owner, 10),
                            }]),
                            2 => Op::Write(vec![Write::Delete(Key::Integer(pk))]),
                            _ => Op::Get(Key::Integer(pk)),
                        };
                        let allowed = enabled
                            && match kind {
                                0 => owner == actor && !model.contains_key(&pk),
                                1 => model.get(&pk) == Some(&actor) && owner == actor,
                                2 => model.get(&pk) == Some(&actor),
                                _ => true,
                            };
                        let result = call(&mut f, who, operation);
                        if !enabled {
                            prop_assert!(matches!(result, Err(Error::Denied)));
                        } else if kind == 3 {
                            prop_assert_eq!(
                                result.unwrap(),
                                Out::Row(
                                    model
                                        .get(&pk)
                                        .filter(|o| **o == actor)
                                        .map(|o| row(pk, o, 10))
                                )
                            );
                        } else if allowed {
                            prop_assert!(
                                matches!(result, Ok(Out::Committed { .. })),
                                "expected committed packet"
                            );
                            if kind == 2 {
                                model.remove(&pk);
                            } else {
                                model.insert(pk, owner);
                            }
                        } else {
                            prop_assert!(result.is_err());
                        }
                        if !allowed || kind == 3 {
                            prop_assert_eq!(histories(&f.store), before);
                        }
                    }
                    prop_assert_eq!(
                        rows(&f),
                        model
                            .iter()
                            .map(|(pk, owner)| row(*pk, owner, 10))
                            .collect::<Vec<_>>()
                    );
                    drop(f.root);
                    f.root = AccountRoot::open(&f.store.root, pool()).unwrap();
                    prop_assert_eq!(f.root.public_admission(&id, &key).unwrap().enabled, enabled);
                }
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn keyless_staged_and_caller_received_packets_recover_on_both_wals_without_service_credentials() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compacted in [false, true] {
        for point in ["user_table_staged", "user_table_ack"] {
            let dir = tempfile::tempdir().unwrap();
            let mut f = setup(dir.path(), compacted, true);
            let a = f.first.metadata.user;
            let id = f.store.credentials[0].0.clone();
            call(&mut f, 0, Op::Write(vec![Write::Insert(row(1, &a, 10))])).unwrap();
            let before = histories(&f.store);
            let input = dir.path().join("synthetic-public-operation.json");
            fs::write(
                &input,
                serde_json::to_vec(&("", f.first.access.expose(), row(2, &a, 20))).unwrap(),
            )
            .unwrap();
            fs::set_permissions(&input, fs::Permissions::from_mode(0o600)).unwrap();
            drop(f.root);
            let worker = Worker::start(&f.store.root, &input, &id, "root-public-user-table", point);
            worker.reach(point);
            worker.kill();
            f.root = AccountRoot::open(&f.store.root, pool()).unwrap();
            let after = histories(&f.store);
            assert_eq!(&before[..2], &after[..2]);
            assert_eq!(&before[3..], &after[3..]);
            if point.ends_with("staged") {
                assert_eq!(histories(&f.store), before);
                assert_eq!(rows(&f), vec![row(1, &a, 10)]);
            } else {
                assert_eq!(rows(&f), vec![row(1, &a, 30), row(2, &a, 20)]);
            }
            assert_eq!(
                call(&mut f, 0, Op::Get(Key::Integer(1))).unwrap(),
                Out::Row(Some(row(
                    1,
                    &a,
                    if point.ends_with("staged") { 10 } else { 30 }
                )))
            );
        }
    }
}
