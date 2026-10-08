use super::*;

fn page(f: &mut Fixture, who: usize, after: Option<Key>, limit: usize) -> (Vec<Row>, Option<Key>) {
    match call(f, who, Op::Page { after, limit }).unwrap() {
        Out::Page { rows, next } => (rows, next),
        other => panic!("unexpected page result: {other:?}"),
    }
}
fn insert(f: &mut Fixture, who: usize, keys: impl IntoIterator<Item = i64>) {
    let user = if who == 0 {
        f.first.metadata.user
    } else {
        f.second.metadata.user
    };
    let operations = keys
        .into_iter()
        .map(|key| Write::Insert(row(key, &user, 1)))
        .collect::<Vec<_>>();
    if !operations.is_empty() {
        call(f, who, Op::Write(operations)).unwrap();
    }
}
#[test]
fn user_pages_maximum_visible_lookahead_hidden_gaps_and_tail_never_disclose_hidden_keys() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = setup(dir.path(), compact);
        insert(&mut f, 0, (0..130).map(|n| n * 2));
        insert(
            &mut f,
            1,
            (0..130).map(|n| n * 2 + 1).chain([-100, -1, i64::MAX]),
        );
        let before = public(&f);
        let private_before = private(&f);
        let (first, next) = page(&mut f, 0, None, crate::MAX_USER_PAGE_ROWS);
        assert_eq!(first.len(), 128);
        assert_eq!(first.first().unwrap()[0], Value::Integer(0));
        assert_eq!(first.last().unwrap()[0], Value::Integer(254));
        assert_eq!(next, Some(Key::Integer(254)));
        let a = f.first.metadata.user;
        assert!(first.iter().all(|r| r[1] == Value::Bytes(a.to_vec())));
        assert_eq!(
            page(&mut f, 0, next, 128),
            (vec![row(256, &a, 1), row(258, &a, 1)], None)
        );
        // A full final visible page followed only by hidden rows has no continuation.
        assert_eq!(page(&mut f, 0, Some(Key::Integer(254)), 2).1, None);
        assert_eq!(page(&mut f, 0, Some(Key::Integer(258)), 1), (vec![], None));
        assert_eq!(
            page(&mut f, 0, Some(Key::Integer(i64::MAX)), 1),
            (vec![], None)
        );
        let b = f.second.metadata.user;
        assert_eq!(
            page(&mut f, 1, None, 1),
            (vec![row(-100, &b, 1)], Some(Key::Integer(-100)))
        );
        assert_eq!(public(&f), before);
        assert_eq!(private(&f), private_before);
        assert_eq!(
            format!(
                "{:?}",
                Out::Page {
                    rows: first,
                    next: Some(Key::Integer(254))
                }
            ),
            "UserTableResult(redacted)"
        );
    }
}
#[test]
fn user_pages_deleted_cursor_and_current_policy_session_and_fresh_table_apply_each_time() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = setup(dir.path(), compact);
        insert(&mut f, 0, [1, 3, 5]);
        insert(&mut f, 1, [2, 4, 6]);
        let (first, next) = page(&mut f, 0, None, 1);
        let a = f.first.metadata.user;
        assert_eq!(first, vec![row(1, &a, 1)]);
        assert_eq!(next, Some(Key::Integer(1)));
        call(
            &mut f,
            0,
            Op::Write(vec![
                Write::Delete(Key::Integer(1)),
                Write::Insert(row(0, &a, 1)),
                Write::Insert(row(7, &a, 1)),
            ]),
        )
        .unwrap();
        drop(f.root);
        f.root = AccountRoot::open(&f.store.root, pool()).unwrap();
        assert_eq!(
            page(&mut f, 0, next, 3),
            (vec![row(3, &a, 1), row(5, &a, 1), row(7, &a, 1)], None)
        );
        let (id, key) = f.store.credentials[0].clone();
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
        let before = public(&f);
        let private_before = private(&f);
        assert_eq!(page(&mut f, 0, None, 128), (vec![], None));
        assert_eq!(public(&f), before);
        assert_eq!(private(&f), private_before);
        f.root
            .install_row_policy(&id, &key, "owned", deny.revision, OWN)
            .unwrap();
        assert_eq!(
            page(&mut f, 0, Some(Key::Integer(3)), 1),
            (vec![row(5, &a, 1)], Some(Key::Integer(5)))
        );
        f.root
            .logout_session(&id, &key, f.first.refresh.expose(), 50)
            .unwrap();
        assert!(
            call(
                &mut f,
                0,
                Op::Page {
                    after: None,
                    limit: 1
                }
            )
            .is_err()
        );
        f.first = f.root.sign_in(&id, &key, LOGIN, PASSWORD, 50).unwrap();
        f.root
            .execute(
                &id,
                &key,
                "DROP TABLE owned; CREATE TABLE owned(id INT PRIMARY KEY,owner BYTES,amount INT)",
                &[],
            )
            .unwrap();
        assert!(
            call(
                &mut f,
                0,
                Op::Page {
                    after: None,
                    limit: 1
                }
            )
            .is_err()
        );
        f.root
            .install_row_policy(&id, &key, "owned", 0, OWN)
            .unwrap();
        assert_eq!(page(&mut f, 0, Some(Key::Integer(5)), 1), (vec![], None));
    }
}
#[test]
fn user_pages_limits_key_types_empty_end_bounds_and_complete_schema_are_checked() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = setup(dir.path(), false);
    let (id, key) = f.store.credentials[0].clone();
    let before = public(&f);
    let private_before = private(&f);
    for limit in [0, 129, usize::MAX] {
        assert!(matches!(
            f.root.user_table(
                &id,
                &key,
                "owned",
                "synthetic-invalid-token",
                100,
                Op::Page { after: None, limit }
            ),
            Err(Error::UserRows(UserRowsError::Input))
        ));
    }
    for after in [Key::Text("wrong-type".into()), Key::Text("x".repeat(3073))] {
        assert!(
            call(
                &mut f,
                0,
                Op::Page {
                    after: Some(after),
                    limit: 1
                }
            )
            .is_err()
        );
    }
    assert_eq!(page(&mut f, 0, None, 1), (vec![], None));
    assert_eq!(
        page(&mut f, 0, Some(Key::Integer(i64::MAX)), 128),
        (vec![], None)
    );
    assert_eq!(public(&f), before);
    assert_eq!(private(&f), private_before);
    let data = Database::open(f.store.root.join("registry").join(&id).join("data")).unwrap();
    let table = data.view().unwrap().table_id("owned").unwrap();
    let mut schema = data.view().unwrap().schema("owned").unwrap().clone();
    drop(data);
    schema.columns[2].data_type = emilybase_catalog::DataType::Text;
    drop(f.root);
    let mut accounts =
        AccountStore::open(f.store.root.join("private").join(&id), &id, pool()).unwrap();
    let revision = accounts.row_policy_receipts().unwrap()[0].revision;
    accounts
        .install_row_policy(
            emilybase_auth::row_policy::TableContext {
                project: &id,
                id: table,
                schema: &schema,
            },
            revision,
            OWN,
        )
        .unwrap();
    drop(accounts);
    f.root = AccountRoot::open(&f.store.root, pool()).unwrap();
    let before = public(&f);
    let private_before = private(&f);
    for after in [None, Some(Key::Integer(i64::MAX))] {
        assert!(matches!(
            call(&mut f, 0, Op::Page { after, limit: 1 }),
            Err(Error::UserRows(UserRowsError::Policy(PolicyError::Scope)))
        ));
    }
    assert_eq!(public(&f), before);
    assert_eq!(private(&f), private_before);
}
#[test]
fn user_pages_signed_extremes_and_long_unicode_nul_text_keys_use_visible_exclusive_order() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = setup(dir.path(), compact);
        let a = f.first.metadata.user;
        insert(&mut f, 0, [i64::MIN, 9_007_199_254_740_993, i64::MAX]);
        insert(&mut f, 1, [i64::MIN + 1, i64::MAX - 1]);
        assert_eq!(
            page(&mut f, 0, None, 2),
            (
                vec![row(i64::MIN, &a, 1), row(9_007_199_254_740_993, &a, 1)],
                Some(Key::Integer(9_007_199_254_740_993))
            )
        );
        assert_eq!(
            page(&mut f, 0, Some(Key::Integer(i64::MAX - 1)), 1),
            (vec![row(i64::MAX, &a, 1)], None)
        );
        let (id, key) = f.store.credentials[0].clone();
        f.root
            .execute(
                &id,
                &key,
                "CREATE TABLE text_owned(id TEXT PRIMARY KEY,owner BYTES,amount INT)",
                &[],
            )
            .unwrap();
        f.root
            .install_row_policy(&id, &key, "text_owned", 0, OWN)
            .unwrap();
        let keys = [
            String::new(),
            "\0".repeat(3072),
            "a".repeat(3072),
            "界".repeat(1024),
        ];
        let b = f.second.metadata.user;
        let values = keys
            .iter()
            .enumerate()
            .map(|(i, k)| {
                vec![
                    Value::Text(k.clone()),
                    Value::Bytes(if i == 2 { b.to_vec() } else { a.to_vec() }),
                    Value::Integer(1),
                ]
            })
            .collect::<Vec<_>>();
        for who in 0..2 {
            let operations = values
                .iter()
                .enumerate()
                .filter(|(i, _)| (*i == 2) == (who == 1))
                .map(|(_, r)| Write::Insert(r.clone()))
                .collect();
            let access = if who == 0 {
                f.first.access.expose()
            } else {
                f.second.access.expose()
            };
            f.root
                .user_table(&id, &key, "text_owned", access, 50, Op::Write(operations))
                .unwrap();
        }
        let before = public(&f);
        let private_before = private(&f);
        let first = f
            .root
            .user_table(
                &id,
                &key,
                "text_owned",
                f.first.access.expose(),
                50,
                Op::Page {
                    after: None,
                    limit: 2,
                },
            )
            .unwrap();
        assert_eq!(
            first,
            Out::Page {
                rows: vec![values[0].clone(), values[1].clone()],
                next: Some(Key::Text(keys[1].clone()))
            }
        );
        f.root
            .user_table(
                &id,
                &key,
                "text_owned",
                f.first.access.expose(),
                50,
                Op::Write(vec![Write::Delete(Key::Text(keys[1].clone()))]),
            )
            .unwrap();
        let after_delete = public(&f);
        assert_ne!(after_delete, before);
        let rest = f
            .root
            .user_table(
                &id,
                &key,
                "text_owned",
                f.first.access.expose(),
                50,
                Op::Page {
                    after: Some(Key::Text(keys[1].clone())),
                    limit: 2,
                },
            )
            .unwrap();
        assert_eq!(
            rest,
            Out::Page {
                rows: vec![values[3].clone()],
                next: None
            }
        );
        assert!(
            f.root
                .user_table(
                    &id,
                    &key,
                    "text_owned",
                    f.first.access.expose(),
                    50,
                    Op::Page {
                        after: Some(Key::Text("x".repeat(3073))),
                        limit: 1
                    }
                )
                .is_err()
        );
        assert_eq!(public(&f), after_delete);
        assert_eq!(private(&f), private_before);
    }
}
#[test]
fn user_pages_generated_current_owner_maps_match_independent_visible_keyset_model() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let strategy = (
            prop::collection::vec((-30i64..30, any::<bool>(), -20i64..20), 0..50),
            prop::collection::vec(
                (any::<bool>(), prop::option::of(-35i64..35), 1usize..9),
                1..12,
            ),
        );
        let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig::with_cases(8));
        runner
            .run(&strategy, |(input, requests)| {
                let dir = tempfile::tempdir().unwrap();
                let mut f = setup(dir.path(), compact);
                let actors = [f.first.metadata.user, f.second.metadata.user];
                let model = input
                    .into_iter()
                    .map(|(key, second, value)| (key, (usize::from(second), value)))
                    .collect::<BTreeMap<_, _>>();
                for (who, actor) in actors.iter().enumerate() {
                    let operations = model
                        .iter()
                        .filter(|(_, (owner, _))| *owner == who)
                        .map(|(key, (_, value))| Write::Insert(row(*key, actor, *value)))
                        .collect::<Vec<_>>();
                    if !operations.is_empty() {
                        call(&mut f, who, Op::Write(operations)).unwrap();
                    }
                }
                let before = public(&f);
                let private_before = private(&f);
                for (step, (second, after, limit)) in requests.into_iter().enumerate() {
                    let who = usize::from(second);
                    let visible = model
                        .iter()
                        .filter(|(key, (owner, _))| {
                            *owner == who && after.is_none_or(|a| **key > a)
                        })
                        .map(|(key, (_, value))| row(*key, &actors[who], *value))
                        .collect::<Vec<_>>();
                    let expected = visible.iter().take(limit).cloned().collect::<Vec<_>>();
                    let next = if visible.len() > limit {
                        match &expected.last().unwrap()[0] {
                            Value::Integer(key) => Some(Key::Integer(*key)),
                            _ => unreachable!(),
                        }
                    } else {
                        None
                    };
                    prop_assert_eq!(
                        page(&mut f, who, after.map(Key::Integer), limit),
                        (expected, next)
                    );
                    prop_assert_eq!(public(&f), before.clone());
                    prop_assert_eq!(private(&f), private_before.clone());
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
fn user_pages_verified_common_restore_preserves_filtered_data_but_requires_new_session() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = setup(dir.path(), compact);
        insert(&mut f, 0, [1, 3, 5]);
        insert(&mut f, 1, [2, 4, 6]);
        let expected = page(&mut f, 0, Some(Key::Integer(1)), 1);
        let (id, key) = f.store.credentials[0].clone();
        let access = f.first.access.expose().to_owned();
        drop(f.root);
        let image = crate::capture_account_bundle_root(&f.store.root, pool()).unwrap();
        let target = dir.path().join("user-pages-clone");
        restore_account_bundle_bytes(&image, &target, pool(), 50).unwrap();
        let mut copied = AccountRoot::open(&target, pool()).unwrap();
        assert!(
            copied
                .user_table(
                    &id,
                    &key,
                    "owned",
                    &access,
                    50,
                    Op::Page {
                        after: Some(Key::Integer(1)),
                        limit: 1
                    }
                )
                .is_err()
        );
        let fresh = copied.sign_in(&id, &key, LOGIN, PASSWORD, 50).unwrap();
        let result = copied
            .user_table(
                &id,
                &key,
                "owned",
                fresh.access.expose(),
                50,
                Op::Page {
                    after: Some(Key::Integer(1)),
                    limit: 1,
                },
            )
            .unwrap();
        assert_eq!(
            result,
            Out::Page {
                rows: expected.0,
                next: expected.1
            }
        );
        let mut source = AccountRoot::open(&f.store.root, pool()).unwrap();
        assert!(
            source
                .user_table(
                    &id,
                    &key,
                    "owned",
                    &access,
                    50,
                    Op::Page {
                        after: None,
                        limit: 1
                    }
                )
                .is_ok()
        );
    }
}

#[test]
fn user_pages_full_source_of_hidden_rows_yields_no_scan_watermark_or_extra_page() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = setup(dir.path(), false);
    let b = f.second.metadata.user;
    let path = f
        .store
        .root
        .join("registry")
        .join(&f.store.credentials[0].0)
        .join("data");
    // A trusted fixture writer fills the real engine to its existing source cap.
    // It does not alter production authorization or bypass an active owner.
    let mut db = Database::open(path).unwrap();
    let mut clean = db.begin().unwrap();
    clean.drop_table("t").unwrap();
    clean.commit().unwrap();
    let total = emilybase_database::MAX_ROWS;
    for begin in (0..total).step_by(256) {
        let mut tx = db.begin().unwrap();
        for pk in begin..(begin + 256).min(total) {
            tx.insert("owned", row(pk as i64, &b, 1)).unwrap();
        }
        tx.commit().unwrap();
    }
    drop(db);
    let before = public(&f);
    let private_before = private(&f);
    assert_eq!(page(&mut f, 0, None, 128), (vec![], None));
    assert_eq!(page(&mut f, 0, Some(Key::Integer(0)), 1), (vec![], None));
    assert_eq!(public(&f), before);
    assert_eq!(private(&f), private_before);
}
#[test]
fn user_pages_large_physical_rows_retain_exact_visible_limit_and_exclude_lookahead_payload() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = setup(dir.path(), false);
    let (id, key) = f.store.credentials[0].clone();
    f.root
        .execute(
            &id,
            &key,
            "CREATE TABLE large_owned(id INT PRIMARY KEY,owner BYTES,payload TEXT)",
            &[],
        )
        .unwrap();
    f.root
        .install_row_policy(&id, &key, "large_owned", 0, OWN)
        .unwrap();
    let a = f.first.metadata.user;
    let payload = "界".repeat(1024);
    let make = |pk| {
        vec![
            Value::Integer(pk),
            Value::Bytes(a.to_vec()),
            Value::Text(payload.clone()),
        ]
    };
    f.root
        .user_table(
            &id,
            &key,
            "large_owned",
            f.first.access.expose(),
            50,
            Op::Write((0..129).map(|pk| Write::Insert(make(pk))).collect()),
        )
        .unwrap();
    let before = public(&f);
    let private_before = private(&f);
    let result = f
        .root
        .user_table(
            &id,
            &key,
            "large_owned",
            f.first.access.expose(),
            50,
            Op::Page {
                after: None,
                limit: 128,
            },
        )
        .unwrap();
    let Out::Page { rows, next } = result else {
        panic!("expected page");
    };
    assert_eq!(rows, (0..128).map(make).collect::<Vec<_>>());
    assert_eq!(next, Some(Key::Integer(127)));
    let encoded_bytes = rows
        .iter()
        .map(|r| emilybase_catalog::encode_row(r).unwrap().len())
        .sum::<usize>();
    assert!(encoded_bytes <= crate::MAX_USER_PAGE_ROWS * emilybase_catalog::MAX_ENCODED_BYTES);
    assert_eq!(
        f.root
            .user_table(
                &id,
                &key,
                "large_owned",
                f.first.access.expose(),
                50,
                Op::Page {
                    after: next,
                    limit: 128
                }
            )
            .unwrap(),
        Out::Page {
            rows: vec![make(128)],
            next: None
        }
    );
    assert_eq!(public(&f), before);
    assert_eq!(private(&f), private_before);
}
