use super::*;
use crate::{UserRowsError, UserTableOperation as Op, UserTableResult as Out, UserWrite as Write};
use emilybase_auth::{accounts::IssuedSession, row_policy::PolicyError};
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
fn setup(parent: &Path, compact: bool) -> Fixture {
    let store = restored(parent, 2, compact);
    let (id, key) = &store.credentials[0];
    let mut root = AccountRoot::open(&store.root, pool()).unwrap();
    root.create_user(id, key, "second_user", PASSWORD).unwrap();
    let first = root.sign_in(id, key, LOGIN, PASSWORD, 50).unwrap();
    let second = root.sign_in(id, key, "second_user", PASSWORD, 50).unwrap();
    root.execute(
        id,
        key,
        "CREATE TABLE owned(id INT PRIMARY KEY,owner BYTES,amount INT)",
        &[],
    )
    .unwrap();
    root.enable_row_policy_catalog(id, key).unwrap();
    root.install_row_policy(id, key, "owned", 0, OWN).unwrap();
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
    let (id, key) = &f.store.credentials[0];
    let access = if who == 0 {
        f.first.access.expose()
    } else {
        f.second.access.expose()
    };
    f.root.user_table(id, key, "owned", access, 50, operation)
}
fn public(f: &Fixture) -> Vec<u8> {
    fs::read(
        f.store
            .root
            .join("registry")
            .join(&f.store.credentials[0].0)
            .join("data/redo.wal"),
    )
    .unwrap()
}
fn private(f: &Fixture) -> Vec<u8> {
    fs::read(
        f.store
            .root
            .join("private")
            .join(&f.store.credentials[0].0)
            .join("redo.wal"),
    )
    .unwrap()
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
fn denied(result: crate::Result<Out>) {
    assert!(matches!(
        result,
        Err(Error::UserRows(
            UserRowsError::Policy(PolicyError::Denied) | UserRowsError::Rejected
        ))
    ));
}
#[test]
fn user_rows_owned_crud_hidden_reads_and_staged_packet_commit_once_on_both_wals() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = setup(dir.path(), compact);
        let a = f.first.metadata.user;
        let b = f.second.metadata.user;
        let private_before = private(&f);
        let first = call(
            &mut f,
            0,
            Op::Write(vec![
                Write::Insert(row(1, &a, 10)),
                Write::Update {
                    key: Key::Integer(1),
                    row: row(1, &a, 20),
                },
            ]),
        )
        .unwrap();
        assert!(matches!(first, Out::Committed { operations: 2, .. }));
        let before = public(&f);
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
        assert_eq!(public(&f), before);
        denied(call(
            &mut f,
            1,
            Op::Write(vec![Write::Update {
                key: Key::Integer(1),
                row: row(1, &b, 21),
            }]),
        ));
        denied(call(
            &mut f,
            0,
            Op::Write(vec![
                Write::Insert(row(2, &a, 1)),
                Write::Update {
                    key: Key::Integer(1),
                    row: row(1, &b, 99),
                },
            ]),
        ));
        assert_eq!(public(&f), before);
        assert_eq!(rows(&f), vec![row(1, &a, 20)]);
        denied(call(
            &mut f,
            0,
            Op::Write(vec![
                Write::Insert(row(2, &a, 1)),
                Write::Insert(row(1, &a, 99)),
            ]),
        ));
        assert_eq!(public(&f), before);
        denied(call(
            &mut f,
            1,
            Op::Write(vec![Write::Delete(Key::Integer(1))]),
        ));
        assert_eq!(public(&f), before);
        call(&mut f, 0, Op::Write(vec![Write::Delete(Key::Integer(1))])).unwrap();
        assert!(rows(&f).is_empty());
        assert_eq!(private(&f), private_before);
    }
}
#[test]
fn user_rows_current_policy_credentials_scope_and_table_schema_are_checked_each_time() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = setup(dir.path(), compact);
        let a = f.first.metadata.user;
        call(&mut f, 0, Op::Write(vec![Write::Insert(row(1, &a, 10))])).unwrap();
        let (id, mut key) = f.store.credentials[0].clone();
        let other = f.store.credentials[1].clone();
        let before = public(&f);
        for (project, key, token) in [
            (id.as_str(), other.1.as_str(), f.first.access.expose()),
            (other.0.as_str(), other.1.as_str(), f.first.access.expose()),
            (id.as_str(), key.as_str(), f.first.refresh.expose()),
        ] {
            assert!(
                f.root
                    .user_table(project, key, "owned", token, 50, Op::Get(Key::Integer(1)))
                    .is_err()
            );
        }
        assert_eq!(public(&f), before);
        let receipt = f
            .root
            .row_policy_receipts(&id, &key)
            .unwrap()
            .pop()
            .unwrap();
        let deny = f
            .root
            .install_row_policy(&id, &key, "owned", receipt.revision, DENY)
            .unwrap();
        assert_eq!(
            call(&mut f, 0, Op::Get(Key::Integer(1))).unwrap(),
            Out::Row(None)
        );
        denied(call(
            &mut f,
            0,
            Op::Write(vec![Write::Delete(Key::Integer(1))]),
        ));
        assert_eq!(public(&f), before);
        f.root
            .install_row_policy(&id, &key, "owned", deny.revision, OWN)
            .unwrap();
        assert!(matches!(
            call(&mut f, 0, Op::Get(Key::Integer(1))).unwrap(),
            Out::Row(Some(_))
        ));
        f.root.set_disabled(&id, &key, LOGIN, true).unwrap();
        assert!(call(&mut f, 0, Op::Get(Key::Integer(1))).is_err());
        f.root.set_disabled(&id, &key, LOGIN, false).unwrap();
        assert!(call(&mut f, 0, Op::Get(Key::Integer(1))).is_err());
        f.first = f.root.sign_in(&id, &key, LOGIN, PASSWORD, 50).unwrap();
        let refreshed = f
            .root
            .refresh_session(&id, &key, f.first.refresh.expose(), 50)
            .unwrap();
        assert!(call(&mut f, 0, Op::Get(Key::Integer(1))).is_err());
        f.first = refreshed;
        assert!(call(&mut f, 0, Op::Get(Key::Integer(1))).is_ok());
        f.root
            .logout_session(&id, &key, f.first.refresh.expose(), 50)
            .unwrap();
        assert!(call(&mut f, 0, Op::Get(Key::Integer(1))).is_err());
        f.first = f.root.sign_in(&id, &key, LOGIN, PASSWORD, 50).unwrap();
        f.root
            .change_password(&id, &key, LOGIN, PASSWORD, b"synthetic-new-password")
            .unwrap();
        assert!(call(&mut f, 0, Op::Get(Key::Integer(1))).is_err());
        f.first = f
            .root
            .sign_in(&id, &key, LOGIN, b"synthetic-new-password", 50)
            .unwrap();
        let rotated = f.root.rotate_project_key(&id).unwrap();
        assert!(call(&mut f, 0, Op::Get(Key::Integer(1))).is_err());
        key = rotated.api_key;
        f.store.credentials[0].1 = key.clone();
        assert!(matches!(
            call(&mut f, 0, Op::Get(Key::Integer(1))).unwrap(),
            Out::Row(Some(_))
        ));
        f.root
            .execute(
                &id,
                &key,
                "DROP TABLE owned;CREATE TABLE owned(id INT PRIMARY KEY,owner BYTES,amount TEXT)",
                &[],
            )
            .unwrap();
        assert!(matches!(
            call(&mut f, 0, Op::Get(Key::Integer(1))),
            Err(Error::Accounts(
                emilybase_auth::accounts::Error::PolicyDenied
            ))
        ));
    }
}
#[test]
fn user_rows_packet_and_typed_bounds_ledger_exclusion_and_public_private_owner_retention_are_real()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = setup(dir.path(), false);
    let a = f.first.metadata.user;
    let before = public(&f);
    let private_before = private(&f);
    for n in [0, 257] {
        let writes = (0..n).map(|i| Write::Insert(row(i, &a, 1))).collect();
        assert!(matches!(
            call(&mut f, 0, Op::Write(writes)),
            Err(Error::UserRows(UserRowsError::Input))
        ));
        assert_eq!(public(&f), before);
    }
    for bad in [
        vec![Value::Integer(1)],
        row(1, &a, 1).into_iter().chain([Value::Null]).collect(),
        vec![
            Value::Integer(1),
            Value::Bytes(a.to_vec()),
            Value::Text("synthetic-private-row".into()),
        ],
    ] {
        assert!(call(&mut f, 0, Op::Write(vec![Write::Insert(bad)])).is_err());
        assert_eq!(public(&f), before);
    }
    let (id, key) = &f.store.credentials[0];
    for table in [
        emilybase_migrations::LEDGER_TABLE,
        "_EMILYBASE_MIGRATIONS_V1",
    ] {
        assert!(matches!(
            f.root.user_table(
                id,
                key,
                table,
                f.first.access.expose(),
                50,
                Op::Get(Key::Integer(1))
            ),
            Err(Error::UserRows(UserRowsError::Rejected))
        ));
    }
    let (id, key) = &f.store.credentials[0];
    assert!(matches!(
        f.root.user_table(
            id,
            key,
            "owned",
            f.first.access.expose(),
            49,
            Op::Get(Key::Integer(1))
        ),
        Err(Error::Accounts(emilybase_auth::accounts::Error::Clock))
    ));
    assert_eq!(private(&f), private_before);
    let public_path = f.store.root.join("registry").join(id).join("data");
    let private_path = f.store.root.join("private").join(id);
    let project = id.clone();
    let _guard = durability::on_boundary("root_user_table_verified", move || {
        assert!(Database::open(&public_path).is_err());
        assert!(AccountStore::open(&private_path, &project, pool()).is_err());
    });
    assert_eq!(
        call(&mut f, 0, Op::Get(Key::Integer(1))).unwrap(),
        Out::Row(None)
    );
    assert_eq!(
        format!(
            "{:?}",
            Op::Write(vec![Write::Insert(vec![Value::Text(
                "synthetic-private-row".into()
            )])])
        ),
        "UserTableOperation(redacted)"
    );
    call(
        &mut f,
        0,
        Op::Write((0..256).map(|i| Write::Insert(row(i, &a, i))).collect()),
    )
    .unwrap();
    assert_eq!(rows(&f).len(), 256);
}
#[test]
fn user_rows_forward_clock_is_separate_from_rejected_public_write_and_restore_revokes_the_old_scope()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = setup(dir.path(), false);
    let b = f.second.metadata.user;
    let (id, key) = f.store.credentials[0].clone();
    let before = public(&f);
    let private_before = private(&f);
    denied(f.root.user_table(
        &id,
        &key,
        "owned",
        f.first.access.expose(),
        100,
        Op::Write(vec![Write::Insert(row(1, &b, 1))]),
    ));
    assert_eq!(public(&f), before);
    assert_ne!(private(&f), private_before);
    let access = f.first.access.expose().to_owned();
    let source = f.store.root.clone();
    drop(f.root);
    let image = crate::capture_account_bundle_root(&source, pool()).unwrap();
    let target = dir.path().join("user-rows-clone");
    restore_account_bundle_bytes(&image, &target, pool(), 100).unwrap();
    let mut copied = AccountRoot::open(&target, pool()).unwrap();
    assert!(
        copied
            .user_table(&id, &key, "owned", &access, 100, Op::Get(Key::Integer(1)))
            .is_err()
    );
    let fresh = copied.sign_in(&id, &key, LOGIN, PASSWORD, 100).unwrap();
    assert_eq!(
        copied
            .user_table(
                &id,
                &key,
                "owned",
                fresh.access.expose(),
                100,
                Op::Get(Key::Integer(1))
            )
            .unwrap(),
        Out::Row(None)
    );
    let mut source = AccountRoot::open(&source, pool()).unwrap();
    assert!(
        source
            .user_table(&id, &key, "owned", &access, 100, Op::Get(Key::Integer(1)))
            .is_ok()
    );
}

#[test]
fn user_rows_absent_read_still_refuses_a_policy_bound_to_the_wrong_schema() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let f = setup(dir.path(), false);
    let (id, key) = f.store.credentials[0].clone();
    let token = f.first.access.expose().to_owned();
    let public_path = f.store.root.join("registry").join(&id).join("data");
    let data = Database::open(&public_path).unwrap();
    let table = data.view().unwrap().table_id("owned").unwrap();
    let mut stale = data.view().unwrap().schema("owned").unwrap().clone();
    drop(data);
    stale.columns[2].data_type = emilybase_catalog::DataType::Text;
    drop(f.root);
    let mut account =
        AccountStore::open(f.store.root.join("private").join(&id), &id, pool()).unwrap();
    let revision = account.row_policy_receipts().unwrap()[0].revision;
    account
        .install_row_policy(
            emilybase_auth::row_policy::TableContext {
                project: &id,
                id: table,
                schema: &stale,
            },
            revision,
            OWN,
        )
        .unwrap();
    drop(account);
    let mut root = AccountRoot::open(&f.store.root, pool()).unwrap();
    assert!(matches!(
        root.user_table(&id, &key, "owned", &token, 50, Op::Get(Key::Integer(999))),
        Err(Error::UserRows(UserRowsError::Policy(PolicyError::Scope)))
    ));
}

#[test]
fn user_rows_generated_operations_match_an_independent_owner_map_across_restarts_and_both_wals() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let strategy =
            prop::collection::vec((0u8..6, 0i64..8, 0u8..3, -50i64..50, any::<bool>()), 1..16);
        let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig::with_cases(12));
        runner
            .run(&strategy, |commands| {
                let dir = tempfile::tempdir().unwrap();
                let mut f = setup(dir.path(), compact);
                let identities = [f.first.metadata.user, f.second.metadata.user];
                let mut model = BTreeMap::<i64, ([u8; 16], i64)>::new();
                for (step, (kind, pk, owner, value, second)) in commands.into_iter().enumerate() {
                    let who = usize::from(second);
                    let actor = identities[who];
                    let candidate = if owner == 0 {
                        actor.to_vec()
                    } else if owner == 1 {
                        identities[1 - who].to_vec()
                    } else {
                        vec![0; 15]
                    };
                    let before = public(&f);
                    let private_before = private(&f);
                    let accepted = match kind {
                        0 => {
                            let expected = candidate == actor && !model.contains_key(&pk);
                            let result = call(
                                &mut f,
                                who,
                                Op::Write(vec![Write::Insert(row(pk, &candidate, value))]),
                            );
                            prop_assert_eq!(result.is_ok(), expected);
                            if expected {
                                model.insert(pk, (actor, value));
                            }
                            expected
                        }
                        1 => {
                            let expected = candidate == actor
                                && model.get(&pk).is_some_and(|(id, _)| *id == actor);
                            let result = call(
                                &mut f,
                                who,
                                Op::Write(vec![Write::Update {
                                    key: Key::Integer(pk),
                                    row: row(pk, &candidate, value),
                                }]),
                            );
                            prop_assert_eq!(result.is_ok(), expected);
                            if expected {
                                model.insert(pk, (actor, value));
                            }
                            expected
                        }
                        2 => {
                            let expected = model.get(&pk).is_some_and(|(id, _)| *id == actor);
                            let result = call(
                                &mut f,
                                who,
                                Op::Write(vec![Write::Delete(Key::Integer(pk))]),
                            );
                            prop_assert_eq!(result.is_ok(), expected);
                            if expected {
                                model.remove(&pk);
                            }
                            expected
                        }
                        3 => {
                            let expected = model
                                .get(&pk)
                                .filter(|(id, _)| *id == actor)
                                .map(|(id, v)| row(pk, id, *v));
                            prop_assert_eq!(
                                call(&mut f, who, Op::Get(Key::Integer(pk))).unwrap(),
                                Out::Row(expected)
                            );
                            false
                        }
                        4 => {
                            let new = 100 + step as i64;
                            let result = call(
                                &mut f,
                                who,
                                Op::Write(vec![
                                    Write::Insert(row(new, &actor, value)),
                                    Write::Update {
                                        key: Key::Integer(pk),
                                        row: row(pk, &identities[1 - who], value),
                                    },
                                ]),
                            );
                            prop_assert!(result.is_err());
                            false
                        }
                        _ => {
                            let new = 100 + step as i64;
                            call(
                                &mut f,
                                who,
                                Op::Write(vec![
                                    Write::Insert(row(new, &actor, value)),
                                    Write::Update {
                                        key: Key::Integer(new),
                                        row: row(new, &actor, value + 1),
                                    },
                                ]),
                            )
                            .unwrap();
                            model.insert(new, (actor, value + 1));
                            true
                        }
                    };
                    if !accepted {
                        prop_assert_eq!(public(&f), before);
                    }
                    prop_assert_eq!(private(&f), private_before);
                    let expected = model
                        .iter()
                        .map(|(pk, (id, v))| row(*pk, id, *v))
                        .collect::<Vec<_>>();
                    prop_assert_eq!(rows(&f), expected);
                    if step % 3 == 0 {
                        drop(f.root);
                        f.root = AccountRoot::open(&f.store.root, pool()).unwrap();
                    }
                }
                Ok(())
            })
            .unwrap();
    }
}
#[test]
fn user_rows_two_current_owners_serialize_same_key_writes_to_one_winner() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let f = setup(dir.path(), false);
    let credentials = f.store.credentials[0].clone();
    let a = f.first.metadata.user;
    let b = f.second.metadata.user;
    let tokens = [
        f.first.access.expose().to_owned(),
        f.second.access.expose().to_owned(),
    ];
    let root = Arc::new(Mutex::new(f.root));
    let barrier = Arc::new(Barrier::new(3));
    let result = std::thread::scope(|scope| {
        let handles = [a, b]
            .into_iter()
            .zip(tokens.iter())
            .map(|(owner, token)| {
                let root = root.clone();
                let barrier = barrier.clone();
                let credentials = &credentials;
                scope.spawn(move || {
                    barrier.wait();
                    root.lock().unwrap().user_table(
                        &credentials.0,
                        &credentials.1,
                        "owned",
                        token,
                        50,
                        Op::Write(vec![Write::Insert(row(1, &owner, 10))]),
                    )
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(result.iter().filter(|r| r.is_ok()).count(), 1);
    let data = Database::open(
        f.store
            .root
            .join("registry")
            .join(&credentials.0)
            .join("data"),
    )
    .unwrap();
    assert_eq!(data.view().unwrap().scan("owned", 2).unwrap().len(), 1);
}
#[test]
fn user_rows_controlled_staged_and_received_result_kills_recover_only_the_atomic_packet_on_both_wals()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for point in ["user_table_staged", "user_table_ack"] {
            let dir = tempfile::tempdir().unwrap();
            let mut f = setup(dir.path(), compact);
            let a = f.first.metadata.user;
            call(&mut f, 0, Op::Write(vec![Write::Insert(row(1, &a, 10))])).unwrap();
            let before = public(&f);
            let private_before = private(&f);
            let (id, key) = f.store.credentials[0].clone();
            let input = dir.path().join("synthetic-user-operation.json");
            fs::write(
                &input,
                serde_json::to_vec(&(key.clone(), f.first.access.expose(), row(2, &a, 20)))
                    .unwrap(),
            )
            .unwrap();
            fs::set_permissions(&input, fs::Permissions::from_mode(0o600)).unwrap();
            drop(f.root);
            let worker = Worker::start(&f.store.root, &input, &id, "root-user-table", point);
            worker.reach(point);
            worker.kill();
            f.root = AccountRoot::open(&f.store.root, pool()).unwrap();
            assert_eq!(private(&f), private_before);
            if point.ends_with("staged") {
                assert_eq!(public(&f), before);
                assert_eq!(rows(&f), vec![row(1, &a, 10)]);
            } else {
                assert_eq!(rows(&f), vec![row(1, &a, 30), row(2, &a, 20)]);
            }
        }
    }
}

#[test]
fn user_rows_update_using_and_check_do_not_gain_an_implicit_select_requirement_or_allow_primary_key_changes()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = setup(dir.path(), false);
    let a = f.first.metadata.user;
    call(&mut f, 0, Op::Write(vec![Write::Insert(row(1, &a, 10))])).unwrap();
    let mut definition: serde_json::Value = serde_json::from_slice(OWN).unwrap();
    definition["select"] = serde_json::json!({"kind":"deny"});
    let document = serde_json::to_vec(&definition).unwrap();
    let (id, key) = &f.store.credentials[0];
    let current = f.root.row_policy_receipts(id, key).unwrap()[0].revision;
    f.root
        .install_row_policy(id, key, "owned", current, &document)
        .unwrap();
    assert_eq!(
        call(&mut f, 0, Op::Get(Key::Integer(1))).unwrap(),
        Out::Row(None)
    );
    call(
        &mut f,
        0,
        Op::Write(vec![Write::Update {
            key: Key::Integer(1),
            row: row(1, &a, 20),
        }]),
    )
    .unwrap();
    assert_eq!(rows(&f), vec![row(1, &a, 20)]);
    let before = public(&f);
    assert!(matches!(
        call(
            &mut f,
            0,
            Op::Write(vec![
                Write::Insert(row(2, &a, 1)),
                Write::Update {
                    key: Key::Integer(1),
                    row: row(3, &a, 30)
                }
            ])
        ),
        Err(Error::UserRows(UserRowsError::Policy(PolicyError::Row)))
    ));
    assert_eq!(public(&f), before);
    assert!(matches!(
        call(
            &mut f,
            0,
            Op::Write(vec![Write::Insert(row(4, &[0; 3073], 1))])
        ),
        Err(Error::UserRows(UserRowsError::Policy(PolicyError::Row)))
    ));
    assert_eq!(public(&f), before);
}

mod pages;
