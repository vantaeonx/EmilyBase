use super::*;
use emilybase_auth::key_file::read_api_key_file;
use std::os::unix::fs::MetadataExt;

fn assert_only_selected_metadata_changed(before: &[Vec<u8>], after: &[Vec<u8>]) {
    assert_eq!(before.len(), after.len());
    assert_ne!(before[1], after[1]);
    for (i, (a, b)) in before.iter().zip(after).enumerate() {
        if i != 1 {
            assert_eq!(a, b);
        }
    }
}
fn stages(parent: &Path) -> Vec<PathBuf> {
    fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".emilybase-service-key-")
        })
        .collect()
}

#[test]
fn key_file_rotation_saves_exact_private_key_before_activation_and_preserves_users_rows_and_siblings()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = restored(dir.path(), 2, compact);
        let (id, old) = &f.credentials[0];
        let mut root = AccountRoot::open(&f.root, pool()).unwrap();
        let session = root.sign_in(id, old, LOGIN, PASSWORD, 50).unwrap();
        let before = histories(&f);
        let output = dir.path().join("ключ 界 с пробелами.key");
        let info = root.rotate_project_key_to_file(id, &output).unwrap();
        assert_eq!(info.id, *id);
        assert_eq!(info.key_epoch, 2);
        assert_eq!(fs::metadata(&output).unwrap().len(), 64);
        assert_eq!(fs::metadata(&output).unwrap().mode() & 0o7777, 0o600);
        assert_eq!(fs::metadata(&output).unwrap().nlink(), 1);
        let key = read_api_key_file(&output).unwrap();
        assert!(old != key.as_str());
        denied(root.list_users(id, old, None, 1));
        root.with_access(id, &key, session.access.expose(), 50, |_| ())
            .unwrap();
        assert_eq!(
            root.execute(id, &key, "SELECT * FROM t", &[])
                .unwrap()
                .results[0]
                .rows
                .len(),
            1
        );
        assert_only_selected_metadata_changed(&before, &histories(&f));
        assert!(stages(dir.path()).is_empty());
        let after = histories(&f);
        assert!(root.rotate_project_key_to_file(id, &output).is_err());
        assert_eq!(histories(&f), after);
        drop(root);
        let mut root = AccountRoot::open(&f.root, pool()).unwrap();
        assert_eq!(root.list_users(id, &key, None, 128).unwrap().users.len(), 1);
        let (sibling, sibling_key) = &f.credentials[1];
        assert_eq!(
            root.list_users(sibling, sibling_key, None, 128)
                .unwrap()
                .users
                .len(),
            1
        );
    }
}

#[test]
fn protected_targets_internal_ancestors_and_unknown_projects_never_rotate_or_replace_files() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let f = restored(dir.path(), 1, false);
    let (id, key) = &f.credentials[0];
    let before = histories(&f);
    let protected = dir.path().join("protected");
    fs::write(&protected, b"synthetic protected bytes").unwrap();
    let link = dir.path().join("link");
    symlink(&protected, &link).unwrap();
    let dangling = dir.path().join("dangling");
    symlink(dir.path().join("absent"), &dangling).unwrap();
    let folder = dir.path().join("folder");
    fs::create_dir(&folder).unwrap();
    let alias = dir.path().join("root-alias");
    symlink(&f.root, &alias).unwrap();
    let mut root = AccountRoot::open(&f.root, pool()).unwrap();
    for target in [
        &protected,
        &link,
        &dangling,
        &folder,
        &f.root.join("forbidden.key"),
        &f.root.join("private").join(id).join("forbidden.key"),
        &alias.join("registry").join(id).join("data/forbidden.key"),
    ] {
        assert!(root.rotate_project_key_to_file(id, target).is_err());
    }
    let missing = dir.path().join("missing.key");
    assert!(
        root.rotate_project_key_to_file(&"f".repeat(32), &missing)
            .is_err()
    );
    assert!(!missing.exists());
    assert_eq!(fs::read(&protected).unwrap(), b"synthetic protected bytes");
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 0);
    assert_eq!(histories(&f), before);
    assert!(stages(dir.path()).is_empty());
    root.list_users(id, key, None, 1).unwrap();
}

#[test]
fn key_file_sync_failures_distinguish_preparation_published_inactive_and_active_uncertainty() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for phase in [
        "service_key_file_sync",
        "service_key_parent_sync",
        "rotate_file_sync",
        "rotate_directory_sync",
        "service_key_final_parent_sync",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let f = restored(dir.path(), 1, false);
        let (id, old) = &f.credentials[0];
        let before = histories(&f);
        let target = dir.path().join("next.key");
        let mut root = AccountRoot::open(&f.root, pool()).unwrap();
        durability::inject(phase);
        let outcome = root.rotate_project_key_to_file(id, &target);
        if phase == "service_key_file_sync" {
            assert!(matches!(outcome, Err(Error::Io(_))));
            assert!(!target.exists());
            assert!(stages(dir.path()).is_empty());
        } else {
            assert!(matches!(outcome, Err(Error::PublicationUnknown(_))));
            assert!(target.exists());
        }
        drop(root);
        let mut root = AccountRoot::open(&f.root, pool()).unwrap();
        let active = matches!(
            phase,
            "rotate_directory_sync" | "service_key_final_parent_sync"
        );
        if active {
            let key = read_api_key_file(&target).unwrap();
            denied(root.list_users(id, old, None, 1));
            root.list_users(id, &key, None, 1).unwrap();
            assert_only_selected_metadata_changed(&before, &histories(&f));
        } else {
            root.list_users(id, old, None, 1).unwrap();
            assert_eq!(histories(&f), before);
            if target.exists() {
                let key = read_api_key_file(&target).unwrap();
                denied(root.list_users(id, &key, None, 1));
            }
        }
    }
}

#[test]
fn key_file_crash_matrix_keeps_an_external_key_for_every_activated_digest_on_both_wals() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for point in [
            "service_key_file_synced",
            "service_key_file_published",
            "service_key_parent_synced",
            "rotate_file_synced",
            "rotate_renamed",
            "service_key_activated",
            "service_key_ack",
            "service_key_received",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let f = restored(dir.path(), 1, compact);
            let (id, old) = &f.credentials[0];
            let before = histories(&f);
            let target = dir.path().join("next.key");
            let worker = Worker::start(&f.root, &target, id, "root-key-file", point);
            worker.reach(point);
            worker.kill();
            let mut root = AccountRoot::open(&f.root, pool()).unwrap();
            let active = matches!(
                point,
                "rotate_renamed"
                    | "service_key_activated"
                    | "service_key_ack"
                    | "service_key_received"
            );
            if active {
                let key = read_api_key_file(&target).unwrap();
                denied(root.list_users(id, old, None, 1));
                root.list_users(id, &key, None, 1).unwrap();
                assert_only_selected_metadata_changed(&before, &histories(&f));
            } else {
                root.list_users(id, old, None, 1).unwrap();
                assert_eq!(histories(&f), before);
            }
            if point == "service_key_file_synced" {
                assert!(!target.exists());
                assert_eq!(stages(dir.path()).len(), 1);
            } else {
                assert!(target.exists());
                assert_eq!(fs::metadata(&target).unwrap().mode() & 0o7777, 0o600);
            }
        }
    }
}

#[test]
fn a_racing_existing_target_never_activates_or_overwrites_and_changed_parent_is_preserved() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for replace_parent in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = restored(dir.path(), 1, false);
        let (id, key) = &f.credentials[0];
        let before = histories(&f);
        let parent = dir.path().join("outputs");
        fs::create_dir(&parent).unwrap();
        let target = parent.join("next.key");
        let worker = Worker::start(
            &f.root,
            &target,
            id,
            "root-key-file-race",
            "service_key_file_synced",
        );
        worker.reach("service_key_file_synced");
        let detached = dir.path().join("detached");
        let mut foreign_stage = None;
        if replace_parent {
            let name = stages(&parent)[0].file_name().unwrap().to_os_string();
            fs::rename(&parent, &detached).unwrap();
            fs::create_dir(&parent).unwrap();
            let path = parent.join(name);
            fs::write(&path, b"foreign synthetic stage").unwrap();
            foreign_stage = Some(path);
        }
        fs::write(&target, b"foreign protected target").unwrap();
        let mut worker = worker;
        worker.release();
        let log = worker.finish();
        assert!(log.contains("KEY_FILE_CONFLICT"));
        assert_eq!(fs::read(&target).unwrap(), b"foreign protected target");
        assert_eq!(histories(&f), before);
        AccountRoot::open(&f.root, pool())
            .unwrap()
            .list_users(id, key, None, 1)
            .unwrap();
        if replace_parent {
            // Cleanup unlinks only its retained original inode, never the new alias.
            assert!(stages(&detached).is_empty());
            assert_eq!(
                fs::read(foreign_stage.unwrap()).unwrap(),
                b"foreign synthetic stage"
            );
        } else {
            assert!(stages(&parent).is_empty());
        }
    }
}

#[test]
fn published_file_permissions_or_content_change_before_activation_keeps_old_digest() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for permissions in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = restored(dir.path(), 1, false);
        let (id, key) = &f.credentials[0];
        let before = histories(&f);
        let target = dir.path().join("next.key");
        let worker = Worker::start(
            &f.root,
            &target,
            id,
            "root-key-file-race",
            "service_key_parent_synced",
        );
        worker.reach("service_key_parent_synced");
        if permissions {
            fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        } else {
            fs::write(&target, "c".repeat(64)).unwrap();
        }
        let mut worker = worker;
        worker.release();
        let log = worker.finish();
        assert!(log.contains("KEY_FILE_UNCERTAIN"));
        assert_eq!(histories(&f), before);
        AccountRoot::open(&f.root, pool())
            .unwrap()
            .list_users(id, key, None, 1)
            .unwrap();
        assert!(target.exists());
    }
}

#[test]
fn target_substitution_after_activation_refuses_success_and_preserves_the_foreign_file() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let f = restored(dir.path(), 1, false);
    let (id, old) = &f.credentials[0];
    let before = histories(&f);
    let target = dir.path().join("next.key");
    let detached = dir.path().join("detached.key");
    let worker = Worker::start(
        &f.root,
        &target,
        id,
        "root-key-file-race",
        "service_key_activated",
    );
    worker.reach("service_key_activated");
    fs::rename(&target, &detached).unwrap();
    fs::write(&target, b"foreign synthetic final file").unwrap();
    let mut worker = worker;
    worker.release();
    let log = worker.finish();
    assert!(log.contains("KEY_FILE_UNCERTAIN"));
    assert!(!log.contains("KEY_FILE_OK"));
    let mut root = AccountRoot::open(&f.root, pool()).unwrap();
    denied(root.list_users(id, old, None, 1));
    let key = read_api_key_file(&detached).unwrap();
    root.list_users(id, &key, None, 1).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"foreign synthetic final file");
    assert_only_selected_metadata_changed(&before, &histories(&f));
}
