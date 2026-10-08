use super::files::Worker;
use crate::{
    AccountRoot, Error, capture_account_bundle_root, durability, initialize_account_root,
    inspect_account_bundle_root, restore_account_bundle_bytes,
};
use emilybase_auth::password::PasswordPool;
use proptest::prelude::*;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

const PREFIX: &str = ".emilybase-account-init-";
fn pool() -> PasswordPool {
    PasswordPool::new(1).unwrap()
}
fn stages(parent: &Path) -> Vec<PathBuf> {
    fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(PREFIX))
        .collect()
}
fn stage(parent: &Path) -> PathBuf {
    let found = stages(parent);
    assert_eq!(found.len(), 1);
    found[0].clone()
}
fn empty(root: &Path, now: u64) {
    let report = inspect_account_bundle_root(root, pool()).unwrap();
    assert_eq!(report.reset_at, now);
    assert_eq!(report.registry.projects.len(), 1);
    assert_eq!(report.private_accounts.len(), 1);
    assert_eq!(report.registry.projects[0].tables, 0);
    assert_eq!(report.registry.projects[0].rows, 0);
    let private = &report.private_accounts[0];
    assert_eq!(private.project, report.registry.projects[0].id);
    assert_eq!(private.inventory.private_version, 3);
    assert_eq!(private.inventory.accounts, 0);
    assert_eq!(private.inventory.session_families, 0);
    assert_eq!(private.inventory.clock_floor, Some(now));
    let service = AccountRoot::open(root, pool()).unwrap();
    assert_eq!(service.projects().unwrap().len(), 1);
}
#[test]
fn new_root_contains_one_empty_private_project_and_can_run_full_lifecycle_after_operator_rotation()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root with spaces 界");
    let report = initialize_account_root(&root, "synthetic 界 project", pool(), 50).unwrap();
    assert_eq!(report, inspect_account_bundle_root(&root, pool()).unwrap());
    empty(&root, 50);
    assert!(stages(dir.path()).is_empty());
    let id = report.registry.projects[0].id.clone();
    for path in [
        &root,
        &root.join("registry"),
        &root.join("private"),
        &root.join("private").join(&id),
    ] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    assert_eq!(
        fs::metadata(root.join("root.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let mut service = AccountRoot::open(&root, pool()).unwrap();
    assert_eq!(service.projects().unwrap()[0].name, "synthetic 界 project");
    assert_eq!(service.projects().unwrap()[0].key_epoch, 1);
    assert!(AccountRoot::open(&root, pool()).is_err());
    let key = service.rotate_project_key(&id).unwrap();
    assert_eq!(key.project.key_epoch, 2);
    let user = service
        .create_user(&id, &key.api_key, "synthetic_user", b"synthetic-password")
        .unwrap();
    let pair = service
        .sign_in(
            &id,
            &key.api_key,
            "synthetic_user",
            b"synthetic-password",
            50,
        )
        .unwrap();
    assert_eq!(
        service
            .with_access(&id, &key.api_key, pair.access.expose(), 50, |p| p
                .account()
                .id)
            .unwrap(),
        user.id
    );
    service
        .execute(
            &id,
            &key.api_key,
            "CREATE TABLE t(id INT PRIMARY KEY);INSERT INTO t VALUES(1)",
            &[],
        )
        .unwrap();
    drop(service);
    let image = capture_account_bundle_root(&root, pool()).unwrap();
    let copy = dir.path().join("clone");
    restore_account_bundle_bytes(&image, &copy, pool(), 0).unwrap();
    let mut restored = AccountRoot::open(&copy, pool()).unwrap();
    assert!(
        restored
            .with_access(&id, &key.api_key, pair.access.expose(), 50, |_| ())
            .is_err()
    );
    assert_eq!(
        restored
            .execute(&id, &key.api_key, "SELECT * FROM t", &[])
            .unwrap()
            .results[0]
            .rows
            .len(),
        1
    );
    drop(restored);
    let mut original = AccountRoot::open(&root, pool()).unwrap();
    assert!(
        original
            .with_access(&id, &key.api_key, pair.access.expose(), 50, |_| ())
            .is_ok()
    );
}
#[test]
fn invalid_name_or_time_fails_before_staging_and_existing_objects_are_never_replaced() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("new-root");
    for name in [
        "",
        " ",
        "synthetic\nsecret",
        &"x".repeat(129),
        &"界".repeat(43),
    ] {
        assert!(matches!(
            initialize_account_root(&root, name, pool(), 0),
            Err(Error::Name)
        ));
    }
    assert!(matches!(
        initialize_account_root(&root, "synthetic", pool(), u64::MAX),
        Err(Error::BundleRoot(_))
    ));
    assert!(!root.exists());
    assert!(stages(dir.path()).is_empty());
    fs::write(&root, b"synthetic protected bytes").unwrap();
    let link = dir.path().join("link");
    symlink(&root, &link).unwrap();
    let directory = dir.path().join("directory");
    fs::create_dir(&directory).unwrap();
    for target in [&root, &link, &directory] {
        assert!(initialize_account_root(target, "synthetic", pool(), 0).is_err());
    }
    assert_eq!(fs::read(&root).unwrap(), b"synthetic protected bytes");
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
    assert!(stages(dir.path()).is_empty());
    let missing_parent = dir.path().join("missing-parent/new-root");
    assert!(initialize_account_root(&missing_parent, "synthetic", pool(), 0).is_err());
    assert!(!missing_parent.parent().unwrap().exists());
}
#[test]
fn sync_failures_leave_no_partial_selected_root_and_distinguish_publication_uncertainty() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for point in [
        "account_init_private_sync",
        "account_init_private_sync_after",
        "account_init_manifest_sync",
        "account_init_manifest_sync_after",
        "account_init_stage_sync",
        "account_init_stage_sync_after",
        "account_init_parent_sync",
        "account_init_parent_sync_after",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        durability::inject(point);
        let result = initialize_account_root(&root, "synthetic", pool(), 50);
        if point.contains("parent") {
            assert!(matches!(result, Err(Error::PublicationUnknown(_))));
            empty(&root, 50);
            assert!(stages(dir.path()).is_empty());
        } else {
            assert!(result.is_err());
            assert!(!root.exists());
            assert_eq!(stages(dir.path()).len(), 1);
        }
        // The operation never retries over a selected or uncertain destination.
        let retry = dir.path().join("independent-retry");
        initialize_account_root(&retry, "synthetic", pool(), 50).unwrap();
        empty(&retry, 50);
    }
}
#[test]
fn prepared_private_corruption_is_detected_and_failed_stage_foreign_entries_are_preserved() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for corrupt in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let parent = dir.path().to_owned();
        let _guard = durability::on_boundary("account_init_prepared", move || {
            let selected = stage(&parent);
            if corrupt {
                let id = fs::read_dir(selected.join("private"))
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .file_name();
                let wal = selected.join("private").join(id).join("redo.wal");
                let mut bytes = fs::read(&wal).unwrap();
                bytes[0] ^= 1;
                fs::write(wal, bytes).unwrap();
            } else {
                fs::write(selected.join("foreign"), b"synthetic foreign bytes").unwrap();
            }
        });
        assert!(initialize_account_root(&root, "synthetic", pool(), 50).is_err());
        assert!(!root.exists());
        let failed = stage(dir.path());
        if !corrupt {
            assert_eq!(
                fs::read(failed.join("foreign")).unwrap(),
                b"synthetic foreign bytes"
            );
        }
    }
}

#[test]
fn final_root_manifest_private_registry_data_and_parent_substitutions_refuse_without_sweeping_foreign_paths()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for selected in [
        "root", "manifest", "private", "account", "registry", "data", "target", "parent",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("parent");
        fs::create_dir(&parent).unwrap();
        let root = parent.join("root");
        let callback_parent = parent.clone();
        let moved = dir.path().join("detached");
        let callback_moved = moved.clone();
        let _guard = durability::on_boundary("account_init_stage_synced", move || {
            let staged = stage(&callback_parent);
            let id = fs::read_dir(staged.join("private"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .file_name();
            let path = match selected {
                "root" => staged.clone(),
                "manifest" => staged.join("root.json"),
                "private" => staged.join("private"),
                "account" => staged.join("private").join(&id),
                "registry" => staged.join("registry"),
                "data" => staged.join("registry").join(&id).join("data"),
                "target" => callback_parent.join("root"),
                "parent" => callback_parent.clone(),
                _ => unreachable!(),
            };
            if selected == "target" {
                fs::create_dir(&path).unwrap();
                fs::write(path.join("foreign"), b"synthetic protected bytes").unwrap();
            } else {
                fs::rename(&path, &callback_moved).unwrap();
                if selected == "manifest" {
                    fs::write(&path, b"synthetic protected bytes").unwrap();
                } else {
                    fs::create_dir(&path).unwrap();
                    fs::write(path.join("foreign"), b"synthetic protected bytes").unwrap();
                }
            }
        });
        assert!(
            initialize_account_root(&root, "synthetic", pool(), 50).is_err(),
            "substitution {selected}"
        );
        if selected == "target" {
            assert_eq!(
                fs::read(root.join("foreign")).unwrap(),
                b"synthetic protected bytes"
            );
        } else {
            assert!(moved.exists());
            if selected == "manifest" {
                assert_eq!(
                    fs::read(stage(&parent).join("root.json")).unwrap(),
                    b"synthetic protected bytes"
                );
            }
        }
    }
}
#[test]
fn native_kills_select_only_complete_roots_and_release_all_owners() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for point in [
        "account_init_prepared",
        "account_init_manifest_synced",
        "bundle_restore_owners_locked",
        "account_init_stage_synced",
        "account_init_renamed",
        "account_init_parent_synced",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let worker = Worker::start(dir.path(), &root, "", "root-init", point);
        worker.reach(point);
        worker.kill();
        if point == "account_init_renamed" || point == "account_init_parent_synced" {
            empty(&root, 50);
        } else {
            assert!(!root.exists());
            assert_eq!(stages(dir.path()).len(), 1);
        }
        let retry = dir.path().join("independent-retry");
        initialize_account_root(&retry, "synthetic", pool(), 50).unwrap();
    }
}
#[test]
fn two_native_initializers_publish_exactly_one_root_and_preserve_the_losing_stage() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let point = "account_init_stage_synced";
    let mut a = Worker::start(dir.path(), &root, "", "root-init-race", point);
    a.reach(point);
    let mut b = Worker::start(dir.path(), &root, "", "root-init-race", point);
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
    empty(&root, 50);
    assert_eq!(stages(dir.path()).len(), 1);
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]
    #[test]
    fn generated_bootstrap_time_and_name_match_independent_empty_roster_model(now in 0u64..=i64::MAX as u64, name in "[a-zA-Z0-9_]{1,128}") {
        let _serial = durability::PROCESS_TESTS.blocking_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        initialize_account_root(&root, &name, pool(), now).unwrap();
        empty(&root, now);
        let service = AccountRoot::open(&root, pool()).unwrap();
        prop_assert_eq!(&service.projects().unwrap()[0].name, &name);
    }
}
