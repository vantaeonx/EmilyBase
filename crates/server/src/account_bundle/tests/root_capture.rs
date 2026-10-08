use super::{files::Worker, fixture};
use crate::{
    ProjectStore, backup_account_bundle_root, capture_account_bundle_root, durability,
    inspect_account_bundle, inspect_account_bundle_bytes, inspect_account_bundle_root,
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

fn pool() -> PasswordPool {
    PasswordPool::new(1).unwrap()
}
fn restored(parent: &Path, count: usize, compact: bool) -> PathBuf {
    let mut f = fixture(parent, count);
    if compact {
        for account in &mut f.accounts {
            account.compact().unwrap();
        }
        for path in &f.data_paths {
            Database::open(path).unwrap().compact().unwrap();
        }
    }
    let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
    let root = parent.join("restored");
    restore_account_bundle_bytes(&image, &root, pool(), 50).unwrap();
    root
}
fn histories(root: &Path) -> Vec<Vec<u8>> {
    let report = inspect_account_bundle_root(root, pool()).unwrap();
    let mut bytes = vec![fs::read(root.join("root.json")).unwrap()];
    for p in report.registry.projects {
        bytes.push(fs::read(root.join("registry").join(p.id).join("data/redo.wal")).unwrap());
    }
    for p in report.private_accounts {
        bytes.push(fs::read(root.join("private").join(p.project).join("redo.wal")).unwrap());
    }
    bytes
}

#[test]
fn rooted_capture_equals_explicit_common_capture_and_file_without_resetting_source() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let root = restored(dir.path(), 3, compact);
        let before = histories(&root);
        let report = inspect_account_bundle_root(&root, pool()).unwrap();
        let expected = {
            let mut registry = ProjectStore::open_existing(root.join("registry")).unwrap();
            let mut accounts = report
                .private_accounts
                .iter()
                .map(|p| {
                    AccountStore::open(root.join("private").join(&p.project), &p.project, pool())
                        .unwrap()
                })
                .collect::<Vec<_>>();
            registry.capture_account_bundle(&mut accounts).unwrap()
        };
        let image = capture_account_bundle_root(&root, pool()).unwrap();
        assert_eq!(image, expected);
        let target = dir.path().join("root.account-bundle");
        let saved = backup_account_bundle_root(&root, &target, pool()).unwrap();
        assert_eq!(fs::read(&target).unwrap(), image);
        assert_eq!(inspect_account_bundle(&target).unwrap(), saved);
        assert_eq!(saved.registry, report.registry);
        assert_eq!(saved.private_accounts, report.private_accounts);
        assert_eq!(histories(&root), before);
        assert_eq!(
            fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn all_source_owners_span_root_capture_and_release_after_success_or_final_refusal() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let root = restored(dir.path(), 3, false);
    let before = histories(&root);
    let report = inspect_account_bundle_root(&root, pool()).unwrap();
    let ids = report
        .private_accounts
        .iter()
        .map(|p| p.project.clone())
        .collect::<Vec<_>>();
    let callback_root = root.clone();
    let called = Arc::new(AtomicBool::new(false));
    let observed = called.clone();
    let _guard = durability::on_boundary("bundle_restore_inventory_validated", move || {
        assert!(File::open(&callback_root).unwrap().try_lock().is_err());
        for id in ids {
            assert!(Database::open(callback_root.join("registry").join(&id).join("data")).is_err());
            assert!(
                AccountStore::open(callback_root.join("private").join(&id), &id, pool()).is_err()
            );
        }
        observed.store(true, Ordering::SeqCst);
    });
    capture_account_bundle_root(&root, pool()).unwrap();
    assert!(called.load(Ordering::SeqCst));
    assert_eq!(histories(&root), before);
    let callback_root = root.clone();
    let _guard = durability::on_boundary("bundle_restore_inventory_validated", move || {
        fs::write(callback_root.join("foreign"), b"preserve").unwrap();
    });
    let target = dir.path().join("refused.account-bundle");
    assert!(backup_account_bundle_root(&root, &target, pool()).is_err());
    assert!(!target.exists());
    assert_eq!(fs::read(root.join("foreign")).unwrap(), b"preserve");
    fs::remove_file(root.join("foreign")).unwrap();
    assert_eq!(histories(&root), before);
}

#[test]
fn rooted_backup_refuses_internal_targets_foreign_files_and_unlisted_private_entries() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let root = restored(dir.path(), 1, false);
    let before = histories(&root);
    for target in [
        root.join("forbidden.account-bundle"),
        root.join("private/forbidden.account-bundle"),
        root.join("root.json"),
    ] {
        assert!(backup_account_bundle_root(&root, &target, pool()).is_err());
    }
    let protected = dir.path().join("foreign");
    fs::write(&protected, b"preserve").unwrap();
    let alias = dir.path().join("alias");
    symlink(&protected, &alias).unwrap();
    for target in [&protected, &alias] {
        assert!(backup_account_bundle_root(&root, target, pool()).is_err());
    }
    assert_eq!(fs::read(protected).unwrap(), b"preserve");
    assert!(
        fs::symlink_metadata(alias)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let missing = dir.path().join("missing");
    let target = dir.path().join("out.account-bundle");
    assert!(backup_account_bundle_root(&missing, &target, pool()).is_err());
    assert!(!missing.exists());
    assert!(!target.exists());
    let extra = root.join("private").join("f".repeat(32));
    fs::create_dir(&extra).unwrap();
    assert!(backup_account_bundle_root(&root, &target, pool()).is_err());
    assert!(!target.exists());
    assert!(extra.is_dir());
    fs::remove_dir(extra).unwrap();
    assert_eq!(histories(&root), before);
}

#[test]
fn root_backup_faults_preserve_exact_sources_and_selected_uncertainty() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let root = restored(dir.path(), 1, compact);
        let image = capture_account_bundle_root(&root, pool()).unwrap();
        let before = histories(&root);
        for (i, point) in [
            "bundle_backup_file_sync",
            "bundle_backup_file_sync_after",
            "bundle_backup_parent_sync",
            "bundle_backup_parent_sync_after",
        ]
        .into_iter()
        .enumerate()
        {
            let target = dir.path().join(format!("fault-{i}.account-bundle"));
            durability::inject(point);
            let result = backup_account_bundle_root(&root, &target, pool());
            if point.contains("parent") {
                assert!(matches!(result, Err(crate::Error::PublicationUnknown(_))));
                assert_eq!(fs::read(target).unwrap(), image);
            } else {
                assert!(result.is_err());
                assert!(!target.exists());
            }
            assert_eq!(histories(&root), before);
            backup_account_bundle_root(
                &root,
                dir.path().join(format!("retry-{i}.account-bundle")),
                pool(),
            )
            .unwrap();
        }
    }
}

#[test]
fn native_root_backup_kills_preserve_exact_source_scopes_histories_and_complete_files() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let root = restored(dir.path(), 1, compact);
        let before = histories(&root);
        let image = capture_account_bundle_root(&root, pool()).unwrap();
        for point in [
            "registry_capture_owners_locked",
            "bundle_restore_inventory_validated",
            "bundle_backup_file_synced",
            "bundle_backup_renamed",
            "bundle_backup_parent_synced",
            "bundle_backup_ack",
        ] {
            let target = dir.path().join(format!("{point}.account-bundle"));
            let worker = Worker::start(&root, &target, "", "root-backup", point);
            worker.reach(point);
            worker.kill();
            let selected = matches!(
                point,
                "bundle_backup_renamed" | "bundle_backup_parent_synced" | "bundle_backup_ack"
            );
            assert_eq!(target.exists(), selected);
            if selected {
                assert_eq!(fs::read(&target).unwrap(), image);
                assert!(inspect_account_bundle(&target).is_ok());
            }
            assert_eq!(histories(&root), before);
            backup_account_bundle_root(
                &root,
                dir.path().join(format!("retry-{point}.account-bundle")),
                pool(),
            )
            .unwrap();
        }
    }
}

#[test]
fn two_native_root_backups_select_one_file_from_the_same_source() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let root = restored(dir.path(), 3, false);
    let image = capture_account_bundle_root(&root, pool()).unwrap();
    let before = histories(&root);
    let target = dir.path().join("selected.account-bundle");
    let point = "bundle_backup_file_synced";
    let mut a = Worker::start(&root, &target, "", "root-backup-race", point);
    a.reach(point);
    // Source capture is exclusive. Begin the second capture after the first
    // releases its source owners, while both output publishers still race.
    let mut b = Worker::start(&root, &target, "", "root-backup-race", point);
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
    assert_eq!(fs::read(target).unwrap(), image);
    assert_eq!(histories(&root), before);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn root_rebackup_preserves_independent_row_and_disabled_models_without_resetting_source(
        values in prop::collection::btree_set(2_i64..32,0..12),disabled in any::<bool>(),compact in any::<bool>()
    ) {
        let _serial=durability::PROCESS_TESTS.blocking_lock();let dir=tempfile::tempdir().unwrap();let root=restored(dir.path(),1,compact);
        let report=inspect_account_bundle_root(&root,pool()).unwrap();let id=report.registry.projects[0].id.clone();
        {let mut db=Database::open(root.join("registry").join(&id).join("data")).unwrap();for value in &values {let mut tx=db.begin().unwrap();tx.insert("t",vec![Value::Integer(*value),Value::Integer(*value)]).unwrap();tx.commit().unwrap();}}
        {let mut account=AccountStore::open(root.join("private").join(&id),&id,pool()).unwrap();account.set_disabled("synthetic_user",disabled).unwrap();}
        let before=histories(&root);let image=capture_account_bundle_root(&root,pool()).unwrap();let target=dir.path().join("copy");
        let report=restore_account_bundle_bytes(&image,&target,pool(),0).unwrap();prop_assert_eq!(report.registry.projects[0].rows,values.len()+1);
        let db=Database::open(target.join("registry").join(&id).join("data")).unwrap();let rows=db.view().unwrap().scan("t",100).unwrap();
        let mut expected=vec![vec![Value::Integer(1),Value::Integer(0)]];
        expected.extend(values.iter().map(|value|vec![Value::Integer(*value),Value::Integer(*value)]));
        prop_assert_eq!(rows,expected);
        let account=AccountStore::open(target.join("private").join(&id),&id,pool()).unwrap();prop_assert_eq!(account.check_password("synthetic_user",b"synthetic-password").unwrap().is_some(),!disabled);
        prop_assert_eq!(histories(&root),before);prop_assert_eq!(inspect_account_bundle_bytes(&image).unwrap().registry.projects[0].rows,values.len()+1);
    }
}
