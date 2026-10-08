use super::*;
use crate::row_policy::{Change, PolicyError, TableContext, records};
use emilybase_catalog::{Column, DataType, Schema};
const PROJECT: &str = "11111111111111111111111111111111";
pub(super) const DENY:&[u8]=br#"{"version":1,"select":{"kind":"deny"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}}"#;
pub(super) const OWN:&[u8]=br#"{"version":1,"select":{"kind":"owner","column":"owner"},"insert":{"kind":"owner","column":"owner"},"update_using":{"kind":"owner","column":"owner"},"update_check":{"kind":"owner","column":"owner"},"delete":{"kind":"owner","column":"owner"}}"#;
pub(super) fn schema() -> Schema {
    Schema {
        name: "items".into(),
        columns: vec![
            Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
            Column {
                name: "owner".into(),
                data_type: DataType::Bytes,
                nullable: false,
            },
        ],
        primary_key: 0,
    }
}
pub(super) fn context(schema: &Schema) -> TableContext<'_> {
    TableContext {
        project: PROJECT,
        id: 7,
        schema,
    }
}
fn create(path: &Path, version: u16) -> AccountStore {
    let mut store = AccountStore::create(path, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
    store
        .create_user("synthetic", b"synthetic-password")
        .unwrap();
    if version == 2 {
        store.enable_session_storage().unwrap();
    }
    if version >= 3 {
        store.enable_session_clock(100).unwrap();
    }
    if version == 4 {
        store.enable_row_policy_catalog().unwrap();
    }
    store
}
fn inventory(store: &mut AccountStore) -> PrivateArchiveReport {
    let bytes = store.backup_image().unwrap();
    inspect_private_account_backup_bytes(&bytes, PROJECT).unwrap()
}
#[test]
fn explicit_v4_migration_preserves_session_clock_and_ownership_and_identical_retry_writes_nothing()
{
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    for format in [1, 2] {
        let path = dir.path().join(format!("private-{format}"));
        let mut store = create(&path, 3);
        if format == 2 {
            store.compact().unwrap();
        }
        let pair = store
            .sign_in("synthetic", b"synthetic-password", 100)
            .unwrap();
        let before_clock = store.session_clock_floor().unwrap();
        let before_scope = store.session_scope.clone();
        let before = store.database.last_transaction();
        store.enable_row_policy_catalog().unwrap();
        assert_eq!(store.database.last_transaction(), before + 1);
        assert_eq!(store.session_clock_floor().unwrap(), before_clock);
        assert_eq!(store.session_scope, before_scope);
        assert!(store.verify_access(pair.access.expose(), 100).is_ok());
        assert_eq!(inventory(&mut store).private_version, 4);
        assert!(store.row_policy_receipts().unwrap().is_empty());
        let wal = store.database.committed_wal().unwrap();
        store.enable_row_policy_catalog().unwrap();
        assert_eq!(store.database.committed_wal().unwrap(), wal);
        drop(store);
        let mut store = AccountStore::open(&path, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
        assert!(store.verify_access(pair.access.expose(), 100).is_ok());
        assert!(store.row_policy_receipts().unwrap().is_empty());
    }
}
#[test]
fn v1_v2_and_v3_policy_calls_refuse_without_implicit_schema_or_clock_changes() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let schema = schema();
    for version in [1, 2, 3] {
        let mut store = create(&dir.path().join(format!("private-{version}")), version);
        let wal = store.database.committed_wal().unwrap();
        assert!(matches!(
            store.row_policy_receipts(),
            Err(Error::PolicySchema)
        ));
        assert!(matches!(
            store.install_row_policy(context(&schema), 0, DENY),
            Err(Error::PolicySchema)
        ));
        assert!(matches!(
            store.verify_row_policy_access("synthetic-invalid", 100, 7),
            Err(Error::PolicySchema)
        ));
        if version < 3 {
            assert!(matches!(
                store.enable_row_policy_catalog(),
                Err(Error::PolicySchema)
            ));
        }
        assert_eq!(store.database.committed_wal().unwrap(), wal);
        assert_eq!(inventory(&mut store).private_version, version);
    }
}
#[test]
fn atomic_install_and_replacement_use_actual_commit_ids_and_exact_expected_retry_metadata() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let schema = schema();
    let mut store = create(&dir.path().join("private"), 4);
    let next = store.database.last_transaction() + 1;
    let first = store.install_row_policy(context(&schema), 0, OWN).unwrap();
    assert_eq!(first.revision, next);
    assert_eq!(first.previous, 0);
    assert_eq!(store.database.last_transaction(), next);
    let before = store.database.committed_wal().unwrap();
    assert_eq!(
        store.install_row_policy(context(&schema), 0, OWN).unwrap(),
        first
    );
    assert_eq!(
        store
            .install_row_policy(context(&schema), first.revision, OWN)
            .unwrap(),
        first
    );
    assert_eq!(store.database.committed_wal().unwrap(), before);
    assert!(matches!(
        store.install_row_policy(context(&schema), 0, DENY),
        Err(Error::PolicyConflict)
    ));
    assert!(matches!(
        store.install_row_policy(context(&schema), first.revision, b"synthetic-invalid"),
        Err(Error::Policy(_))
    ));
    assert!(matches!(
        store.install_row_policy(
            TableContext {
                project: "22222222222222222222222222222222",
                ..context(&schema)
            },
            first.revision,
            DENY
        ),
        Err(Error::ScopeMismatch)
    ));
    assert_eq!(store.database.committed_wal().unwrap(), before);
    store.advance_session_clock(101).unwrap();
    let next = store.database.last_transaction() + 1;
    let second = store
        .install_row_policy(context(&schema), first.revision, DENY)
        .unwrap();
    assert_eq!(second.revision, next);
    assert_eq!(second.previous, first.revision);
    assert!(second.revision > first.revision + 1);
    let before = store.database.committed_wal().unwrap();
    assert_eq!(
        store
            .install_row_policy(context(&schema), first.revision, DENY)
            .unwrap(),
        second
    );
    assert!(matches!(
        store.install_row_policy(context(&schema), 0, OWN),
        Err(Error::PolicyConflict)
    ));
    assert_eq!(store.database.committed_wal().unwrap(), before);
    assert_eq!(store.row_policy_receipts().unwrap(), vec![second]);
}
#[test]
fn current_borrowed_policy_reflects_replacement_without_reissuing_user_session_and_absence_denies()
{
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let schema = schema();
    let mut store = create(&dir.path().join("private"), 4);
    let pair = store
        .sign_in("synthetic", b"synthetic-password", 100)
        .unwrap();
    assert!(matches!(
        store.verify_row_policy_access(pair.access.expose(), 100, 7),
        Err(Error::PolicyDenied)
    ));
    let first = store.install_row_policy(context(&schema), 0, OWN).unwrap();
    let user = store
        .check_password("synthetic", b"synthetic-password")
        .unwrap()
        .unwrap();
    let row = vec![Value::Integer(1), Value::Bytes(user.id.to_vec())];
    {
        let proof = store
            .verify_row_policy_access(pair.access.expose(), 100, 7)
            .unwrap();
        assert_eq!(proof.receipt(), first);
        assert!(
            proof
                .authorize(context(&schema), Change::Select(&row))
                .is_ok()
        );
        assert!(matches!(
            proof.authorize(
                TableContext {
                    id: 8,
                    ..context(&schema)
                },
                Change::Select(&row)
            ),
            Err(PolicyError::Scope)
        ));
        assert!(!format!("{proof:?}").contains(PROJECT));
    }
    store
        .install_row_policy(context(&schema), first.revision, DENY)
        .unwrap();
    assert!(store.verify_access(pair.access.expose(), 100).is_ok());
    {
        let proof = store
            .verify_row_policy_access(pair.access.expose(), 100, 7)
            .unwrap();
        assert!(matches!(
            proof.authorize(context(&schema), Change::Select(&row)),
            Err(PolicyError::Denied)
        ));
    }
    store.logout_session(pair.refresh.expose(), 101).unwrap();
    assert!(matches!(
        store.verify_row_policy_access(pair.access.expose(), 101, 7),
        Err(Error::Denied)
    ));
    assert!(
        store
            .verify_row_policy_access(pair.refresh.expose(), 101, 7)
            .is_err()
    );
}
#[test]
fn long_record_groups_replace_without_orphans_and_private_restore_keeps_policies_but_revokes_old_sessions()
 {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let schema = schema();
    for format in [1, 2] {
        let path = dir.path().join(format!("private-{format}"));
        let mut store = create(&path, 4);
        if format == 2 {
            store.compact().unwrap();
        }
        let pair = store
            .sign_in("synthetic", b"synthetic-password", 100)
            .unwrap();
        let mut padded = OWN.to_vec();
        padded.resize(crate::row_policy::MAX_DOCUMENT_BYTES, b' ');
        let first = store
            .install_row_policy(context(&schema), 0, &padded)
            .unwrap();
        assert!(
            store
                .database
                .view()
                .unwrap()
                .scan(&records::chunk_schema().name, 10)
                .unwrap()
                .len()
                > 1
        );
        let second = store
            .install_row_policy(context(&schema), first.revision, OWN)
            .unwrap();
        assert_eq!(
            store
                .database
                .view()
                .unwrap()
                .scan(&records::chunk_schema().name, 10)
                .unwrap()
                .len(),
            1
        );
        let receipts = store.row_policy_receipts().unwrap();
        assert_eq!(receipts, vec![second]);
        let before = store.database.committed_wal().unwrap();
        let archive = dir.path().join(format!("backup-{format}"));
        store.backup(&archive).unwrap();
        assert_eq!(store.database.committed_wal().unwrap(), before);
        let copy = dir.path().join(format!("copy-{format}"));
        restore_private_accounts(&archive, &copy, PROJECT, PasswordPool::new(1).unwrap(), 200)
            .unwrap();
        let mut copy = AccountStore::open(&copy, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
        assert_eq!(inventory(&mut copy).private_version, 4);
        assert_eq!(copy.row_policy_receipts().unwrap(), receipts);
        assert!(
            copy.verify_row_policy_access(pair.access.expose(), 200, 7)
                .is_err()
        );
        assert!(copy.refresh_session(pair.refresh.expose(), 200).is_err());
        let fresh = copy
            .sign_in("synthetic", b"synthetic-password", 200)
            .unwrap();
        assert!(
            copy.verify_row_policy_access(fresh.access.expose(), 200, 7)
                .is_ok()
        );
        assert!(
            store
                .verify_row_policy_access(pair.access.expose(), 100, 7)
                .is_ok()
        );
    }
}
#[test]
fn complete_inventory_rejects_orphan_chunks_future_revisions_and_semantic_damage_without_repair() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let schema = schema();
    for defect in 0..5 {
        let path = dir.path().join(format!("private-{defect}"));
        let mut store = create(&path, 4);
        store.install_row_policy(context(&schema), 0, OWN).unwrap();
        let mut tx = store.database.begin().unwrap();
        match defect {
            0 => {
                tx.insert(
                    &records::chunk_schema().name,
                    vec![Value::Text("8:0".into()), Value::Bytes(vec![1])],
                )
                .unwrap();
            }
            1 => {
                let bad = records::encode(context(&schema), u64::MAX, 0, OWN).unwrap();
                tx.update(
                    &records::header_schema().name,
                    &Key::Text("7".into()),
                    bad.header().to_vec(),
                )
                .unwrap();
            }
            2 => {
                tx.delete(&records::chunk_schema().name, &Key::Text("7:0".into()))
                    .unwrap();
            }
            3 => {
                let mut header = tx
                    .view()
                    .unwrap()
                    .get(&records::header_schema().name, &Key::Text("7".into()))
                    .unwrap()
                    .unwrap()
                    .clone();
                header[5] = Value::Integer(4001);
                tx.update(
                    &records::header_schema().name,
                    &Key::Text("7".into()),
                    header,
                )
                .unwrap();
            }
            _ => {
                tx.create_table(Schema {
                    name: "unexpected".into(),
                    columns: vec![Column {
                        name: "id".into(),
                        data_type: DataType::Integer,
                        nullable: false,
                    }],
                    primary_key: 0,
                })
                .unwrap();
            }
        };
        tx.commit().unwrap();
        let before = store.database.committed_wal().unwrap();
        assert!(store.row_policy_receipts().is_err(), "defect {defect}");
        assert!(store.backup_image().is_err());
        assert!(store.enable_row_policy_catalog().is_err());
        assert_eq!(store.database.committed_wal().unwrap(), before);
        drop(store);
        assert!(AccountStore::open(&path, PROJECT, PasswordPool::new(1).unwrap()).is_err());
        let mut raw = Database::open(&path).unwrap();
        assert_eq!(raw.committed_wal().unwrap(), before);
    }
}

#[test]
fn bounded_policy_capacity_and_maximum_table_identity_refuse_the_next_install_without_writes() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut store = create(&dir.path().join("private"), 4);
    let schema = schema();
    for table in 1..=MAX_ROW_POLICIES as u64 {
        let context = TableContext {
            id: if table == MAX_ROW_POLICIES as u64 {
                u64::MAX
            } else {
                table
            },
            ..context(&schema)
        };
        store.install_row_policy(context, 0, DENY).unwrap();
    }
    let listed = store.row_policy_receipts().unwrap();
    assert_eq!(listed.len(), MAX_ROW_POLICIES);
    assert_eq!(listed.last().unwrap().table, u64::MAX);
    assert!(listed.windows(2).all(|p| p[0].table < p[1].table));
    let before = store.database.committed_wal().unwrap();
    assert!(matches!(
        store.install_row_policy(
            TableContext {
                id: 129,
                ..context(&schema)
            },
            0,
            DENY
        ),
        Err(Error::PolicyCapacity)
    ));
    assert_eq!(store.database.committed_wal().unwrap(), before);
    let existing = listed[0].clone();
    let changed = store
        .install_row_policy(
            TableContext {
                id: existing.table,
                ..context(&schema)
            },
            existing.revision,
            OWN,
        )
        .unwrap();
    assert!(changed.revision > existing.revision);
    assert_eq!(store.row_policy_receipts().unwrap().len(), MAX_ROW_POLICIES);
}
#[test]
fn generated_expected_revision_sequences_match_independent_model_across_restarts_and_wal_formats() {
    use proptest::prelude::*;
    let _io = TEST_IO.lock().unwrap();
    let strategy = proptest::collection::vec((any::<bool>(), 0u8..4), 1..20);
    for format in [1, 2] {
        let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig::with_cases(16));
        runner
            .run(&strategy, |events| {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("private");
                let mut store = create(&path, 4);
                if format == 2 {
                    store.compact().unwrap();
                }
                let schema = schema();
                let mut model: Option<(Vec<u8>, u64, u64)> = None;
                for (index, (owner, mode)) in events.into_iter().enumerate() {
                    let document = if owner { OWN } else { DENY };
                    let expected = match (&model, mode) {
                        (Some((_, revision, _)), 0) => *revision,
                        (Some((_, _, previous)), 1) => *previous,
                        (_, 2) => 0,
                        _ => u64::MAX,
                    };
                    let before = store.database.committed_wal().unwrap();
                    let next = store.database.last_transaction() + 1;
                    let predicted = match &model {
                        None => {
                            if expected == 0 {
                                1
                            } else {
                                0
                            }
                        }
                        Some((old, revision, previous)) => {
                            if old == document && (expected == *revision || expected == *previous) {
                                2
                            } else if expected == *revision {
                                1
                            } else {
                                0
                            }
                        }
                    };
                    let result = store.install_row_policy(context(&schema), expected, document);
                    if predicted == 0 {
                        prop_assert!(matches!(result, Err(Error::PolicyConflict)));
                        prop_assert_eq!(store.database.committed_wal().unwrap(), before);
                    } else if predicted == 2 {
                        let receipt = result.unwrap();
                        prop_assert_eq!(receipt.revision, model.as_ref().unwrap().1);
                        prop_assert_eq!(store.database.committed_wal().unwrap(), before);
                    } else {
                        let receipt = result.unwrap();
                        let previous = model.as_ref().map_or(0, |m| m.1);
                        prop_assert_eq!(receipt.revision, next);
                        prop_assert_eq!(receipt.previous, previous);
                        model = Some((document.to_vec(), next, previous));
                    }
                    if index % 3 == 0 {
                        drop(store);
                        store = AccountStore::open(&path, PROJECT, PasswordPool::new(1).unwrap())
                            .unwrap();
                    }
                    let listed = store.row_policy_receipts().unwrap();
                    if let Some((_, revision, previous)) = &model {
                        prop_assert_eq!(listed.len(), 1);
                        prop_assert_eq!(listed[0].revision, *revision);
                        prop_assert_eq!(listed[0].previous, *previous);
                    } else {
                        prop_assert!(listed.is_empty());
                    }
                }
                Ok(())
            })
            .unwrap();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn native_policy_catalog_migration_first_and_replacement_staged_and_ack_kills_are_atomic_on_both_wals()
 {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let schema = schema();
    for format in [1, 2] {
        for phase in ["migrate", "first", "replace"] {
            for commit in [false, true] {
                let path = dir.path().join(format!("{format}-{phase}-{commit}"));
                let mut store = create(&path, if phase == "migrate" { 3 } else { 4 });
                let pair = store
                    .sign_in("synthetic", b"synthetic-password", 100)
                    .unwrap();
                let user = store
                    .check_password("synthetic", b"synthetic-password")
                    .unwrap()
                    .unwrap();
                let own = vec![Value::Integer(1), Value::Bytes(user.id.to_vec())];
                let original = if phase == "replace" {
                    let mut padded = OWN.to_vec();
                    padded.resize(crate::row_policy::MAX_DOCUMENT_BYTES, b' ');
                    Some(
                        store
                            .install_row_policy(context(&schema), 0, &padded)
                            .unwrap(),
                    )
                } else {
                    None
                };
                if format == 2 {
                    store.compact().unwrap();
                }
                let before = store.database.committed_wal().unwrap();
                let before_transaction = store.database.last_transaction();
                drop(store);
                super::recovery_tests::kill_worker_at(
                    &path,
                    &format!(
                        "policy-catalog-{phase}-{}",
                        if commit { "commit" } else { "stage" }
                    ),
                );
                let mut store =
                    AccountStore::open(&path, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
                assert_eq!(
                    store.database.last_transaction(),
                    before_transaction + u64::from(commit)
                );
                assert!(store.verify_access(pair.access.expose(), 100).is_ok());
                if !commit {
                    assert_eq!(store.database.committed_wal().unwrap(), before);
                }
                if phase == "migrate" && !commit {
                    assert_eq!(inventory(&mut store).private_version, 3);
                    assert!(matches!(
                        store.row_policy_receipts(),
                        Err(Error::PolicySchema)
                    ));
                } else {
                    assert_eq!(inventory(&mut store).private_version, 4);
                    let receipts = store.row_policy_receipts().unwrap();
                    let expected = if phase == "migrate" || phase == "first" && !commit {
                        0
                    } else {
                        1
                    };
                    assert_eq!(receipts.len(), expected);
                    if expected == 1 {
                        let proof = store
                            .verify_row_policy_access(pair.access.expose(), 100, 7)
                            .unwrap();
                        assert_eq!(
                            proof
                                .authorize(context(&schema), Change::Select(&own))
                                .is_ok(),
                            phase != "replace" || !commit
                        );
                        if commit {
                            assert_eq!(receipts[0].revision, before_transaction + 1);
                            assert_eq!(
                                receipts[0].previous,
                                original.as_ref().map_or(0, |p| p.revision)
                            );
                        } else {
                            assert_eq!(receipts[0], original.clone().unwrap());
                        }
                    }
                }
            }
        }
    }
}
