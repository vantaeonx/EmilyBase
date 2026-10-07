use super::*;
use emilybase_catalog::Row;
use proptest::prelude::*;

pub(super) const PROJECT: &str = "11111111111111111111111111111111";
const OTHER: &str = "22222222222222222222222222222222";

fn fixture_digest() -> PasswordDigest {
    let mut record = [0; 72];
    record[..8].copy_from_slice(b"EBPWD\0\0\0");
    record[8..12].copy_from_slice(&[1, 0, 2, 19]);
    record[12..16].copy_from_slice(&19456_u32.to_le_bytes());
    record[16..20].copy_from_slice(&2_u32.to_le_bytes());
    record[20..24].copy_from_slice(&1_u32.to_le_bytes());
    record[24..40].copy_from_slice(b"0123456789abcdef");
    let hash = "a4ae807f135c26201d1689dd0d26d93d04e28a91bf16dd1f3381535303db8656";
    for i in 0..32 {
        record[40 + i] = u8::from_str_radix(&hash[2 * i..2 * i + 2], 16).unwrap();
    }
    PasswordDigest::decode(&record).unwrap()
}
pub(super) fn fixture_record(login: &str, id: [u8; 16], epoch: u64) -> Record {
    Record {
        info: AccountInfo {
            login: login.into(),
            id,
            credential_epoch: epoch,
            disabled: false,
        },
        digest: fixture_digest(),
    }
}

#[test]
fn canonical_login_is_bounded_ascii_and_never_normalized_into_paths() {
    for login in ["a", "0", "a-b.c_d", &"a".repeat(64)] {
        validate_login(login).unwrap();
    }
    for login in [
        "",
        "A",
        "_a",
        ".",
        "../x",
        "a/b",
        "a\\b",
        "синтетический",
        "a\0b",
        "a b",
        " a",
        &"a".repeat(65),
    ] {
        assert!(matches!(validate_login(login), Err(Error::Login)));
    }
}

#[test]
fn provisioning_password_changes_disable_and_reopen_are_real_commits() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("accounts");
    let pool = PasswordPool::new(2).unwrap();
    let mut store = AccountStore::create(&path, PROJECT, pool.clone()).unwrap();
    assert_eq!(store.project(), PROJECT);
    assert_eq!(store.count().unwrap(), 0);
    let info = store
        .create_user("synthetic", b"synthetic-original")
        .unwrap();
    assert_eq!(info.credential_epoch, 1);
    assert_eq!(
        store
            .check_password("synthetic", b"synthetic-original")
            .unwrap(),
        Some(info.clone())
    );
    assert!(
        store
            .check_password("missing", b"synthetic-original")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .check_password("synthetic", b"wrong")
            .unwrap()
            .is_none()
    );
    let before = store.database.committed_wal().unwrap();
    assert!(matches!(
        store.create_user("synthetic", b"different"),
        Err(Error::Exists)
    ));
    assert!(matches!(
        store.change_password("synthetic", b"wrong", b"replacement"),
        Err(Error::Denied)
    ));
    assert_eq!(store.database.committed_wal().unwrap(), before);
    let replacement = store
        .change_password("synthetic", b"synthetic-original", b"replacement")
        .unwrap();
    assert_eq!(replacement.id, info.id);
    assert_eq!(replacement.credential_epoch, 2);
    assert!(
        store
            .check_password("synthetic", b"synthetic-original")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.check_password("synthetic", b"replacement").unwrap(),
        Some(replacement)
    );
    let disabled = store.set_disabled("synthetic", true).unwrap();
    assert_eq!(disabled.credential_epoch, 3);
    assert!(
        store
            .check_password("synthetic", b"replacement")
            .unwrap()
            .is_none()
    );
    let before = store.database.committed_wal().unwrap();
    assert_eq!(store.set_disabled("synthetic", true).unwrap(), disabled);
    assert_eq!(store.database.committed_wal().unwrap(), before);
    drop(store);
    assert!(matches!(
        AccountStore::open(&path, OTHER, pool.clone()),
        Err(Error::ScopeMismatch)
    ));
    let mut store = AccountStore::open(&path, PROJECT, pool).unwrap();
    assert!(
        store
            .check_password("synthetic", b"replacement")
            .unwrap()
            .is_none()
    );
    let enabled = store.set_disabled("synthetic", false).unwrap();
    assert_eq!(enabled.credential_epoch, 4);
    assert_eq!(
        store.check_password("synthetic", b"replacement").unwrap(),
        Some(enabled)
    );
}

#[test]
fn invalid_project_scope_refuses_before_directory_creation_or_open() {
    let dir = tempfile::tempdir().unwrap();
    let pool = PasswordPool::new(1).unwrap();
    for project in [
        "",
        "../data",
        &"g".repeat(32),
        &"a".repeat(31),
        &"a".repeat(33),
    ] {
        let target = dir.path().join("not-created");
        assert!(matches!(
            AccountStore::create(&target, project, pool.clone()),
            Err(Error::Scope)
        ));
        assert!(matches!(
            AccountStore::open(&target, project, pool.clone()),
            Err(Error::Scope)
        ));
        assert!(!target.exists());
    }
}

#[test]
fn typed_record_rejects_wrong_shape_cost_id_epoch_and_login() {
    let row = fixture_record("synthetic", [1; 16], 1).encode();
    assert_eq!(Record::decode(&row).unwrap().encode(), row);
    for column in 0..5 {
        let mut changed = row.clone();
        changed[column] = Value::Null;
        assert!(matches!(Record::decode(&changed), Err(Error::Corrupt)));
    }
    for bytes in [vec![], vec![1; 15], vec![1; 17]] {
        let mut changed = row.clone();
        changed[1] = Value::Bytes(bytes);
        assert!(matches!(Record::decode(&changed), Err(Error::Corrupt)));
    }
    for epoch in [i64::MIN, -1, 0] {
        let mut changed = row.clone();
        changed[3] = Value::Integer(epoch);
        assert!(matches!(Record::decode(&changed), Err(Error::Corrupt)));
    }
    let mut changed = row.clone();
    changed[0] = Value::Text("../private".into());
    assert!(matches!(Record::decode(&changed), Err(Error::Corrupt)));
    let mut changed = row.clone();
    changed[2] = Value::Bytes(vec![0; 72]);
    assert!(matches!(Record::decode(&changed), Err(Error::Corrupt)));
    assert!(matches!(Record::decode(&row[..4]), Err(Error::Corrupt)));
    let mut extra = row;
    extra.push(Value::Integer(1));
    assert!(matches!(Record::decode(&extra), Err(Error::Corrupt)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn generated_private_records_roundtrip_exactly(login in "[a-z0-9][a-z0-9._-]{0,63}",
        id in any::<[u8;16]>(), epoch in 1_i64..=i64::MAX, disabled in any::<bool>()) {
        let mut record=fixture_record(&login,id,epoch as u64);record.info.disabled=disabled;
        let encoded:Row=record.encode();let decoded=Record::decode(&encoded).unwrap();
        prop_assert_eq!(&decoded.info,&record.info);
        prop_assert_eq!(decoded.encode(),encoded);
    }
}

pub(super) fn raw_store(path: &Path, users: Vec<Row>) {
    let mut database = Database::create(path).unwrap();
    let mut transaction = database.begin().unwrap();
    transaction.create_table(scope_schema()).unwrap();
    transaction.create_table(user_schema()).unwrap();
    transaction
        .insert(
            SCOPE,
            vec![
                Value::Integer(1),
                Value::Integer(1),
                Value::Text(PROJECT.into()),
                Value::Bytes(fixture_digest().encode().to_vec()),
            ],
        )
        .unwrap();
    transaction.commit().unwrap();
    for chunk in users.chunks(128) {
        let mut transaction = database.begin().unwrap();
        for row in chunk {
            transaction.insert(USERS, row.clone()).unwrap();
        }
        transaction.commit().unwrap();
    }
}

#[test]
fn same_login_in_separate_project_stores_never_crosses_credentials_or_scope() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (left_path, right_path) = (dir.path().join("left"), dir.path().join("right"));
    let pool = PasswordPool::new(2).unwrap();
    let mut left = AccountStore::create(&left_path, PROJECT, pool.clone()).unwrap();
    let mut right = AccountStore::create(&right_path, OTHER, pool.clone()).unwrap();
    let a = left.create_user("same", b"synthetic-left").unwrap();
    let b = right.create_user("same", b"synthetic-right").unwrap();
    assert_ne!(a.id, b.id);
    assert!(
        left.check_password("same", b"synthetic-right")
            .unwrap()
            .is_none()
    );
    assert!(
        right
            .check_password("same", b"synthetic-left")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        left.check_password("same", b"synthetic-left").unwrap(),
        Some(a)
    );
    assert_eq!(
        right.check_password("same", b"synthetic-right").unwrap(),
        Some(b)
    );
    assert_eq!(pool.usage().operations, 0);
    drop(left);
    drop(right);
    assert!(matches!(
        AccountStore::open(&left_path, OTHER, pool.clone()),
        Err(Error::ScopeMismatch)
    ));
    assert!(matches!(
        AccountStore::open(&right_path, PROJECT, pool),
        Err(Error::ScopeMismatch)
    ));
}

#[test]
fn both_wal_versions_preserve_epochs_old_views_and_verified_private_backups() {
    let _io = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("accounts");
        let pool = PasswordPool::new(1).unwrap();
        let mut store = AccountStore::create(&path, PROJECT, pool.clone()).unwrap();
        let original = store.create_user("synthetic", b"first-password").unwrap();
        if compacted {
            store.compact().unwrap();
        }
        let old = store.database.view().unwrap().clone();
        let old_row = old
            .get(USERS, &Key::Text("synthetic".into()))
            .unwrap()
            .unwrap()
            .clone();
        let replacement = store
            .change_password("synthetic", b"first-password", b"second-password")
            .unwrap();
        let disabled = store.set_disabled("synthetic", true).unwrap();
        assert_eq!(
            (
                original.credential_epoch,
                replacement.credential_epoch,
                disabled.credential_epoch
            ),
            (1, 2, 3)
        );
        assert_eq!(
            old.get(USERS, &Key::Text("synthetic".into())).unwrap(),
            Some(&old_row)
        );
        let mut rollback = store.database.begin().unwrap();
        rollback
            .delete(USERS, &Key::Text("synthetic".into()))
            .unwrap();
        rollback.rollback();
        assert_eq!(store.count().unwrap(), 1);
        let wal = store.database.committed_wal().unwrap();
        for plaintext in [b"first-password".as_slice(), b"second-password"] {
            assert!(!wal.windows(plaintext.len()).any(|part| part == plaintext));
        }
        let archive = dir.path().join("private.backup");
        let report = store.backup(&archive).unwrap();
        assert_eq!(report.wal_version, if compacted { 2 } else { 1 });
        assert_eq!((report.tables, report.rows), (2, 2));
        assert_eq!(report, emilybase_backup::inspect(&archive).unwrap());
        let target = dir.path().join("restored");
        emilybase_backup::restore(&archive, &target).unwrap();
        assert!(matches!(
            AccountStore::open(&target, OTHER, pool.clone()),
            Err(Error::ScopeMismatch)
        ));
        let mut restored = AccountStore::open(&target, PROJECT, pool.clone()).unwrap();
        assert!(
            restored
                .check_password("synthetic", b"second-password")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            restored
                .set_disabled("synthetic", false)
                .unwrap()
                .credential_epoch,
            4
        );
        assert!(
            restored
                .check_password("synthetic", b"second-password")
                .unwrap()
                .is_some()
        );
        assert!(
            restored
                .check_password("synthetic", b"first-password")
                .unwrap()
                .is_none()
        );
        drop(store);
        let mut reopened = AccountStore::open(&path, PROJECT, pool).unwrap();
        assert_eq!(reopened.database.committed_wal().unwrap(), wal);
        assert_eq!(
            reopened.record("synthetic").unwrap().unwrap().info,
            disabled
        );
    }
}

#[test]
fn full_1024_user_store_refuses_growth_before_hashing_and_oversized_store_fails_open() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let pool = PasswordPool::new(1).unwrap();
    for count in [MAX_ACCOUNTS, MAX_ACCOUNTS + 1] {
        let path = dir.path().join(format!("size-{count}"));
        let users = (0..count)
            .map(|i| {
                let mut id = [0; 16];
                id[..8].copy_from_slice(&(i as u64).to_le_bytes());
                fixture_record(&format!("u{i:04}"), id, 1).encode()
            })
            .collect();
        raw_store(&path, users);
        if count > MAX_ACCOUNTS {
            assert!(matches!(
                AccountStore::open(&path, PROJECT, pool.clone()),
                Err(Error::Corrupt)
            ));
        } else {
            let mut store = AccountStore::open(&path, PROJECT, pool.clone()).unwrap();
            assert_eq!(store.count().unwrap(), count);
            let before = store.database.committed_wal().unwrap();
            // Empty input would fail at the KDF boundary; capacity wins first.
            assert!(matches!(
                store.create_user("overflow", &[]),
                Err(Error::Capacity)
            ));
            assert!(matches!(
                store.create_user("u0000", b"different"),
                Err(Error::Exists)
            ));
            assert_eq!(store.database.committed_wal().unwrap(), before);
        }
        assert_eq!(pool.usage().workspace_bytes, 0);
    }
}

#[test]
fn opening_rejects_semantically_invalid_users_even_with_valid_page_and_wal_checksums() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let pool = PasswordPool::new(1).unwrap();
    let original = fixture_record("synthetic", [7; 16], 1).encode();
    let mut cases = Vec::new();
    let mut row = original.clone();
    row[0] = Value::Text("../private".into());
    cases.push(vec![row]);
    let mut row = original.clone();
    row[1] = Value::Bytes(vec![0; 15]);
    cases.push(vec![row]);
    let mut row = original.clone();
    row[3] = Value::Integer(0);
    cases.push(vec![row]);
    let mut row = original.clone();
    row[3] = Value::Integer(i64::MIN);
    cases.push(vec![row]);
    let mut digest = fixture_digest().encode();
    digest[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut row = original.clone();
    row[2] = Value::Bytes(digest.to_vec());
    cases.push(vec![row]);
    let mut digest = fixture_digest().encode();
    digest[8] = 2;
    let mut row = original.clone();
    row[2] = Value::Bytes(digest.to_vec());
    cases.push(vec![row]);
    let mut row = original.clone();
    row[0] = Value::Text("other".into());
    cases.push(vec![original, row]);
    for (i, users) in cases.into_iter().enumerate() {
        let path = dir.path().join(format!("case-{i}"));
        raw_store(&path, users);
        let opened = AccountStore::open(&path, PROJECT, pool.clone());
        let diagnostic = match &opened {
            Err(Error::Storage(error)) => format!("storage {error:?}"),
            Err(error) => format!("{error:?}"),
            Ok(_) => "accepted".into(),
        };
        assert!(
            matches!(opened, Err(Error::Corrupt)),
            "user case {i}: {diagnostic}"
        );
        assert_eq!(pool.usage().workspace_bytes, 0);
    }
}

#[test]
fn opening_requires_exact_scope_row_and_schema_inventory() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let pool = PasswordPool::new(1).unwrap();
    for case in 0..8 {
        let path = dir.path().join(format!("scope-{case}"));
        raw_store(&path, vec![]);
        let mut database = Database::open(&path).unwrap();
        let mut transaction = database.begin().unwrap();
        match case {
            0 => transaction.delete(SCOPE, &Key::Integer(1)).unwrap(),
            1 => {
                let mut row = transaction
                    .view()
                    .unwrap()
                    .get(SCOPE, &Key::Integer(1))
                    .unwrap()
                    .unwrap()
                    .clone();
                row[0] = Value::Integer(2);
                transaction.insert(SCOPE, row).unwrap();
            }
            2..=5 => {
                let mut row = transaction
                    .view()
                    .unwrap()
                    .get(SCOPE, &Key::Integer(1))
                    .unwrap()
                    .unwrap()
                    .clone();
                match case {
                    2 => row[1] = Value::Integer(2),
                    3 => row[2] = Value::Text("../private".into()),
                    4 => row[3] = Value::Bytes(vec![0; 72]),
                    _ => {
                        row[0] = Value::Integer(2);
                        transaction.delete(SCOPE, &Key::Integer(1)).unwrap();
                    }
                }
                if case == 5 {
                    transaction.insert(SCOPE, row).unwrap();
                } else {
                    transaction.update(SCOPE, &Key::Integer(1), row).unwrap();
                }
            }
            6 => {
                let mut extra = user_schema();
                extra.name = "unexpected".into();
                transaction.create_table(extra).unwrap();
            }
            _ => {
                transaction.drop_table(USERS).unwrap();
                let mut wrong = user_schema();
                wrong.columns[4].nullable = true;
                transaction.create_table(wrong).unwrap();
            }
        }
        transaction.commit().unwrap();
        drop(database);
        let opened = AccountStore::open(&path, PROJECT, pool.clone());
        let diagnostic = match &opened {
            Err(Error::Storage(error)) => format!("storage {error:?}"),
            Err(error) => format!("{error:?}"),
            Ok(_) => "accepted".into(),
        };
        assert!(
            matches!(opened, Err(Error::Corrupt)),
            "scope case {case}: {diagnostic}"
        );
    }
}

#[test]
fn epoch_exhaustion_and_failed_password_changes_never_modify_the_journal() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("epochs");
    raw_store(
        &path,
        vec![fixture_record("synthetic", [9; 16], i64::MAX as u64).encode()],
    );
    let mut store = AccountStore::open(&path, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
    let before = store.database.committed_wal().unwrap();
    assert!(matches!(
        store.set_disabled("synthetic", true),
        Err(Error::Epoch)
    ));
    assert!(matches!(
        store.change_password("synthetic", b"synthetic-password", b"replacement"),
        Err(Error::Epoch)
    ));
    assert!(matches!(
        store.change_password("synthetic", b"synthetic-password", &[]),
        Err(Error::Password(PasswordError::Input))
    ));
    assert!(matches!(
        store.change_password("missing", b"synthetic-password", b"replacement"),
        Err(Error::Denied)
    ));
    assert_eq!(
        store
            .set_disabled("synthetic", false)
            .unwrap()
            .credential_epoch,
        i64::MAX as u64
    );
    assert_eq!(store.database.committed_wal().unwrap(), before);
}

#[test]
fn account_debug_and_error_reports_redact_identity_and_password_data() {
    let info = fixture_record("synthetic-secret-login", [0xaa; 16], 1).info;
    assert!(!format!("{info:?}").contains("synthetic-secret-login"));
    for error in [
        Error::Login,
        Error::ScopeMismatch,
        Error::Denied,
        Error::Corrupt,
        Error::Password(PasswordError::Input),
    ] {
        assert!(!format!("{error:?}").contains("synthetic-secret-login"));
        assert!(!error.to_string().contains("synthetic-password"));
    }
}

#[test]
fn directory_ownership_private_permissions_and_no_clobber_are_preserved() {
    let _io = TEST_IO.lock().unwrap();
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private");
    let pool = PasswordPool::new(1).unwrap();
    let mut store = AccountStore::create(&path, PROJECT, pool.clone()).unwrap();
    store
        .create_user("synthetic", b"synthetic-password")
        .unwrap();
    let before = store.database.committed_wal().unwrap();
    assert!(matches!(
        AccountStore::open(&path, PROJECT, pool.clone()),
        Err(Error::Storage(_))
    ));
    assert!(AccountStore::create(&path, OTHER, pool.clone()).is_err());
    assert_eq!(store.database.committed_wal().unwrap(), before);
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(path.join("redo.wal"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let archive = dir.path().join("private.backup");
    store.backup(&archive).unwrap();
    assert_eq!(
        std::fs::metadata(archive).unwrap().permissions().mode() & 0o777,
        0o600
    );
    drop(store);
    let linked = dir.path().join("link");
    symlink(&path, &linked).unwrap();
    assert!(AccountStore::open(linked, PROJECT, pool).is_err());
}

#[test]
fn matching_the_dummy_digest_still_cannot_authenticate_a_missing_identity() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dummy");
    raw_store(&path, vec![]);
    let store = AccountStore::open(path, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
    // This synthetic raw fixture deliberately uses a publicly known dummy input.
    assert!(
        store
            .check_password("missing", b"synthetic-password")
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        store.check_password("missing", &[]),
        Err(Error::Password(PasswordError::Input))
    ));
    assert!(matches!(
        store.check_password("../private", b"synthetic-password"),
        Err(Error::Login)
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_disable_history_matches_independent_epoch_model_and_reopen(
        commands in prop::collection::vec(any::<u8>(),0..32), compact in any::<bool>()
    ) {
        let _io = TEST_IO.lock().unwrap();
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("model");
        raw_store(&path,(0..3).map(|i|fixture_record(&format!("u{i}"),[i;16],1).encode()).collect());
        let pool=PasswordPool::new(1).unwrap();let mut store=AccountStore::open(&path,PROJECT,pool.clone()).unwrap();
        let mut model=[(1_u64,false);3];
        for command in commands {
            let slot=usize::from(command%5);let disabled=command&8!=0;
            let before=store.database.committed_wal().unwrap();
            let actual=store.set_disabled(&format!("u{slot}"),disabled);
            if slot>=3 {
                prop_assert!(matches!(actual,Err(Error::Denied)));
                prop_assert_eq!(store.database.committed_wal().unwrap(),before);
            } else {
                let changed=model[slot].1!=disabled;
                if changed {model[slot].0+=1;model[slot].1=disabled;}
                let info=actual.unwrap();prop_assert_eq!(info.id,[slot as u8;16]);
                prop_assert_eq!((info.credential_epoch,info.disabled),model[slot]);
                if !changed {prop_assert_eq!(store.database.committed_wal().unwrap(),before);}
            }
            prop_assert_eq!(store.count().unwrap(),3);
        }
        if compact {store.compact().unwrap();}
        drop(store);let mut store=AccountStore::open(&path,PROJECT,pool.clone()).unwrap();
        let archive=dir.path().join("private.backup");store.backup(&archive).unwrap();
        let target=dir.path().join("restored");emilybase_backup::restore(archive,&target).unwrap();
        let restored=AccountStore::open(target,PROJECT,pool).unwrap();
        for (slot,expected) in model.into_iter().enumerate() {
            let login=format!("u{slot}");let local=store.record(&login).unwrap().unwrap();
            let recovered=restored.record(&login).unwrap().unwrap();
            prop_assert_eq!((local.info.credential_epoch,local.info.disabled),expected);
            prop_assert_eq!(local.encode(),recovered.encode());
        }
    }
}
