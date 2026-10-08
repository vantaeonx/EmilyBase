use super::{files::Worker, fixture, histories, raw};
use crate::registry_files::account_root::AccountBundleRootManifest;
use crate::{
    Error, ProjectStore, durability, inspect_account_bundle_root,
    inspect_account_bundle_root_manifest_bytes, restore_account_bundle,
    restore_account_bundle_bytes,
};
use emilybase_auth::{accounts::AccountStore, password::PasswordPool};
use emilybase_catalog::Value;
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::fs::{self, File};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const PREFIX: &str = ".emilybase-account-restore-";
fn stages(parent: &Path) -> Vec<PathBuf> {
    fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(PREFIX))
        .collect()
}
fn stage(parent: &Path) -> PathBuf {
    let entries = stages(parent);
    assert_eq!(entries.len(), 1);
    entries[0].clone()
}
fn write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
fn pool() -> PasswordPool {
    PasswordPool::new(1).unwrap()
}
fn manifest(value: &AccountBundleRootManifest) -> Vec<u8> {
    let payload = serde_json::to_vec(value).unwrap();
    format!(
        "{{\"payload\":{},\"checksum\":{}}}",
        std::str::from_utf8(&payload).unwrap(),
        crc32fast::hash(&payload)
    )
    .into_bytes()
}

#[test]
fn file_and_byte_root_restore_reset_every_private_version_before_selection() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for version in 1..=3 {
        for compact in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut f = fixture(dir.path(), 1);
            let id = f.credentials[0].0.clone();
            if version >= 2 {
                f.accounts[0].enable_session_storage().unwrap();
            }
            let old = if version == 3 {
                f.accounts[0].enable_session_clock(100).unwrap();
                Some(
                    f.accounts[0]
                        .sign_in("synthetic_user", b"synthetic-password", 100)
                        .unwrap(),
                )
            } else {
                None
            };
            let prior_scope = f.accounts[0].session_storage_scope().unwrap();
            if compact {
                f.accounts[0].compact().unwrap();
                Database::open(&f.data_paths[0]).unwrap().compact().unwrap();
            }
            let before = histories(&f);
            let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
            let input = dir.path().join("synthetic.account-bundle");
            write(&input, &image);
            for bytes in [false, true] {
                let target = dir.path().join(format!("restored-{bytes}"));
                let parent = dir.path().to_owned();
                let target_seen = target.clone();
                let id_seen = id.clone();
                let scope = prior_scope.clone();
                let called = Arc::new(AtomicBool::new(false));
                let observed = called.clone();
                let _guard = durability::on_boundary("bundle_restore_manifest_synced", move || {
                    assert!(!target_seen.exists());
                    let root = stage(&parent);
                    let store =
                        AccountStore::open(root.join("private").join(&id_seen), &id_seen, pool())
                            .unwrap();
                    assert_eq!(store.session_clock_floor().unwrap(), Some(50));
                    assert_ne!(store.session_storage_scope().unwrap(), scope);
                    assert!(
                        store
                            .check_password("synthetic_user", b"synthetic-password")
                            .unwrap()
                            .is_some()
                    );
                    observed.store(true, Ordering::SeqCst);
                });
                let result = if bytes {
                    restore_account_bundle_bytes(&image, &target, pool(), 50)
                } else {
                    restore_account_bundle(&input, &target, pool(), 50)
                }
                .unwrap();
                assert!(called.load(Ordering::SeqCst));
                assert_eq!(
                    result,
                    inspect_account_bundle_root(&target, pool()).unwrap()
                );
                assert_eq!(result.reset_at, 50);
                assert_eq!(result.private_accounts[0].inventory.private_version, 3);
                let original = crate::inspect_account_bundle_bytes(&image).unwrap();
                assert_eq!(result.registry, original.registry);
                assert_eq!(
                    result.private_accounts[0].inventory.database.wal_version,
                    if compact { 2 } else { 1 }
                );
                let registry = ProjectStore::open_existing(target.join("registry")).unwrap();
                assert_eq!(
                    registry
                        .authorize(&id, &f.credentials[0].1)
                        .unwrap()
                        .status()
                        .unwrap()
                        .rows,
                    1
                );
                let mut store =
                    AccountStore::open(target.join("private").join(&id), &id, pool()).unwrap();
                if let Some(old) = &old {
                    assert!(store.verify_access(old.access.expose(), 50).is_err());
                    assert!(store.refresh_session(old.refresh.expose(), 50).is_err());
                }
                let new = store
                    .sign_in("synthetic_user", b"synthetic-password", 50)
                    .unwrap();
                assert!(store.verify_access(new.access.expose(), 50).is_ok());
                assert_eq!(
                    fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                    0o700
                );
                assert_eq!(
                    fs::metadata(target.join("root.json"))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                );
                assert!(stages(dir.path()).is_empty());
            }
            assert_eq!(histories(&f), before);
            assert_eq!(fs::read(input).unwrap(), image);
        }
    }
}

#[test]
fn multi_project_empty_and_subset_private_rosters_are_restored_exactly() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 3);
    for count in [0, 1, 3] {
        let image = f
            .registry
            .capture_account_bundle(&mut f.accounts[..count])
            .unwrap();
        let target = dir.path().join(format!("selected-{count}"));
        let report = restore_account_bundle_bytes(&image, &target, pool(), 0).unwrap();
        assert_eq!(report.private_accounts.len(), count);
        assert_eq!(report.registry.projects.len(), 3);
        assert_eq!(fs::read_dir(target.join("private")).unwrap().count(), count);
        assert_eq!(
            inspect_account_bundle_root(&target, pool()).unwrap(),
            report
        );
        for (index, (id, key)) in f.credentials.iter().enumerate() {
            let registry = ProjectStore::open_existing(target.join("registry")).unwrap();
            assert_eq!(
                registry.authorize(id, key).unwrap().status().unwrap().rows,
                1
            );
            assert_eq!(target.join("private").join(id).exists(), index < count);
        }
    }
    let mut empty = ProjectStore::open(dir.path().join("empty")).unwrap();
    let image = empty.capture_account_bundle(&mut []).unwrap();
    let target = dir.path().join("empty-root");
    assert!(
        restore_account_bundle_bytes(&image, &target, pool(), i64::MAX as u64)
            .unwrap()
            .registry
            .projects
            .is_empty()
    );
}

#[test]
fn malformed_complete_sources_and_invalid_time_fail_before_staging() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 3);
    let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
    let (registry, mut entries) = super::parts(&image);
    entries.last_mut().unwrap().1[0] ^= 1;
    let broken = raw(registry, &entries);
    let target = dir.path().join("selected");
    for bytes in [&[][..], &[0; 128][..], &broken[..]] {
        assert!(restore_account_bundle_bytes(bytes, &target, pool(), 50).is_err());
        assert!(!target.exists());
        assert!(stages(dir.path()).is_empty());
    }
    assert!(restore_account_bundle_bytes(&image, &target, pool(), u64::MAX).is_err());
    assert!(!target.exists());
    assert!(stages(dir.path()).is_empty());
}

#[test]
fn restore_never_replaces_foreign_files_directories_symlinks_or_source_stores() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 1);
    let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
    let before = histories(&f);
    let foreign = dir.path().join("foreign");
    write(&foreign, b"preserve");
    let directory = dir.path().join("directory");
    fs::create_dir(&directory).unwrap();
    let link = dir.path().join("link");
    symlink(&foreign, &link).unwrap();
    for target in [
        &foreign,
        &directory,
        &link,
        &f.private_paths[0],
        &dir.path().join("registry"),
    ] {
        assert!(restore_account_bundle_bytes(&image, target, pool(), 50).is_err());
    }
    assert_eq!(fs::read(foreign).unwrap(), b"preserve");
    assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
    assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
    assert_eq!(histories(&f), before);
    assert!(stages(dir.path()).is_empty());
}

#[test]
fn canonical_manifest_limits_and_complete_corruption_are_rejected() {
    let valid = AccountBundleRootManifest {
        version: 1,
        private_projects: (0..128).map(|n| format!("{n:032x}")).collect(),
        reset_at: i64::MAX as u64,
    };
    let bytes = manifest(&valid);
    assert!(bytes.len() < 8192);
    assert_eq!(
        inspect_account_bundle_root_manifest_bytes(&bytes).unwrap(),
        valid
    );
    for index in 0..bytes.len() {
        let mut damaged = bytes.clone();
        damaged[index] ^= 1;
        assert!(inspect_account_bundle_root_manifest_bytes(&damaged).is_err());
    }
    for invalid in [
        AccountBundleRootManifest {
            version: 2,
            ..valid.clone()
        },
        AccountBundleRootManifest {
            reset_at: u64::MAX,
            ..valid.clone()
        },
        AccountBundleRootManifest {
            private_projects: vec!["../escape".into()],
            ..valid.clone()
        },
        AccountBundleRootManifest {
            private_projects: vec!["0".repeat(32); 2],
            ..valid.clone()
        },
        AccountBundleRootManifest {
            private_projects: (0..129).map(|n| format!("{n:032x}")).collect(),
            ..valid.clone()
        },
        AccountBundleRootManifest {
            private_projects: valid.private_projects.iter().rev().cloned().collect(),
            ..valid.clone()
        },
    ] {
        assert!(inspect_account_bundle_root_manifest_bytes(&manifest(&invalid)).is_err());
    }
    let mut spaced = bytes.clone();
    spaced.push(b' ');
    assert!(inspect_account_bundle_root_manifest_bytes(&spaced).is_err());
    assert!(inspect_account_bundle_root_manifest_bytes(&vec![b' '; 8193]).is_err());
    assert!(inspect_account_bundle_root_manifest_bytes(br#"{"payload":{"version":1,"version":1,"private_projects":[],"reset_at":0},"checksum":0}"#).is_err());
}

#[test]
fn restore_sync_faults_leave_only_complete_selected_roots_and_allow_independent_retry() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 1);
    for compact in [false, true] {
        if compact {
            f.accounts[0].compact().unwrap();
            Database::open(&f.data_paths[0]).unwrap().compact().unwrap();
        }
        let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
        let before = histories(&f);
        for (index, point) in [
            "bundle_restore_private_sync",
            "bundle_restore_private_sync_after",
            "bundle_restore_manifest_sync",
            "bundle_restore_manifest_sync_after",
            "bundle_restore_stage_sync",
            "bundle_restore_stage_sync_after",
            "bundle_restore_parent_sync",
            "bundle_restore_parent_sync_after",
        ]
        .into_iter()
        .enumerate()
        {
            let target = dir.path().join(format!("fault-{compact}-{index}"));
            durability::inject(point);
            let result = restore_account_bundle_bytes(&image, &target, pool(), 50);
            if point.contains("parent") {
                assert!(matches!(result, Err(Error::PublicationUnknown(_))));
                assert!(inspect_account_bundle_root(&target, pool()).is_ok());
            } else {
                assert!(result.is_err());
                assert!(!target.exists());
            }
            assert!(stages(dir.path()).is_empty());
            assert_eq!(histories(&f), before);
            restore_account_bundle_bytes(
                &image,
                dir.path().join(format!("retry-{compact}-{index}")),
                pool(),
                50,
            )
            .unwrap();
        }
    }
}

#[test]
fn prepared_manifest_and_private_history_substitution_cannot_pass_final_validation() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for change in ["manifest", "private-history", "private-directory"] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = fixture(dir.path(), 1);
        f.accounts[0].enable_session_clock(100).unwrap();
        let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
        let before = histories(&f);
        let original = before[1].clone();
        let id = f.credentials[0].0.clone();
        let parent = dir.path().to_owned();
        let boundary = if change == "private-directory" {
            "bundle_restore_inventory_validated"
        } else {
            "bundle_restore_manifest_synced"
        };
        let _guard = durability::on_boundary(boundary, move || {
            let root = stage(&parent);
            let private = root.join("private").join(&id);
            match change {
                "manifest" => {
                    let path = root.join("root.json");
                    let mut m =
                        inspect_account_bundle_root_manifest_bytes(&fs::read(&path).unwrap())
                            .unwrap();
                    m.reset_at = 49;
                    write(&path, &manifest(&m));
                }
                "private-history" => write(&private.join("redo.wal"), &original),
                _ => {
                    fs::rename(&private, parent.join("detached-private")).unwrap();
                    fs::create_dir(&private).unwrap();
                    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
                    write(&private.join("foreign"), b"preserve");
                }
            }
        });
        let target = dir.path().join("selected");
        assert!(restore_account_bundle_bytes(&image, &target, pool(), 50).is_err());
        assert!(!target.exists());
        assert_eq!(histories(&f), before);
        if change == "private-directory" {
            assert!(dir.path().join("detached-private/redo.wal").exists());
            let retained = stage(dir.path());
            assert_eq!(
                fs::read(
                    retained
                        .join("private")
                        .join(&f.credentials[0].0)
                        .join("foreign")
                )
                .unwrap(),
                b"preserve"
            );
        }
    }
}

#[test]
fn root_inspection_refuses_links_permissions_extra_missing_roster_and_busy_owners() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 1);
    let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
    let target = dir.path().join("selected");
    restore_account_bundle_bytes(&image, &target, pool(), 50).unwrap();
    let alias = dir.path().join("alias");
    symlink(&target, &alias).unwrap();
    assert!(inspect_account_bundle_root(alias, pool()).is_err());
    let root = File::open(&target).unwrap();
    root.lock().unwrap();
    assert!(matches!(
        inspect_account_bundle_root(&target, pool()),
        Err(Error::Busy)
    ));
    drop(root);
    let id = &f.credentials[0].0;
    let account = AccountStore::open(target.join("private").join(id), id, pool()).unwrap();
    assert!(inspect_account_bundle_root(&target, pool()).is_err());
    drop(account);
    let path = target.join("root.json");
    let original = fs::read(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(inspect_account_bundle_root(&target, pool()).is_err());
    write(&path, &original);
    let extra = target.join("extra");
    write(&extra, b"foreign");
    assert!(inspect_account_bundle_root(&target, pool()).is_err());
    fs::remove_file(extra).unwrap();
    let extra = target.join("private").join("f".repeat(32));
    fs::create_dir(&extra).unwrap();
    assert!(inspect_account_bundle_root(&target, pool()).is_err());
    fs::remove_dir(extra).unwrap();
    let private = target.join("private").join(id);
    let detached = dir.path().join("missing-private");
    fs::rename(&private, &detached).unwrap();
    assert!(inspect_account_bundle_root(&target, pool()).is_err());
    fs::rename(detached, private).unwrap();
    assert!(inspect_account_bundle_root(&target, pool()).is_ok());
}

#[test]
fn process_kills_select_no_partial_root_and_reset_old_sessions_before_ack() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = fixture(dir.path(), 1);
        f.accounts[0].enable_session_clock(100).unwrap();
        let old = f.accounts[0]
            .sign_in("synthetic_user", b"synthetic-password", 100)
            .unwrap();
        if compact {
            f.accounts[0].compact().unwrap();
            Database::open(&f.data_paths[0]).unwrap().compact().unwrap();
        }
        let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
        let before = histories(&f);
        let input = dir.path().join("input.account-bundle");
        write(&input, &image);
        let id = &f.credentials[0].0;
        for point in [
            "bundle_restore_registry_prepared",
            "bundle_restore_private_prepared",
            "bundle_restore_manifest_synced",
            "bundle_restore_owners_locked",
            "bundle_restore_inventory_validated",
            "bundle_restore_stage_synced",
            "bundle_restore_renamed",
            "bundle_restore_parent_synced",
            "bundle_restore_ack",
        ] {
            let target = dir.path().join(point);
            let worker = Worker::start(&input, &target, "", "root", point);
            worker.reach(point);
            worker.kill();
            let selected = matches!(
                point,
                "bundle_restore_renamed" | "bundle_restore_parent_synced" | "bundle_restore_ack"
            );
            assert_eq!(target.exists(), selected);
            if selected {
                let report = inspect_account_bundle_root(&target, pool()).unwrap();
                assert_eq!(report.private_accounts[0].inventory.clock_floor, Some(50));
                let mut account =
                    AccountStore::open(target.join("private").join(id), id, pool()).unwrap();
                assert!(account.verify_access(old.access.expose(), 50).is_err());
                assert!(account.refresh_session(old.refresh.expose(), 50).is_err());
                assert!(
                    account
                        .check_password("synthetic_user", b"synthetic-password")
                        .unwrap()
                        .is_some()
                );
            }
            assert_eq!(histories(&f), before);
            assert_eq!(fs::read(&input).unwrap(), image);
            restore_account_bundle_bytes(
                &image,
                dir.path().join(format!("retry-{point}")),
                pool(),
                50,
            )
            .unwrap();
        }
    }
}

#[test]
fn synchronized_native_root_restorers_publish_one_complete_winner() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 3);
    let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
    let before = histories(&f);
    let input = dir.path().join("input.account-bundle");
    write(&input, &image);
    let target = dir.path().join("selected");
    let point = "bundle_restore_stage_synced";
    let mut a = Worker::start(&input, &target, "", "root-race", point);
    let mut b = Worker::start(&input, &target, "", "root-race", point);
    a.reach(point);
    b.reach(point);
    a.release();
    b.release();
    let results = [a.finish(), b.finish()];
    assert_eq!(
        results.iter().filter(|s| s.contains("BUNDLE_OK")).count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|s| s.contains("BUNDLE_CONFLICT"))
            .count(),
        1
    );
    assert_eq!(
        inspect_account_bundle_root(&target, pool())
            .unwrap()
            .private_accounts
            .len(),
        3
    );
    assert_eq!(histories(&f), before);
    assert!(stages(dir.path()).is_empty());
}

#[test]
fn root_inspector_holds_all_private_data_and_root_owners_through_final_inventory_check() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 3);
    let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
    let target = dir.path().join("selected");
    restore_account_bundle_bytes(&image, &target, pool(), 50).unwrap();
    let root = target.clone();
    let ids = f
        .credentials
        .iter()
        .map(|p| p.0.clone())
        .collect::<Vec<_>>();
    let called = Arc::new(AtomicBool::new(false));
    let observed = called.clone();
    let _guard = durability::on_boundary("bundle_restore_inventory_validated", move || {
        let owner = File::open(&root).unwrap();
        assert!(matches!(
            owner.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        assert!(ProjectStore::open_existing(root.join("registry")).is_err());
        for id in ids {
            assert!(Database::open(root.join("registry").join(&id).join("data")).is_err());
            assert!(AccountStore::open(root.join("private").join(&id), &id, pool()).is_err());
        }
        observed.store(true, Ordering::SeqCst);
    });
    inspect_account_bundle_root(&target, pool()).unwrap();
    assert!(called.load(Ordering::SeqCst));
    for (id, _) in &f.credentials {
        assert!(Database::open(target.join("registry").join(id).join("data")).is_ok());
        assert!(AccountStore::open(target.join("private").join(id), id, pool()).is_ok());
    }
}

#[test]
fn root_parent_staging_and_selected_substitutions_preserve_foreign_entries() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for kind in ["parent", "stage", "selected"] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = fixture(dir.path(), 1);
        let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
        let parent = dir.path().join("parent");
        fs::create_dir(&parent).unwrap();
        let detached = dir.path().join("detached");
        let target = parent.join("selected");
        let changed_parent = parent.clone();
        let changed_detached = detached.clone();
        let changed_target = target.clone();
        let point = if kind == "selected" {
            "bundle_restore_renamed"
        } else {
            "bundle_restore_manifest_synced"
        };
        let _guard = durability::on_boundary(point, move || {
            let selected = match kind {
                "parent" => changed_parent.clone(),
                "stage" => stage(&changed_parent),
                _ => changed_target,
            };
            fs::rename(&selected, changed_detached).unwrap();
            fs::create_dir(&selected).unwrap();
            write(&selected.join("foreign"), b"preserve");
        });
        let result = restore_account_bundle_bytes(&image, &target, pool(), 50);
        if kind == "selected" {
            assert!(matches!(result, Err(Error::PublicationUnknown(_))));
            assert_eq!(fs::read(target.join("foreign")).unwrap(), b"preserve");
            assert!(inspect_account_bundle_root(&detached, pool()).is_ok());
        } else if kind == "stage" {
            assert!(matches!(result, Err(Error::Path)));
            assert!(!target.exists());
            assert_eq!(
                fs::read(stage(&parent).join("foreign")).unwrap(),
                b"preserve"
            );
            assert!(inspect_account_bundle_root(&detached, pool()).is_ok());
        } else {
            assert!(matches!(result, Err(Error::Path)));
            assert_eq!(fs::read(parent.join("foreign")).unwrap(), b"preserve");
            assert!(!target.exists());
        }
    }
}

#[test]
fn late_manifest_replacement_or_unsafe_permissions_are_retained_without_publication() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for permissions in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = fixture(dir.path(), 1);
        let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
        let parent = dir.path().to_owned();
        let _guard = durability::on_boundary("bundle_restore_stage_synced", move || {
            let path = stage(&parent).join("root.json");
            if permissions {
                fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
            } else {
                let bytes = fs::read(&path).unwrap();
                fs::rename(&path, parent.join("detached-manifest")).unwrap();
                write(&path, &bytes);
            }
        });
        let target = dir.path().join("selected");
        assert!(restore_account_bundle_bytes(&image, &target, pool(), 50).is_err());
        assert!(!target.exists());
        assert!(stage(dir.path()).join("root.json").exists());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn independently_modeled_rows_private_disable_and_retry_survive_common_restore(
        values in prop::collection::btree_set(2_i64..32,0..12), disabled in any::<bool>(),compact in any::<bool>(), now in 0_u64..1000
    ) {
        let _serial=durability::PROCESS_TESTS.blocking_lock();let dir=tempfile::tempdir().unwrap();let mut f=fixture(dir.path(),1);
        for value in &values {f.registry.authorize(&f.credentials[0].0,&f.credentials[0].1).unwrap().execute("INSERT INTO t VALUES($1,$1)",&[Value::Integer(*value)]).unwrap();}
        f.accounts[0].set_disabled("synthetic_user",disabled).unwrap();
        if compact {f.accounts[0].compact().unwrap();Database::open(&f.data_paths[0]).unwrap().compact().unwrap();}
        let image=f.registry.capture_account_bundle(&mut f.accounts).unwrap();let before=histories(&f);let target=dir.path().join("selected");
        durability::inject("bundle_restore_stage_sync");prop_assert!(restore_account_bundle_bytes(&image,&target,pool(),now).is_err());prop_assert!(!target.exists());
        let result=restore_account_bundle_bytes(&image,&target,pool(),now).unwrap();prop_assert_eq!(result.private_accounts[0].inventory.clock_floor,Some(now));
        let registry=ProjectStore::open_existing(target.join("registry")).unwrap();
        let rows=registry.authorize(&f.credentials[0].0,&f.credentials[0].1).unwrap().execute("SELECT id FROM t ORDER BY id",&[]).unwrap().results.remove(0).rows;
        let mut expected=vec![vec![Value::Integer(1)]];expected.extend(values.iter().map(|v|vec![Value::Integer(*v)]));prop_assert_eq!(rows,expected);
        let account=AccountStore::open(target.join("private").join(&f.credentials[0].0),&f.credentials[0].0,pool()).unwrap();
        prop_assert_eq!(account.check_password("synthetic_user",b"synthetic-password").unwrap().is_some(),!disabled);
        prop_assert_eq!(histories(&f),before);
    }
}
