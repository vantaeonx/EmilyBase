use super::{files::Worker, fixture};
use crate::{
    AccountRoot, Error, MAX_ACTIVE_PRIVATE_STORES, ProjectStore, durability,
    inspect_account_bundle_root, restore_account_bundle_bytes,
};
use emilybase_auth::{accounts::AccountStore, password::PasswordPool};
use emilybase_catalog::Value;
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::fs::{self, File};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier, Mutex};

const LOGIN: &str = "synthetic_user";
const PASSWORD: &[u8] = b"synthetic-password";
fn pool() -> PasswordPool {
    PasswordPool::new(1).unwrap()
}
struct Restored {
    root: PathBuf,
    credentials: Vec<(String, String)>,
}
fn restored(parent: &Path, count: usize, compact: bool) -> Restored {
    let mut f = fixture(parent, count);
    if compact {
        for store in &mut f.accounts {
            store.compact().unwrap();
        }
        for path in &f.data_paths {
            Database::open(path).unwrap().compact().unwrap();
        }
    }
    let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
    let root = parent.join("restored");
    restore_account_bundle_bytes(&image, &root, pool(), 50).unwrap();
    Restored {
        root,
        credentials: f.credentials,
    }
}
fn histories(f: &Restored) -> Vec<Vec<u8>> {
    let mut bytes = vec![fs::read(f.root.join("root.json")).unwrap()];
    for (id, _) in &f.credentials {
        bytes.push(fs::read(f.root.join("registry").join(id).join("project.json")).unwrap());
        bytes.push(fs::read(f.root.join("registry").join(id).join("data/redo.wal")).unwrap());
        bytes.push(fs::read(f.root.join("private").join(id).join("redo.wal")).unwrap());
    }
    bytes
}
fn denied<T>(result: crate::Result<T>) {
    assert!(
        matches!(result, Err(Error::Denied)),
        "expected project service denial"
    );
}

#[test]
fn opening_retains_exact_inspected_owners_without_reset_time_or_history_changes() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = restored(dir.path(), 3, compact);
        let before = histories(&f);
        let mut live = AccountRoot::open(&f.root, pool()).unwrap();
        assert_eq!(histories(&f), before);
        assert_eq!(live.projects().unwrap().len(), 3);
        assert!(matches!(
            AccountRoot::open(&f.root, pool()),
            Err(Error::Busy)
        ));
        assert!(File::open(&f.root).unwrap().try_lock().is_err());
        assert!(matches!(
            ProjectStore::open_existing(f.root.join("registry")),
            Err(Error::Busy)
        ));
        for (id, key) in &f.credentials {
            assert!(AccountStore::open(f.root.join("private").join(id), id, pool()).is_err());
            assert!(Database::open(f.root.join("registry").join(id).join("data")).is_ok());
            let session = live.sign_in(id, key, LOGIN, PASSWORD, 50).unwrap();
            let login = live
                .with_access(id, key, session.access.expose(), 50, |principal| {
                    assert_eq!(principal.project(), id);
                    principal.account().login.clone()
                })
                .unwrap();
            assert_eq!(login, LOGIN);
        }
        drop(live);
        assert!(File::open(&f.root).unwrap().try_lock().is_ok());
        assert!(inspect_account_bundle_root(&f.root, pool()).is_ok());
    }
}

#[test]
fn active_private_cap_is_admitted_before_registry_or_private_open_and_offline_cap_is_unchanged() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    assert_eq!(MAX_ACTIVE_PRIVATE_STORES, 4);
    for count in [0, MAX_ACTIVE_PRIVATE_STORES, MAX_ACTIVE_PRIVATE_STORES + 1] {
        let dir = tempfile::tempdir().unwrap();
        let f = restored(dir.path(), count, false);
        let before = histories(&f);
        if count > MAX_ACTIVE_PRIVATE_STORES {
            let registry = ProjectStore::open_existing(f.root.join("registry")).unwrap();
            assert!(matches!(
                AccountRoot::open(&f.root, pool()),
                Err(Error::Limit)
            ));
            drop(registry);
            assert!(inspect_account_bundle_root(&f.root, pool()).is_ok());
        } else {
            let live = AccountRoot::open(&f.root, pool()).unwrap();
            assert_eq!(live.projects().unwrap().len(), count);
        }
        assert_eq!(histories(&f), before);
    }
}

#[test]
fn normal_restart_keeps_sessions_and_time_floor_then_refresh_and_logout_are_durable() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = restored(dir.path(), 1, compact);
        let (id, key) = &f.credentials[0];
        let mut live = AccountRoot::open(&f.root, pool()).unwrap();
        let first = live.sign_in(id, key, LOGIN, PASSWORD, 100).unwrap();
        let before = histories(&f);
        drop(live);
        let mut live = AccountRoot::open(&f.root, pool()).unwrap();
        assert_eq!(histories(&f), before);
        live.with_access(id, key, first.access.expose(), 100, |_| ())
            .unwrap();
        assert!(matches!(
            live.with_access(id, key, first.access.expose(), 99, |_| ()),
            Err(Error::Accounts(emilybase_auth::accounts::Error::Clock))
        ));
        assert_eq!(histories(&f), before);
        let second = live
            .refresh_session(id, key, first.refresh.expose(), 100)
            .unwrap();
        assert!(
            live.with_access(id, key, first.access.expose(), 100, |_| ())
                .is_err()
        );
        assert!(
            live.refresh_session(id, key, first.refresh.expose(), 100)
                .is_err()
        );
        live.with_access(id, key, second.access.expose(), 100, |_| ())
            .unwrap();
        drop(live);
        let mut live = AccountRoot::open(&f.root, pool()).unwrap();
        live.with_access(id, key, second.access.expose(), 100, |_| ())
            .unwrap();
        live.logout_session(id, key, second.refresh.expose(), 100)
            .unwrap();
        drop(live);
        let mut live = AccountRoot::open(&f.root, pool()).unwrap();
        assert!(
            live.with_access(id, key, second.access.expose(), 100, |_| ())
                .is_err()
        );
        assert!(
            live.refresh_session(id, key, second.refresh.expose(), 100)
                .is_err()
        );
    }
}

#[test]
fn current_project_key_precedes_credentials_clock_and_user_tokens_never_authorize_sql() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let f = restored(dir.path(), 2, false);
    let (id, key) = &f.credentials[0];
    let (other, other_key) = &f.credentials[1];
    let mut live = AccountRoot::open(&f.root, pool()).unwrap();
    let session = live.sign_in(id, key, LOGIN, PASSWORD, 50).unwrap();
    let before = histories(&f);
    for (project, bad_key) in [
        (id.as_str(), other_key.as_str()),
        (other, key),
        (id, session.access.expose()),
        (id, session.refresh.expose()),
        ("../private", key),
        (id, "synthetic-secret-invalid-key"),
    ] {
        denied(live.sign_in(project, bad_key, LOGIN, PASSWORD, 500));
        denied(live.refresh_session(project, bad_key, session.refresh.expose(), 500));
        denied(live.logout_session(project, bad_key, session.access.expose(), 500));
        denied(live.with_access(project, bad_key, session.access.expose(), 500, |_| ()));
        denied(live.create_user(project, bad_key, "new_user", PASSWORD));
        denied(live.set_disabled(project, bad_key, LOGIN, true));
        denied(live.execute(project, bad_key, "DELETE FROM t", &[]));
    }
    assert_eq!(histories(&f), before);
    assert!(
        live.with_access(other, other_key, session.access.expose(), 50, |_| ())
            .is_err()
    );
    live.with_access(id, key, session.access.expose(), 50, |_| ())
        .unwrap();
    let rotated = live.rotate_project_key(id).unwrap();
    let after = histories(&f);
    denied(live.with_access(id, key, session.access.expose(), 500, |_| ()));
    assert_eq!(histories(&f), after);
    live.with_access(id, &rotated.api_key, session.access.expose(), 50, |_| ())
        .unwrap();
    let rows = live
        .execute(id, &rotated.api_key, "SELECT * FROM t", &[])
        .unwrap();
    assert_eq!(
        rows.results[0].rows,
        vec![vec![Value::Integer(1), Value::Integer(0)]]
    );
    let redacted = format!("{live:?}");
    for secret in [
        id.as_str(),
        key,
        other_key,
        LOGIN,
        session.access.expose(),
        session.refresh.expose(),
    ] {
        assert!(!redacted.contains(secret));
    }
}

#[test]
fn provisioning_password_epoch_and_disable_changes_revoke_existing_families_after_restart() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let f = restored(dir.path(), 1, false);
    let (id, key) = &f.credentials[0];
    let mut live = AccountRoot::open(&f.root, pool()).unwrap();
    let info = live
        .create_user(id, key, "new_user", b"synthetic-new-password")
        .unwrap();
    assert_eq!(info.credential_epoch, 1);
    assert!(live.create_user(id, key, "new_user", PASSWORD).is_err());
    let first = live
        .sign_in(id, key, "new_user", b"synthetic-new-password", 50)
        .unwrap();
    let changed = live
        .change_password(id, key, "new_user", b"synthetic-new-password", PASSWORD)
        .unwrap();
    assert_eq!(changed.credential_epoch, 2);
    assert!(
        live.with_access(id, key, first.access.expose(), 50, |_| ())
            .is_err()
    );
    assert!(
        live.refresh_session(id, key, first.refresh.expose(), 50)
            .is_err()
    );
    let second = live.sign_in(id, key, "new_user", PASSWORD, 50).unwrap();
    let disabled = live.set_disabled(id, key, "new_user", true).unwrap();
    assert!(disabled.disabled);
    assert_eq!(disabled.credential_epoch, 3);
    drop(live);
    let mut live = AccountRoot::open(&f.root, pool()).unwrap();
    assert!(
        live.with_access(id, key, second.access.expose(), 50, |_| ())
            .is_err()
    );
    assert!(live.sign_in(id, key, "new_user", PASSWORD, 50).is_err());
    assert_eq!(
        live.set_disabled(id, key, "new_user", false)
            .unwrap()
            .credential_epoch,
        4
    );
    live.sign_in(id, key, "new_user", PASSWORD, 50).unwrap();
}

#[test]
fn live_operations_refuse_root_private_registry_data_and_manifest_substitutions() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for component in [
        "root",
        "private",
        "private-store",
        "registry",
        "data",
        "manifest",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let f = restored(dir.path(), 1, false);
        let (id, key) = &f.credentials[0];
        let mut live = AccountRoot::open(&f.root, pool()).unwrap();
        let selected = match component {
            "root" => f.root.clone(),
            "private" => f.root.join("private"),
            "private-store" => f.root.join("private").join(id),
            "registry" => f.root.join("registry"),
            "data" => f.root.join("registry").join(id).join("data"),
            _ => f.root.join("root.json"),
        };
        let detached = dir.path().join("detached");
        let old = if component == "manifest" {
            Some(fs::read(&selected).unwrap())
        } else {
            None
        };
        fs::rename(&selected, &detached).unwrap();
        if let Some(old) = &old {
            fs::write(&selected, old).unwrap();
            fs::set_permissions(&selected, fs::Permissions::from_mode(0o600)).unwrap();
        } else {
            fs::DirBuilder::new().mode(0o700).create(&selected).unwrap();
        }
        assert!(live.projects().is_err());
        assert!(live.sign_in(id, key, LOGIN, PASSWORD, 500).is_err());
        assert!(live.create_user(id, key, "forbidden", PASSWORD).is_err());
        assert!(live.rotate_project_key(id).is_err());
        assert!(live.execute(id, key, "DELETE FROM t", &[]).is_err());
        drop(live);
        assert!(selected.exists());
        assert!(detached.exists());
        if let Some(old) = old {
            assert_eq!(fs::read(selected).unwrap(), old);
        } else {
            assert_eq!(fs::read_dir(selected).unwrap().count(), 0);
        }
    }
}

#[test]
fn failed_open_releases_all_owners_preserves_foreign_entries_and_never_bootstraps_paths() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    assert!(AccountRoot::open(&missing, pool()).is_err());
    assert!(!missing.exists());
    let f = restored(dir.path(), 1, false);
    let (id, _) = &f.credentials[0];
    let before = histories(&f);
    let registry = ProjectStore::open_existing(f.root.join("registry")).unwrap();
    assert!(matches!(
        AccountRoot::open(&f.root, pool()),
        Err(Error::Busy)
    ));
    drop(registry);
    let account = AccountStore::open(f.root.join("private").join(id), id, pool()).unwrap();
    assert!(AccountRoot::open(&f.root, pool()).is_err());
    drop(account);
    fs::write(f.root.join("foreign"), b"preserve").unwrap();
    assert!(AccountRoot::open(&f.root, pool()).is_err());
    assert_eq!(fs::read(f.root.join("foreign")).unwrap(), b"preserve");
    fs::remove_file(f.root.join("foreign")).unwrap();
    let manifest = f.root.join("root.json");
    let bytes = fs::read(&manifest).unwrap();
    fs::set_permissions(&manifest, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(AccountRoot::open(&f.root, pool()).is_err());
    fs::set_permissions(&manifest, fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.path().join("link");
    symlink(&f.root, &link).unwrap();
    assert!(AccountRoot::open(&link, pool()).is_err());
    assert_eq!(fs::read(manifest).unwrap(), bytes);
    assert_eq!(histories(&f), before);
    assert!(AccountRoot::open(&f.root, pool()).is_ok());
}

#[test]
fn same_root_serializes_real_thread_refresh_to_one_winner_and_keeps_new_pair_after_restart() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let f = restored(dir.path(), 1, false);
    let (id, key) = f.credentials[0].clone();
    let mut live = AccountRoot::open(&f.root, pool()).unwrap();
    let first = live.sign_in(&id, &key, LOGIN, PASSWORD, 50).unwrap();
    let root = Arc::new(Mutex::new(live));
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let (root, barrier, id, key, token) = (
            root.clone(),
            barrier.clone(),
            id.clone(),
            key.clone(),
            first.refresh.expose().to_owned(),
        );
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            root.lock().unwrap().refresh_session(&id, &key, &token, 50)
        }));
    }
    barrier.wait();
    let mut winners = workers
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    assert_eq!(winners.len(), 1);
    let second = winners.pop().unwrap();
    drop(root);
    let mut live = AccountRoot::open(&f.root, pool()).unwrap();
    assert!(
        live.refresh_session(&id, &key, first.refresh.expose(), 50)
            .is_err()
    );
    live.with_access(&id, &key, second.access.expose(), 50, |_| ())
        .unwrap();
}

#[test]
fn native_kills_release_live_owners_and_keep_acknowledged_session_families_on_both_wals() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for point in ["account_root_service_opened", "account_root_service_ack"] {
            let dir = tempfile::tempdir().unwrap();
            let f = restored(dir.path(), 1, compact);
            let (id, key) = &f.credentials[0];
            let before = histories(&f);
            let input = dir.path().join("synthetic-credentials.json");
            fs::write(&input, serde_json::to_vec(key).unwrap()).unwrap();
            fs::set_permissions(&input, fs::Permissions::from_mode(0o600)).unwrap();
            let worker = Worker::start(&f.root, &input, id, "root-live", point);
            worker.reach(point);
            assert!(matches!(
                AccountRoot::open(&f.root, pool()),
                Err(Error::Busy)
            ));
            worker.kill();
            let mut live = AccountRoot::open(&f.root, pool()).unwrap();
            if point.ends_with("opened") {
                assert_eq!(histories(&f), before);
            } else {
                let tokens: (String, String) =
                    serde_json::from_slice(&fs::read(input.with_extension("session")).unwrap())
                        .unwrap();
                live.with_access(id, key, &tokens.0, 50, |_| ()).unwrap();
                let second = live.refresh_session(id, key, &tokens.1, 50).unwrap();
                live.with_access(id, key, second.access.expose(), 50, |_| ())
                    .unwrap();
            }
            drop(live);
            let account = AccountStore::open(f.root.join("private").join(id), id, pool()).unwrap();
            assert_eq!(
                account.session_family_count().unwrap(),
                usize::from(point.ends_with("ack"))
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]
    #[test]
    fn independent_epoch_disabled_and_restart_sequences_reject_stale_families(
        actions in prop::collection::vec(0u8..5, 1..9), compact in any::<bool>()) {
        let _serial = durability::PROCESS_TESTS.blocking_lock();
        let dir = tempfile::tempdir().unwrap();
        let f = restored(dir.path(), 1, compact);
        let (id, key) = &f.credentials[0];
        let mut live = AccountRoot::open(&f.root, pool()).unwrap();
        let mut disabled = false;
        let mut generation = 0u64;
        let mut tokens = Vec::new();
        for action in actions {
            match action {
                0 if !disabled => { let pair = live.sign_in(id,key,LOGIN,PASSWORD,50).unwrap(); tokens.push((pair,generation,false)); }
                1 => { disabled = !disabled; generation += 1; live.set_disabled(id,key,LOGIN,disabled).unwrap(); }
                2 => { drop(live); live=AccountRoot::open(&f.root,pool()).unwrap(); }
                3 if !tokens.is_empty() && !disabled && tokens.last().unwrap().1==generation && !tokens.last().unwrap().2 => {
                    let (pair,_,revoked) = tokens.last_mut().unwrap();
                    live.logout_session(id,key,pair.refresh.expose(),50).unwrap();
                    *revoked=true;
                }
                _ => {}
            }
            for (pair,epoch,revoked) in &tokens {
                let expected = !disabled && *epoch==generation && !revoked;
                prop_assert_eq!(live.with_access(id,key,pair.access.expose(),50, |_| ()).is_ok(),expected);
            }
        }
    }
}

#[test]
fn service_mutations_survive_offline_recapture_while_clone_reset_leaves_original_sessions_active() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = restored(dir.path(), 1, compact);
        let (id, key) = &f.credentials[0];
        let mut live = AccountRoot::open(&f.root, pool()).unwrap();
        live.create_user(id, key, "later_user", b"synthetic-later-password")
            .unwrap();
        live.execute(id, key, "UPDATE t SET v=42 WHERE id=1", &[])
            .unwrap();
        let session = live
            .sign_in(id, key, "later_user", b"synthetic-later-password", 100)
            .unwrap();
        let rotated = live.rotate_project_key(id).unwrap();
        assert!(
            live.execute(id, &rotated.api_key, "SELECT * FROM auth_users", &[])
                .is_err()
        );
        drop(live);
        let before = histories(&f);
        let bytes = crate::capture_account_bundle_root(&f.root, pool()).unwrap();
        assert_eq!(histories(&f), before);
        let copy = dir.path().join("independent-root");
        restore_account_bundle_bytes(&bytes, &copy, pool(), 50).unwrap();
        assert_eq!(histories(&f), before);
        let mut original = AccountRoot::open(&f.root, pool()).unwrap();
        let mut clone = AccountRoot::open(&copy, pool()).unwrap();
        denied(clone.sign_in(id, key, "later_user", b"synthetic-later-password", 500));
        let expected = vec![vec![Value::Integer(1), Value::Integer(42)]];
        for root in [&mut original, &mut clone] {
            assert_eq!(
                root.execute(id, &rotated.api_key, "SELECT * FROM t", &[])
                    .unwrap()
                    .results[0]
                    .rows,
                expected
            );
        }
        original
            .with_access(id, &rotated.api_key, session.access.expose(), 100, |_| ())
            .unwrap();
        assert!(
            clone
                .with_access(id, &rotated.api_key, session.access.expose(), 100, |_| ())
                .is_err()
        );
        assert!(
            clone
                .refresh_session(id, &rotated.api_key, session.refresh.expose(), 100)
                .is_err()
        );
        let fresh = clone
            .sign_in(
                id,
                &rotated.api_key,
                "later_user",
                b"synthetic-later-password",
                100,
            )
            .unwrap();
        clone
            .with_access(id, &rotated.api_key, fresh.access.expose(), 100, |_| ())
            .unwrap();
        assert!(
            original
                .with_access(id, &rotated.api_key, fresh.access.expose(), 100, |_| ())
                .is_err()
        );
        original
            .execute(id, &rotated.api_key, "UPDATE t SET v=99 WHERE id=1", &[])
            .unwrap();
        assert_eq!(
            clone
                .execute(id, &rotated.api_key, "SELECT * FROM t", &[])
                .unwrap()
                .results[0]
                .rows,
            expected
        );
    }
}

#[test]
fn explicit_subset_keeps_unattached_project_data_available_but_never_creates_private_state() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut source = fixture(dir.path(), 2);
    source.accounts.truncate(1);
    let bytes = source
        .registry
        .capture_account_bundle(&mut source.accounts)
        .unwrap();
    let root = dir.path().join("subset-root");
    restore_account_bundle_bytes(&bytes, &root, pool(), 50).unwrap();
    let (id, key) = &source.credentials[1];
    let private = root.join("private").join(id);
    let manifest = fs::read(root.join("root.json")).unwrap();
    let data = root.join("registry").join(id).join("data/redo.wal");
    let before = fs::read(&data).unwrap();
    let mut live = AccountRoot::open(&root, pool()).unwrap();
    denied(live.create_user(id, key, "forbidden", PASSWORD));
    denied(live.sign_in(id, key, LOGIN, PASSWORD, 500));
    denied(live.refresh_session(id, key, "synthetic-refresh", 500));
    denied(live.logout_session(id, key, "synthetic-refresh", 500));
    denied(live.prune_session_families(id, key, 500, 128));
    denied(live.list_users(id, key, None, 1));
    denied(live.with_access(id, key, "synthetic-access", 500, |_| ()));
    assert!(!private.exists());
    assert_eq!(fs::read(&data).unwrap(), before);
    assert_eq!(fs::read(root.join("root.json")).unwrap(), manifest);
    assert_eq!(
        live.execute(id, key, "SELECT * FROM t", &[])
            .unwrap()
            .results[0]
            .rows,
        vec![vec![Value::Integer(1), Value::Integer(1)]]
    );
    live.execute(id, key, "INSERT INTO t VALUES(2,7)", &[])
        .unwrap();
    assert!(!private.exists());
}

#[test]
fn denied_authorized_credentials_observe_trusted_time_but_overflow_and_backward_time_change_nothing()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = restored(dir.path(), 1, compact);
        let (id, key) = &f.credentials[0];
        let mut live = AccountRoot::open(&f.root, pool()).unwrap();
        let pair = live.sign_in(id, key, LOGIN, PASSWORD, 100).unwrap();
        assert!(matches!(
            live.sign_in(id, key, LOGIN, b"synthetic-wrong-password", 500),
            Err(Error::Accounts(emilybase_auth::accounts::Error::Denied))
        ));
        let forward = histories(&f);
        assert!(matches!(
            live.sign_in(id, key, "synthetic_missing", PASSWORD, 500),
            Err(Error::Accounts(emilybase_auth::accounts::Error::Denied))
        ));
        assert_eq!(histories(&f), forward);
        for now in [499, u64::MAX] {
            assert!(matches!(
                live.with_access(id, key, pair.access.expose(), now, |_| ()),
                Err(Error::Accounts(emilybase_auth::accounts::Error::Clock))
            ));
            assert!(matches!(
                live.sign_in(id, key, LOGIN, PASSWORD, now),
                Err(Error::Accounts(emilybase_auth::accounts::Error::Clock))
            ));
            assert_eq!(histories(&f), forward);
        }
        drop(live);
        let mut live = AccountRoot::open(&f.root, pool()).unwrap();
        assert_eq!(histories(&f), forward);
        live.with_access(id, key, pair.access.expose(), 500, |_| ())
            .unwrap();
        assert!(
            live.logout_session(id, key, pair.access.expose(), 500)
                .is_err()
        );
        assert_eq!(histories(&f), forward);
        live.logout_session(id, key, pair.refresh.expose(), 500)
            .unwrap();
        assert!(
            live.with_access(id, key, pair.access.expose(), 500, |_| ())
                .is_err()
        );
        let mut called = false;
        assert!(
            live.with_access(id, key, pair.access.expose(), 500, |_| {
                called = true;
            })
            .is_err()
        );
        assert!(!called);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]
    #[test]
    fn bounded_cleanup_matches_inactive_count_model_across_reopen(
        revoked in prop::collection::vec(any::<bool>(), 0..7),
        limit in 1..=4_usize,
        compact in any::<bool>(),
    ) {
        let _serial = durability::PROCESS_TESTS.blocking_lock();
        let dir = tempfile::tempdir().unwrap();
        let f = restored(dir.path(), 1, compact);
        let (id,key) = &f.credentials[0];
        let mut live = AccountRoot::open(&f.root,pool()).unwrap();
        let mut pairs = Vec::new();
        for inactive in &revoked {
            let pair = live.sign_in(id,key,LOGIN,PASSWORD,50).unwrap();
            if *inactive {
                live.logout_session(id,key,pair.refresh.expose(),50).unwrap();
            }
            pairs.push(pair);
        }
        let original = histories(&f);
        let mut inactive = revoked.iter().filter(|flag| **flag).count();
        let active = revoked.len() - inactive;
        loop {
            let expected = inactive.min(limit);
            let removed = live.prune_session_families(id,key,50,limit).unwrap();
            prop_assert_eq!(removed,expected);
            inactive -= expected;
            for (pair,revoked) in pairs.iter().zip(&revoked) {
                prop_assert_eq!(live.with_access(id,key,pair.access.expose(),50, |_|()).is_ok(), !revoked);
            }
            drop(live);
            let report = inspect_account_bundle_root(&f.root,pool()).unwrap();
            prop_assert_eq!(report.private_accounts[0].inventory.session_families,active+inactive);
            prop_assert_eq!(report.private_accounts[0].inventory.clock_floor,Some(50));
            prop_assert_eq!(report.private_accounts[0].inventory.database.wal_version,if compact {2}else{1});
            let after = histories(&f);
            // Manifest, project metadata and public data history remain byte-exact.
            prop_assert_eq!(&after[..3], &original[..3]);
            live=AccountRoot::open(&f.root,pool()).unwrap();
            if removed < limit { break; }
        }
        let before = histories(&f);
        prop_assert_eq!(live.prune_session_families(id,key,50,128).unwrap(),0);
        prop_assert_eq!(histories(&f),before);
    }
}

#[test]
fn root_policy_install_derives_current_table_context_and_holds_its_owner_through_private_commit() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let deny=br#"{"version":1,"select":{"kind":"deny"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}}"#;
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = restored(dir.path(), 2, compact);
        let (id, key) = &f.credentials[0];
        let mut root = AccountRoot::open(&f.root, pool()).unwrap();
        let before = histories(&f);
        denied(root.enable_row_policy_catalog(id, &f.credentials[1].1));
        denied(root.row_policy_receipts(id, &f.credentials[1].1));
        denied(root.install_row_policy(id, &f.credentials[1].1, "t", 0, deny));
        assert_eq!(histories(&f), before);
        root.enable_row_policy_catalog(id, key).unwrap();
        let enabled = histories(&f);
        assert!(
            root.install_row_policy(id, key, "missing", 0, deny)
                .is_err()
        );
        let bad=br#"{"version":1,"select":{"kind":"owner","column":"unknown"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}}"#;
        assert!(root.install_row_policy(id, key, "t", 0, bad).is_err());
        assert_eq!(histories(&f), enabled);
        let path = f.root.join("registry").join(id).join("data");
        let data = Database::open(&path).unwrap();
        let original_id = data.view().unwrap().table_id("t").unwrap();
        drop(data);
        let owner_seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen = owner_seen.clone();
        let guard = durability::on_boundary("root_policy_context_acquired", move || {
            assert!(Database::open(&path).is_err());
            seen.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let first = root.install_row_policy(id, key, "t", 0, deny).unwrap();
        drop(guard);
        assert!(owner_seen.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(first.table, original_id);
        let installed = histories(&f);
        assert_eq!(
            root.install_row_policy(id, key, "t", 0, deny).unwrap(),
            first
        );
        assert_eq!(histories(&f), installed);
        root.execute(
            id,
            key,
            "DROP TABLE t;CREATE TABLE t(id INT PRIMARY KEY,v INT);INSERT INTO t VALUES(1,0)",
            &[],
        )
        .unwrap();
        let changed = histories(&f);
        assert!(
            root.install_row_policy(id, key, "t", first.revision, deny)
                .is_err()
        );
        assert_eq!(histories(&f), changed);
        let second = root.install_row_policy(id, key, "t", 0, deny).unwrap();
        assert!(second.table > first.table);
        assert_eq!(second.previous, 0);
        assert_eq!(
            root.row_policy_receipts(id, key).unwrap(),
            vec![first.clone(), second.clone()]
        );
        let rotated = root.rotate_project_key(id).unwrap();
        let after = histories(&f);
        denied(root.install_row_policy(id, key, "t", second.revision, deny));
        assert_eq!(histories(&f), after);
        assert_eq!(
            root.row_policy_receipts(id, &rotated.api_key)
                .unwrap()
                .len(),
            2
        );
        drop(root);
        let mut root = AccountRoot::open(&f.root, pool()).unwrap();
        assert_eq!(
            root.row_policy_receipts(id, &rotated.api_key).unwrap(),
            vec![first, second]
        );
    }
}

mod key_file;
mod public_admission;
mod user_rows;
