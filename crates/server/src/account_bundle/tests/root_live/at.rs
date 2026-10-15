use super::*;

pub(super) fn copy_directory(source: &Path, target: &Path) {
    fs::DirBuilder::new().mode(0o700).create(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}
fn parent_at(path: &Path) -> File {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    File::open(path).unwrap()
}

#[test]
fn relative_open_keeps_original_parent_after_caller_release_and_move_without_redirecting_sql() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("parent");
        fs::DirBuilder::new().mode(0o700).create(&parent).unwrap();
        let f = restored(&parent, 1, compact);
        let before = histories(&f);
        let (id, key) = &f.credentials[0];
        let handle = parent_at(&parent);
        let root = AccountRoot::open_at(&handle, "restored", pool()).unwrap();
        drop(handle);
        assert_eq!(histories(&f), before);
        assert_eq!(root.projects().unwrap().len(), 1);
        assert!(matches!(
            AccountRoot::open_at(&parent_at(&parent), "restored", pool()),
            Err(Error::Busy)
        ));
        let moved = temp.path().join("moved");
        fs::rename(&parent, &moved).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&parent).unwrap();
        copy_directory(&moved.join("restored"), &f.root);
        root.execute(
            id,
            key,
            "CREATE TABLE at_probe (id INTEGER PRIMARY KEY, label TEXT)",
            &[],
        )
        .unwrap();
        root.execute(id, key, "INSERT INTO at_probe VALUES (1, 'original')", &[])
            .unwrap();
        assert_eq!(root.projects().unwrap().len(), 1);
        drop(root);
        assert_eq!(histories(&f), before);
        let original = AccountRoot::open(moved.join("restored"), pool()).unwrap();
        assert!(
            original
                .execute(id, key, "SELECT * FROM at_probe", &[])
                .is_ok()
        );
        drop(original);
        let replacement = AccountRoot::open(&f.root, pool()).unwrap();
        assert!(
            replacement
                .execute(id, key, "SELECT * FROM at_probe", &[])
                .is_err()
        );
    }
}

#[test]
fn relative_restart_preserves_current_sessions_time_and_exact_refresh_replay_rules() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let f = restored(temp.path(), 1, compact);
        let (id, key) = &f.credentials[0];
        let parent = parent_at(temp.path());
        let mut root = AccountRoot::open_at(&parent, "restored", pool()).unwrap();
        let first = root.sign_in(id, key, LOGIN, PASSWORD, 100).unwrap();
        let expected = histories(&f);
        drop(root);
        let mut root = AccountRoot::open_at(&parent, "restored", pool()).unwrap();
        assert_eq!(histories(&f), expected);
        root.with_access(id, key, first.access.expose(), 100, |p| {
            assert_eq!(p.account().login, LOGIN)
        })
        .unwrap();
        assert!(matches!(
            root.with_access(id, key, first.access.expose(), 99, |_| ()),
            Err(Error::Accounts(emilybase_auth::accounts::Error::Clock))
        ));
        assert_eq!(histories(&f), expected);
        let second = root
            .refresh_session(id, key, first.refresh.expose(), 100)
            .unwrap();
        assert!(
            root.refresh_session(id, key, first.refresh.expose(), 100)
                .is_err()
        );
        root.with_access(id, key, second.access.expose(), 100, |_| ())
            .unwrap();
        root.logout_session(id, key, second.refresh.expose(), 100)
            .unwrap();
        drop(root);
        let mut root = AccountRoot::open_at(&parent, "restored", pool()).unwrap();
        assert!(
            root.with_access(id, key, second.access.expose(), 100, |_| ())
                .is_err()
        );
    }
}

#[test]
fn invalid_leaf_parent_type_permissions_missing_target_and_busy_owners_leave_sources_unchanged() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let f = restored(temp.path(), 1, false);
    let before = histories(&f);
    let parent = parent_at(temp.path());
    let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
    for leaf in [
        "",
        ".",
        "..",
        "restored/",
        "./restored",
        "restored/.",
        "../restored",
        "root\0other",
        &"x".repeat(256),
        &"я".repeat(128),
    ] {
        assert!(matches!(
            AccountRoot::open_at(&parent, leaf, pool()),
            Err(Error::Path)
        ));
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
    }
    assert!(AccountRoot::open_at(&parent, "missing", pool()).is_err());
    let ordinary = File::open(f.root.join("root.json")).unwrap();
    assert!(AccountRoot::open_at(&ordinary, "restored", pool()).is_err());
    drop(ordinary);
    let owner = ProjectStore::open_existing(f.root.join("registry")).unwrap();
    assert!(AccountRoot::open_at(&parent, "restored", pool()).is_err());
    drop(owner);
    let original = AccountRoot::open(&f.root, pool()).unwrap();
    assert!(matches!(
        AccountRoot::open_at(&parent, "restored", pool()),
        Err(Error::Busy)
    ));
    drop(original);
    assert_eq!(histories(&f), before);
    assert!(!temp.path().join("missing").exists());
    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(AccountRoot::open_at(&parent, "restored", pool()).is_err());
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(histories(&f), before);
}

#[test]
fn selected_child_manifest_private_registry_and_parent_changes_refuse_without_adoption_or_cleanup()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for kind in 0..8 {
            let temp = tempfile::tempdir().unwrap();
            let f = restored(temp.path(), 1, compact);
            let parent = parent_at(temp.path());
            let root = AccountRoot::open_at(&parent, "restored", pool()).unwrap();
            let id = &f.credentials[0].0;
            let saved = temp.path().join("saved");
            match kind {
                0 => {
                    fs::rename(&f.root, &saved).unwrap();
                    copy_directory(&saved, &f.root);
                }
                1..=3 => {
                    let child = match kind {
                        1 => f.root.join("registry"),
                        2 => f.root.join("private"),
                        _ => f.root.join("private").join(id),
                    };
                    fs::rename(&child, &saved).unwrap();
                    copy_directory(&saved, &child);
                }
                4 => {
                    let file = f.root.join("root.json");
                    fs::rename(&file, &saved).unwrap();
                    fs::copy(&saved, file).unwrap();
                }
                5 => fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).unwrap(),
                6 => fs::set_permissions(&f.root, fs::Permissions::from_mode(0o755)).unwrap(),
                _ => fs::write(f.root.join("foreign"), b"preserve").unwrap(),
            }
            assert!(root.projects().is_err(), "kind={kind}");
            drop(root);
            if kind == 7 {
                assert_eq!(fs::read(f.root.join("foreign")).unwrap(), b"preserve");
            }
            if kind < 5 {
                assert!(saved.exists());
            }
            if kind == 5 {
                fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
    }
}

#[test]
fn parent_refusal_during_original_inspection_releases_every_partially_acquired_owner() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let f = restored(temp.path(), 1, false);
    let before = histories(&f);
    let parent = parent_at(temp.path());
    let path = temp.path().to_path_buf();
    let guard = durability::on_boundary("bundle_restore_inventory_validated", move || {
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    });
    assert!(AccountRoot::open_at(&parent, "restored", pool()).is_err());
    drop(guard);
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(histories(&f), before);
    AccountRoot::open_at(&parent, "restored", pool()).unwrap();
}

#[test]
fn original_parent_lock_outlives_caller_handle_and_ends_after_all_child_owners() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let f = restored(temp.path(), 1, false);
    let parent = parent_at(temp.path());
    parent.try_lock().unwrap();
    let before = histories(&f);
    let fds = fs::read_dir("/proc/self/fd").unwrap().count();
    let root = AccountRoot::open_at(&parent, "restored", pool()).unwrap();
    drop(parent);
    let observer = File::open(temp.path()).unwrap();
    assert!(observer.try_lock().is_err());
    assert!(File::open(&f.root).unwrap().try_lock().is_err());
    assert!(ProjectStore::open_existing(f.root.join("registry")).is_err());
    assert!(
        AccountStore::open(
            f.root.join("private").join(&f.credentials[0].0),
            &f.credentials[0].0,
            pool()
        )
        .is_err()
    );
    assert_eq!(root.projects().unwrap().len(), 1);
    drop(root);
    observer.try_lock().unwrap();
    drop(observer);
    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds - 1);
    assert_eq!(histories(&f), before);
}

#[test]
fn relative_active_limit_refuses_before_registry_admission_and_never_creates_missing_members() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for count in [4, 5] {
        let temp = tempfile::tempdir().unwrap();
        let f = restored(temp.path(), count, false);
        let parent = parent_at(temp.path());
        let before = histories(&f);
        if count == 5 {
            let registry = ProjectStore::open_existing(f.root.join("registry")).unwrap();
            assert!(matches!(
                AccountRoot::open_at(&parent, "restored", pool()),
                Err(Error::Limit)
            ));
            drop(registry);
        } else {
            assert_eq!(
                AccountRoot::open_at(&parent, "restored", pool())
                    .unwrap()
                    .projects()
                    .unwrap()
                    .len(),
                4
            );
        }
        assert_eq!(histories(&f), before);
    }
    let temp = tempfile::tempdir().unwrap();
    let f = restored(temp.path(), 1, false);
    let parent = parent_at(temp.path());
    let missing = f.root.join("registry");
    let saved = temp.path().join("saved");
    fs::rename(&missing, &saved).unwrap();
    assert!(AccountRoot::open_at(&parent, "restored", pool()).is_err());
    assert!(!missing.exists());
    assert!(saved.exists());
}

#[test]
fn relative_registry_backup_checks_physical_containment_and_restores_exact_external_image() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let f = restored(temp.path(), 1, false);
    let before = histories(&f);
    let registry_path = f.root.join("registry");
    use std::os::fd::AsRawFd;
    let parent = parent_at(temp.path());
    let alias = PathBuf::from(format!(
        "/proc/self/fd/{}/./restored/registry",
        parent.as_raw_fd()
    ));
    let mut registry = ProjectStore::open_existing_relative(&alias).unwrap();
    let inside = registry_path.join(&f.credentials[0].0).join("data/denied");
    assert!(matches!(registry.backup(&inside), Err(Error::Path)));
    assert!(!inside.exists());
    let mut accounts = vec![
        AccountStore::open(
            f.root.join("private").join(&f.credentials[0].0),
            &f.credentials[0].0,
            pool(),
        )
        .unwrap(),
    ];
    assert!(matches!(
        registry.backup_account_bundle(&mut accounts, &inside),
        Err(Error::Path)
    ));
    assert!(!inside.exists());
    let expected = registry.capture_account_bundle(&mut accounts).unwrap();
    let target = temp.path().join("external.bundle");
    let report = registry
        .backup_account_bundle(&mut accounts, &target)
        .unwrap();
    assert_eq!(fs::read(&target).unwrap(), expected);
    assert_eq!(crate::inspect_account_bundle(&target).unwrap(), report);
    drop(accounts);
    drop(registry);
    assert_eq!(histories(&f), before);
    let copy = temp.path().join("copy");
    restore_account_bundle_bytes(&expected, &copy, pool(), 100).unwrap();
    let root = AccountRoot::open_at(&parent_at(temp.path()), "copy", pool()).unwrap();
    assert_eq!(root.projects().unwrap().len(), 1);
}

#[test]
fn killed_relative_readers_release_parent_root_and_children_without_history_changes() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (1, false), (1, true)] {
        for point in ["account_root_service_opened", "account_root_at_read"] {
            let temp = tempfile::tempdir().unwrap();
            let f = restored(temp.path(), count, compact);
            let parent = parent_at(temp.path());
            let before = histories(&f);
            let worker = Worker::start(&f.root, &temp.path().join("unused"), "", "root-at", point);
            worker.reach(point);
            assert!(File::open(temp.path()).unwrap().try_lock().is_err());
            assert!(File::open(&f.root).unwrap().try_lock().is_err());
            worker.kill();
            assert_eq!(histories(&f), before);
            let mut root = AccountRoot::open_at(&parent, "restored", pool()).unwrap();
            assert_eq!(root.projects().unwrap().len(), count);
            if let Some((id, key)) = f.credentials.first() {
                let pair = root.sign_in(id, key, LOGIN, PASSWORD, 100).unwrap();
                root.with_access(id, key, pair.access.expose(), 100, |_| ())
                    .unwrap();
            }
        }
    }
}

#[test]
fn original_selection_check_accepts_same_parent_owner_and_refuses_copies_symlinks_and_bad_leaves() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for relative in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let f = restored(temp.path(), 1, false);
        let parent = parent_at(temp.path());
        let before = histories(&f);
        let root = if relative {
            AccountRoot::open_at(&parent, "restored", pool()).unwrap()
        } else {
            AccountRoot::open(&f.root, pool()).unwrap()
        };
        root.check_directory_at(&parent, "restored").unwrap();
        let another = File::open(temp.path()).unwrap();
        root.check_directory_at(&another, "restored").unwrap();
        copy_directory(&f.root, &temp.path().join("copy"));
        symlink(&f.root, temp.path().join("alias")).unwrap();
        for leaf in [
            "copy",
            "alias",
            "missing",
            "../restored",
            "restored/",
            ".",
            "..",
            "",
        ] {
            assert!(root.check_directory_at(&parent, leaf).is_err());
        }
        assert_eq!(root.projects().unwrap().len(), 1);
        root.check_directory_at(&parent, "restored").unwrap();
        drop(root);
        assert_eq!(histories(&f), before);
    }
}

#[test]
fn selected_namespace_changes_after_service_admission_refuse_before_original_data_open() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for kind in 0..4 {
            let temp = tempfile::tempdir().unwrap();
            let f = restored(temp.path(), 1, compact);
            let parent = parent_at(temp.path());
            let root = AccountRoot::open_at(&parent, "restored", pool()).unwrap();
            let (id, key) = &f.credentials[0];
            let data = f.root.join("registry").join(id).join("data");
            let before = fs::read(data.join("redo.wal")).unwrap();
            let saved = temp.path().join("saved");
            let selected = match kind {
                0 => f.root.clone(),
                1 => f.root.join("registry"),
                2 => f.root.join("registry").join(id),
                _ => data.clone(),
            };
            let original = match kind {
                0 => saved.join("registry").join(id).join("data"),
                1 => saved.join(id).join("data"),
                2 => saved.join("data"),
                _ => saved.clone(),
            };
            let saved_hook = saved.clone();
            let guard = durability::on_boundary("owned_project_directory_check", move || {
                fs::rename(&selected, &saved_hook).unwrap();
                copy_directory(&saved_hook, &selected);
            });
            assert!(
                root.execute(
                    id,
                    key,
                    "CREATE TABLE denied_probe (id INT PRIMARY KEY)",
                    &[]
                )
                .is_err(),
                "kind={kind}"
            );
            drop(guard);
            drop(root);
            assert_eq!(fs::read(data.join("redo.wal")).unwrap(), before);
            assert_eq!(fs::read(original.join("redo.wal")).unwrap(), before);
            assert!(saved.exists());
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_rosters_and_parent_moves_keep_original_declared_scope_and_release_locks(count in 0usize..=4, compact in any::<bool>(), move_parent in any::<bool>()) {
        let _serial=durability::PROCESS_TESTS.blocking_lock();let temp=tempfile::tempdir().unwrap();let parent=temp.path().join("parent");fs::DirBuilder::new().mode(0o700).create(&parent).unwrap();let f=restored(&parent,count,compact);let expected=histories(&f);let handle=parent_at(&parent);let root=AccountRoot::open_at(&handle,"restored",pool()).unwrap();drop(handle);let moved=temp.path().join("moved");if move_parent {fs::rename(&parent,&moved).unwrap();}
        prop_assert_eq!(root.projects().unwrap().len(),count);drop(root);let original=if move_parent {moved} else {parent};let restored=Restored{root:original.join("restored"),credentials:f.credentials};prop_assert_eq!(histories(&restored),expected);let parent=parent_at(&original);prop_assert_eq!(AccountRoot::open_at(&parent,"restored",pool()).unwrap().projects().unwrap().len(),count);
    }
}
