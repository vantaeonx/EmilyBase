use super::at::copy_directory;
use super::*;
use std::os::unix::fs::MetadataExt;

fn parent_at(path: &Path) -> File {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    File::open(path).unwrap()
}

#[test]
fn live_snapshot_matches_existing_offline_v1_image_and_owns_no_source_descriptors() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for count in [0, 1, 3] {
            for relative in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let f = restored(temp.path(), count, compact);
                let parent = parent_at(temp.path());
                let expected = crate::capture_account_bundle_root(&f.root, pool()).unwrap();
                let report = inspect_account_bundle_root(&f.root, pool()).unwrap();
                let before = histories(&f);
                let fds = fs::read_dir("/proc/self/fd").unwrap().count();
                let mut root = if relative {
                    AccountRoot::open_at(&parent, "restored", pool()).unwrap()
                } else {
                    AccountRoot::open(&f.root, pool()).unwrap()
                };
                let snapshot = root.capture().unwrap();
                assert_eq!(snapshot.bundle_bytes(), expected);
                assert_eq!(snapshot.report(), &report);
                assert_eq!(
                    snapshot.manifest(),
                    &crate::inspect_account_bundle_root_manifest_bytes(&before[0]).unwrap()
                );
                assert!(!format!("{snapshot:?}").contains("synthetic"));
                assert_eq!(histories(&f), before);
                drop(root);
                assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
                assert_eq!(snapshot.into_bytes(), expected);
            }
        }
    }
}

#[test]
fn original_all_data_private_registry_root_and_parent_owners_span_coordination_callback() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let f = restored(temp.path(), 4, true);
    let parent = parent_at(temp.path());
    parent.try_lock().unwrap();
    let mut root = AccountRoot::open_at(&parent, "restored", pool()).unwrap();
    drop(parent);
    let before = histories(&f);
    let snapshot = root
        .capture_with(|snapshot| {
            assert!(File::open(temp.path()).unwrap().try_lock().is_err());
            assert!(File::open(&f.root).unwrap().try_lock().is_err());
            assert!(ProjectStore::open_existing(f.root.join("registry")).is_err());
            for (id, _) in &f.credentials {
                assert!(AccountStore::open(f.root.join("private").join(id), id, pool()).is_err());
                assert!(Database::open(f.root.join("registry").join(id).join("data")).is_err());
            }
            Ok(snapshot)
        })
        .unwrap();
    assert_eq!(snapshot.report().private_accounts.len(), 4);
    assert_eq!(histories(&f), before);
    for (id, _) in &f.credentials {
        Database::open(f.root.join("registry").join(id).join("data")).unwrap();
    }
    let result: crate::Result<()> = root.capture_with(|_| Err(Error::Limit));
    assert!(matches!(result, Err(Error::Limit)));
    assert_eq!(histories(&f), before);
    assert_eq!(root.projects().unwrap().len(), 4);
    drop(root);
    File::open(temp.path()).unwrap().try_lock().unwrap();
}

#[test]
fn captured_image_survives_live_mutation_and_restore_revokes_only_the_copys_old_sessions() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let f = restored(temp.path(), 1, compact);
        let (id, key) = &f.credentials[0];
        let mut root = AccountRoot::open(&f.root, pool()).unwrap();
        let pair = root.sign_in(id, key, LOGIN, PASSWORD, 100).unwrap();
        let before = histories(&f);
        let snapshot = root.capture().unwrap();
        assert_eq!(histories(&f), before);
        let expected = snapshot.bundle_bytes().to_vec();
        root.execute(id, key, "INSERT INTO t VALUES(2,17)", &[])
            .unwrap();
        root.with_access(id, key, pair.access.expose(), 100, |_| ())
            .unwrap();
        assert_eq!(snapshot.bundle_bytes(), expected);
        assert_ne!(root.capture().unwrap().bundle_bytes(), expected);
        let copy = temp.path().join("copy");
        restore_account_bundle_bytes(snapshot.bundle_bytes(), &copy, pool(), 200).unwrap();
        let mut clone = AccountRoot::open_at(&parent_at(temp.path()), "copy", pool()).unwrap();
        assert!(
            clone
                .with_access(id, key, pair.access.expose(), 200, |_| ())
                .is_err()
        );
        assert_eq!(
            clone
                .execute(id, key, "SELECT * FROM t", &[])
                .unwrap()
                .results[0]
                .rows
                .len(),
            1
        );
        clone
            .execute(id, key, "INSERT INTO t VALUES(3,21)", &[])
            .unwrap();
        assert_eq!(
            root.execute(id, key, "SELECT * FROM t", &[])
                .unwrap()
                .results[0]
                .rows
                .len(),
            2
        );
        root.with_access(id, key, pair.access.expose(), 100, |_| ())
            .unwrap();
    }
}

#[test]
fn same_byte_private_and_public_wal_replacements_during_checked_capture_are_not_adopted() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for private in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let f = restored(temp.path(), 1, compact);
            let mut root = AccountRoot::open(&f.root, pool()).unwrap();
            let id = &f.credentials[0].0;
            let wal = if private {
                f.root.join("private").join(id).join("redo.wal")
            } else {
                f.root.join("registry").join(id).join("data/redo.wal")
            };
            let bytes = fs::read(&wal).unwrap();
            let saved = temp.path().join("saved");
            let result = root.capture_with(|snapshot| {
                fs::rename(&wal, &saved).unwrap();
                fs::copy(&saved, &wal).unwrap();
                Ok(snapshot)
            });
            assert!(result.is_err(), "private={private}");
            drop(root);
            assert_eq!(fs::read(wal).unwrap(), bytes);
            assert_eq!(fs::read(saved).unwrap(), bytes);
        }
    }
}

#[test]
fn another_valid_same_inode_history_with_equal_final_rows_is_still_refused() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let f = restored(temp.path(), 1, compact);
        let alternate = temp.path().join("alternate");
        copy_directory(&f.root, &alternate);
        let (id, key) = &f.credentials[0];
        let relative = Path::new("registry").join(id).join("data/redo.wal");
        for (path, intermediate) in [(&f.root, 17), (&alternate, 42)] {
            let root = AccountRoot::open(path, pool()).unwrap();
            root.execute(
                id,
                key,
                "UPDATE t SET v=$1 WHERE id=1",
                &[Value::Integer(intermediate)],
            )
            .unwrap();
            root.execute(id, key, "UPDATE t SET v=0 WHERE id=1", &[])
                .unwrap();
        }
        let original = fs::read(f.root.join(&relative)).unwrap();
        let replacement = fs::read(alternate.join(&relative)).unwrap();
        assert_ne!(original, replacement);
        assert_eq!(original.len(), replacement.len());
        let before = emilybase_transactions::recover_image(&original, None).unwrap();
        let after = emilybase_transactions::recover_image(&replacement, None).unwrap();
        assert_eq!(before.database_id, after.database_id);
        assert_eq!(before.last_transaction, after.last_transaction);
        let rows = |image: &emilybase_transactions::RecoveredImage| {
            image
                .snapshot
                .primary_rows("t", None, None)
                .unwrap()
                .map(|row| row.cloned())
                .collect::<emilybase_database::Result<Vec<_>>>()
                .unwrap()
        };
        assert_eq!(rows(&before), rows(&after));
        let wal = f.root.join(relative);
        let inode = fs::metadata(&wal).unwrap().ino();
        let mut root = AccountRoot::open(&f.root, pool()).unwrap();
        let result = root.capture_with(|snapshot| {
            fs::write(&wal, &replacement).unwrap();
            Ok(snapshot)
        });
        assert!(result.is_err());
        assert_eq!(fs::metadata(&wal).unwrap().ino(), inode);
        assert_eq!(fs::read(wal).unwrap(), replacement);
    }
}

#[test]
fn unacknowledged_tails_added_under_original_owners_are_excluded_and_preserved() {
    use std::io::Write;
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for private in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let f = restored(temp.path(), 1, compact);
            let mut root = AccountRoot::open(&f.root, pool()).unwrap();
            let expected = root.capture().unwrap().into_bytes();
            let (id, key) = &f.credentials[0];
            let wal = if private {
                f.root.join("private").join(id).join("redo.wal")
            } else {
                f.root.join("registry").join(id).join("data/redo.wal")
            };
            let mut bytes = fs::read(&wal).unwrap();
            let snapshot = root
                .capture_with(|snapshot| {
                    let mut file = fs::OpenOptions::new().append(true).open(&wal).unwrap();
                    file.write_all(b"incomplete synthetic frame").unwrap();
                    file.sync_all().unwrap();
                    Ok(snapshot)
                })
                .unwrap();
            bytes.extend_from_slice(b"incomplete synthetic frame");
            assert_eq!(snapshot.bundle_bytes(), expected);
            assert_eq!(fs::read(&wal).unwrap(), bytes);
            drop(root);
            assert_eq!(fs::read(&wal).unwrap(), bytes);
            let copy = temp.path().join("copy");
            restore_account_bundle_bytes(snapshot.bundle_bytes(), &copy, pool(), 200).unwrap();
            assert_eq!(
                AccountRoot::open(copy, pool())
                    .unwrap()
                    .execute(id, key, "SELECT * FROM t", &[])
                    .unwrap()
                    .results[0]
                    .rows
                    .len(),
                1
            );
            assert_eq!(fs::read(wal).unwrap(), bytes);
        }
    }
}

#[test]
fn corrupt_public_mode_and_hardlinked_private_or_public_wals_refuse_without_repair() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for private in [false, true] {
            for kind in 0..3 {
                let temp = tempfile::tempdir().unwrap();
                let f = restored(temp.path(), 1, compact);
                let mut root = AccountRoot::open(&f.root, pool()).unwrap();
                let id = &f.credentials[0].0;
                let wal = if private {
                    f.root.join("private").join(id).join("redo.wal")
                } else {
                    f.root.join("registry").join(id).join("data/redo.wal")
                };
                let mut bytes = fs::read(&wal).unwrap();
                let alias = temp.path().join("alias");
                let result = root.capture_with(|snapshot| {
                    match kind {
                        0 => {
                            *bytes.last_mut().unwrap() ^= 1;
                            fs::write(&wal, &bytes).unwrap();
                        }
                        1 => fs::set_permissions(&wal, fs::Permissions::from_mode(0o644)).unwrap(),
                        _ => fs::hard_link(&wal, &alias).unwrap(),
                    };
                    Ok(snapshot)
                });
                assert!(result.is_err(), "private={private} kind={kind}");
                drop(root);
                assert_eq!(fs::read(wal).unwrap(), bytes);
                if kind == 2 {
                    assert!(alias.exists());
                }
            }
        }
    }
}

#[test]
fn structural_changes_during_capture_withhold_results_and_preserve_original_and_foreign_entries() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for relative in [false, true] {
        for kind in 0..9 {
            if kind == 8 && !relative {
                continue;
            }
            let temp = tempfile::tempdir().unwrap();
            let f = restored(temp.path(), 1, false);
            let parent = parent_at(temp.path());
            let mut root = if relative {
                AccountRoot::open_at(&parent, "restored", pool()).unwrap()
            } else {
                AccountRoot::open(&f.root, pool()).unwrap()
            };
            let id = &f.credentials[0].0;
            let saved = temp.path().join("saved");
            let result = root.capture_with(|snapshot| {
                match kind {
                    0 => {
                        fs::rename(&f.root, &saved).unwrap();
                        copy_directory(&saved, &f.root);
                    }
                    1..=4 => {
                        let child = match kind {
                            1 => f.root.join("registry"),
                            2 => f.root.join("private"),
                            3 => f.root.join("private").join(id),
                            _ => f.root.join("registry").join(id).join("data"),
                        };
                        fs::rename(&child, &saved).unwrap();
                        copy_directory(&saved, &child);
                    }
                    5 => {
                        let manifest = f.root.join("root.json");
                        fs::rename(&manifest, &saved).unwrap();
                        fs::copy(&saved, manifest).unwrap();
                    }
                    6 => fs::write(f.root.join("foreign"), b"preserved").unwrap(),
                    7 => fs::set_permissions(&f.root, fs::Permissions::from_mode(0o755)).unwrap(),
                    _ => {
                        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).unwrap()
                    }
                }
                Ok(snapshot)
            });
            assert!(result.is_err(), "relative={relative} kind={kind}");
            drop(root);
            if kind < 6 {
                assert!(saved.exists());
            }
            if kind == 6 {
                assert_eq!(fs::read(f.root.join("foreign")).unwrap(), b"preserved");
            }
            if kind == 8 {
                fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
    }
}

#[test]
fn moved_parent_keeps_selected_live_capture_source_and_never_captures_replacement_path() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let parent_path = temp.path().join("parent");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&parent_path)
        .unwrap();
    let f = restored(&parent_path, 1, true);
    let parent = parent_at(&parent_path);
    let mut root = AccountRoot::open_at(&parent, "restored", pool()).unwrap();
    drop(parent);
    let moved = temp.path().join("moved");
    fs::rename(&parent_path, &moved).unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&parent_path)
        .unwrap();
    copy_directory(&moved.join("restored"), &f.root);
    let (id, key) = &f.credentials[0];
    root.execute(id, key, "INSERT INTO t VALUES(2,17)", &[])
        .unwrap();
    let snapshot = root.capture().unwrap();
    assert_eq!(snapshot.report().registry.projects[0].rows, 2);
    drop(root);
    assert_eq!(
        AccountRoot::open(&f.root, pool())
            .unwrap()
            .capture()
            .unwrap()
            .report()
            .registry
            .projects[0]
            .rows,
        1
    );
}

#[test]
fn killed_live_capturers_preserve_exact_history_and_release_every_original_owner() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (1, false), (1, true)] {
        for point in [
            "root_capture_owners_locked",
            "root_capture_copied",
            "root_capture_coordinated",
            "root_capture_ack",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let f = restored(temp.path(), count, compact);
            let parent = parent_at(temp.path());
            let before = histories(&f);
            let worker = Worker::start(
                &f.root,
                &temp.path().join("unused"),
                "",
                "root-capture",
                point,
            );
            worker.reach(point);
            assert!(File::open(&f.root).unwrap().try_lock().is_err());
            if point != "root_capture_ack" {
                for (id, _) in &f.credentials {
                    assert!(Database::open(f.root.join("registry").join(id).join("data")).is_err());
                }
            }
            worker.kill();
            assert_eq!(histories(&f), before);
            let mut root = AccountRoot::open_at(&parent, "restored", pool()).unwrap();
            let snapshot = root.capture().unwrap();
            assert_eq!(snapshot.report().private_accounts.len(), count);
            if let Some((id, key)) = f.credentials.first() {
                let pair = root.sign_in(id, key, LOGIN, PASSWORD, 100).unwrap();
                root.with_access(id, key, pair.access.expose(), 100, |_| ())
                    .unwrap();
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_live_mutation_and_capture_match_independent_rows_across_fresh_restore(compact in any::<bool>(), values in prop::collection::vec(-100i64..101,1..9)) {
        let _serial=durability::PROCESS_TESTS.blocking_lock();let temp=tempfile::tempdir().unwrap();let f=restored(temp.path(),1,compact);let (id,key)=&f.credentials[0];let parent=parent_at(temp.path());let mut root=AccountRoot::open_at(&parent,"restored",pool()).unwrap();let mut expected=vec![vec![Value::Integer(1),Value::Integer(0)]];
        for (step,value) in values.into_iter().enumerate() {let key_value=step as i64+2;root.execute(id,key,"INSERT INTO t VALUES($1,$2)",&[Value::Integer(key_value),Value::Integer(value)]).unwrap();expected.push(vec![Value::Integer(key_value),Value::Integer(value)]);let before=histories(&f);let snapshot=root.capture().unwrap();prop_assert_eq!(histories(&f),before);prop_assert_eq!(snapshot.report().registry.projects[0].rows,expected.len());let copy=temp.path().join(format!("copy-{step}"));restore_account_bundle_bytes(snapshot.bundle_bytes(),&copy,pool(),200).unwrap();let clone=AccountRoot::open(&copy,pool()).unwrap();prop_assert_eq!(&clone.execute(id,key,"SELECT id,v FROM t ORDER BY id",&[]).unwrap().results[0].rows,&expected);}
    }
}
