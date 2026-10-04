use super::{image, seed};
use crate::durability::{PROCESS_TESTS, on_boundary};
use crate::{Error, ProjectStore, restore_registry_backup};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn staging(parent: &Path, prefix: &str) -> std::ffi::OsString {
    fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .find(|name| name.to_string_lossy().starts_with(prefix))
        .unwrap()
}

#[test]
fn registry_backup_parent_substitution_is_rejected_without_selecting_a_foreign_inode() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    seed(&source);
    let expected = image(&source);
    let parent = temporary.path().join("outputs");
    fs::create_dir(&parent).unwrap();
    let moved = temporary.path().join("moved");
    let replacement = expected.clone();
    let callback_parent = parent.clone();
    let callback_moved = moved.clone();
    let action = on_boundary("registry_backup_file_synced", move || {
        let name = staging(&callback_parent, ".emilybase-registry-backup-");
        fs::rename(&callback_parent, &callback_moved).unwrap();
        fs::create_dir(&callback_parent).unwrap();
        let foreign = callback_parent.join(name);
        fs::write(&foreign, replacement).unwrap();
        fs::set_permissions(foreign, fs::Permissions::from_mode(0o600)).unwrap();
    });
    let target = parent.join("selected.backup");
    let outcome = ProjectStore::open_existing(&source)
        .unwrap()
        .backup(&target);
    drop(action);
    assert!(matches!(outcome, Err(Error::Path)));
    assert!(!target.exists());
    assert_eq!(image(&source), expected);
    let name = staging(&parent, ".emilybase-registry-backup-");
    assert_eq!(fs::read(parent.join(name)).unwrap(), expected);
}

#[test]
fn registry_restore_parent_substitution_cannot_publish_a_foreign_directory() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    seed(&source);
    let expected = image(&source);
    let archive = temporary.path().join("source.backup");
    ProjectStore::open_existing(&source)
        .unwrap()
        .backup(&archive)
        .unwrap();
    let parent = temporary.path().join("outputs");
    let moved = temporary.path().join("moved");
    fs::create_dir(&parent).unwrap();
    let callback_parent = parent.clone();
    let callback_moved = moved.clone();
    let action = on_boundary("registry_restore_stage_synced", move || {
        let name = staging(&callback_parent, ".emilybase-registry-restore-");
        fs::rename(&callback_parent, &callback_moved).unwrap();
        fs::create_dir(&callback_parent).unwrap();
        let foreign = callback_parent.join(name);
        fs::create_dir(&foreign).unwrap();
        fs::write(
            foreign.join("keep"),
            b"foreign synthetic registry directory",
        )
        .unwrap();
    });
    let target = parent.join("selected");
    let outcome = restore_registry_backup(&archive, &target);
    drop(action);
    assert!(matches!(outcome, Err(Error::Path)));
    assert!(!target.exists());
    assert_eq!(image(&source), expected);
    assert_eq!(fs::read(&archive).unwrap(), expected);
    let name = staging(&parent, ".emilybase-registry-restore-");
    assert_eq!(
        fs::read(parent.join(name).join("keep")).unwrap(),
        b"foreign synthetic registry directory"
    );
}

#[test]
fn detached_registry_staging_preserves_both_original_and_foreign_entries() {
    let _serial = PROCESS_TESTS.blocking_lock();
    for restoring in [false, true] {
        for symbolic in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let source = temporary.path().join("source");
            let credentials = seed(&source);
            let expected = image(&source);
            let archive = temporary.path().join("source.backup");
            ProjectStore::open_existing(&source)
                .unwrap()
                .backup(&archive)
                .unwrap();
            let parent = temporary.path().join("outputs");
            fs::create_dir(&parent).unwrap();
            let detached = temporary.path().join("detached");
            let protected = temporary.path().join("protected");
            if restoring {
                fs::create_dir(&protected).unwrap();
                fs::write(protected.join("keep"), b"foreign registry directory").unwrap();
            } else {
                fs::copy(&archive, &protected).unwrap();
            }
            let (callback_parent, callback_detached, callback_protected) =
                (parent.clone(), detached.clone(), protected.clone());
            let phase = if restoring {
                "registry_restore_stage_synced"
            } else {
                "registry_backup_file_synced"
            };
            let action = on_boundary(phase, move || {
                let prefix = if restoring {
                    ".emilybase-registry-restore-"
                } else {
                    ".emilybase-registry-backup-"
                };
                let name = staging(&callback_parent, prefix);
                fs::rename(callback_parent.join(&name), &callback_detached).unwrap();
                let foreign = callback_parent.join(name);
                if symbolic {
                    std::os::unix::fs::symlink(&callback_protected, &foreign).unwrap();
                } else if restoring {
                    fs::create_dir(&foreign).unwrap();
                    fs::write(foreign.join("keep"), b"foreign registry directory").unwrap();
                } else {
                    fs::copy(&callback_protected, &foreign).unwrap();
                }
            });
            let target = parent.join("selected");
            let result = if restoring {
                restore_registry_backup(&archive, &target)
            } else {
                ProjectStore::open_existing(&source)
                    .unwrap()
                    .backup(&target)
            };
            drop(action);
            assert!(matches!(result, Err(Error::Path)));
            assert!(!target.exists());
            let foreign = parent.join(staging(
                &parent,
                if restoring {
                    ".emilybase-registry-restore-"
                } else {
                    ".emilybase-registry-backup-"
                },
            ));
            if symbolic {
                assert!(
                    fs::symlink_metadata(&foreign)
                        .unwrap()
                        .file_type()
                        .is_symlink()
                );
            }
            if restoring {
                assert_eq!(
                    fs::read(foreign.join("keep")).unwrap(),
                    b"foreign registry directory"
                );
                super::usable(&detached, &credentials, &expected);
            } else {
                assert_eq!(fs::read(&foreign).unwrap(), expected);
                assert_eq!(fs::read(&detached).unwrap(), expected);
            }
            assert_eq!(image(&source), expected);
            assert_eq!(fs::read(&archive).unwrap(), expected);
        }
    }
}

#[test]
fn registry_post_rename_changes_preserve_selection_and_report_unknown() {
    let _serial = PROCESS_TESTS.blocking_lock();
    for restoring in [false, true] {
        for replace_parent in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let source = temporary.path().join("source");
            let credentials = seed(&source);
            let expected = image(&source);
            let archive = temporary.path().join("source.backup");
            ProjectStore::open_existing(&source)
                .unwrap()
                .backup(&archive)
                .unwrap();
            let parent = temporary.path().join("outputs");
            fs::create_dir(&parent).unwrap();
            let target = parent.join("selected");
            let detached = temporary.path().join("detached");
            let callback_parent = parent.clone();
            let callback_target = target.clone();
            let callback_detached = detached.clone();
            let action = on_boundary(
                if restoring {
                    "registry_restore_renamed"
                } else {
                    "registry_backup_renamed"
                },
                move || {
                    if replace_parent {
                        fs::rename(&callback_parent, &callback_detached).unwrap();
                        fs::create_dir(&callback_parent).unwrap();
                        fs::write(callback_parent.join("keep"), b"foreign parent").unwrap();
                    } else {
                        fs::rename(&callback_target, &callback_detached).unwrap();
                        if restoring {
                            fs::create_dir(&callback_target).unwrap();
                            fs::write(callback_target.join("keep"), b"foreign selection").unwrap();
                        } else {
                            fs::write(&callback_target, b"foreign selection").unwrap();
                        }
                    }
                },
            );
            let outcome = if restoring {
                restore_registry_backup(&archive, &target)
            } else {
                ProjectStore::open_existing(&source)
                    .unwrap()
                    .backup(&target)
            };
            drop(action);
            assert!(matches!(outcome, Err(Error::PublicationUnknown(_))));
            let original = if replace_parent {
                detached.join("selected")
            } else {
                detached
            };
            if restoring {
                super::usable(&original, &credentials, &expected);
            } else {
                assert_eq!(fs::read(original).unwrap(), expected);
            }
            if replace_parent {
                assert_eq!(fs::read(parent.join("keep")).unwrap(), b"foreign parent");
                assert!(!target.exists());
            } else if restoring {
                assert_eq!(fs::read(target.join("keep")).unwrap(), b"foreign selection");
            } else {
                assert_eq!(fs::read(&target).unwrap(), b"foreign selection");
            }
            assert_eq!(image(&source), expected);
            assert_eq!(fs::read(&archive).unwrap(), expected);
        }
    }
}

#[test]
fn registry_initial_parent_aliases_and_invalid_destinations_cannot_stage() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    seed(&source);
    let expected = image(&source);
    let archive = temporary.path().join("source.backup");
    ProjectStore::open_existing(&source)
        .unwrap()
        .backup(&archive)
        .unwrap();
    let parent = temporary.path().join("outputs");
    fs::create_dir(&parent).unwrap();
    let alias = temporary.path().join("alias");
    std::os::unix::fs::symlink(&parent, &alias).unwrap();
    for target in [
        alias.join("selected"),
        temporary.path().join("missing").join("selected"),
        Path::new("/").to_path_buf(),
        Path::new(".").to_path_buf(),
    ] {
        assert!(
            ProjectStore::open_existing(&source)
                .unwrap()
                .backup(&target)
                .is_err()
        );
        assert!(restore_registry_backup(&archive, target).is_err());
    }
    assert_eq!(fs::read_dir(parent).unwrap().count(), 0);
    assert_eq!(image(&source), expected);
    assert_eq!(fs::read(archive).unwrap(), expected);
}

#[test]
fn ancestor_replacement_during_restore_wal_write_never_redirects_later_projects() {
    let _serial = PROCESS_TESTS.blocking_lock();
    for restoring in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("source");
        seed(&source);
        let expected = image(&source);
        let archive = temporary.path().join("source.backup");
        ProjectStore::open_existing(&source)
            .unwrap()
            .backup(&archive)
            .unwrap();
        let ancestor = temporary.path().join("ancestor");
        let parent = ancestor.join("outputs");
        fs::create_dir_all(&parent).unwrap();
        let moved = temporary.path().join("moved-ancestor");
        let callback_ancestor = ancestor.clone();
        let callback_parent = parent.clone();
        let callback_moved = moved.clone();
        let bytes = expected.clone();
        let phase = if restoring {
            "registry_restore_wal_synced"
        } else {
            "registry_backup_file_synced"
        };
        let action = on_boundary(phase, move || {
            let prefix = if restoring {
                ".emilybase-registry-restore-"
            } else {
                ".emilybase-registry-backup-"
            };
            let name = staging(&callback_parent, prefix);
            fs::rename(&callback_ancestor, &callback_moved).unwrap();
            fs::create_dir_all(&callback_parent).unwrap();
            let foreign = callback_parent.join(name);
            if restoring {
                fs::create_dir(&foreign).unwrap();
                fs::write(foreign.join("keep"), b"foreign ancestor subtree").unwrap();
            } else {
                fs::write(&foreign, bytes).unwrap();
                fs::set_permissions(&foreign, fs::Permissions::from_mode(0o600)).unwrap();
            }
        });
        let target = parent.join("selected");
        let outcome = if restoring {
            restore_registry_backup(&archive, &target)
        } else {
            ProjectStore::open_existing(&source)
                .unwrap()
                .backup(&target)
        };
        drop(action);
        assert!(matches!(outcome, Err(Error::Path)));
        assert!(!target.exists());
        assert_eq!(fs::read_dir(moved.join("outputs")).unwrap().count(), 0);
        let foreign = parent.join(staging(
            &parent,
            if restoring {
                ".emilybase-registry-restore-"
            } else {
                ".emilybase-registry-backup-"
            },
        ));
        if restoring {
            assert_eq!(fs::read_dir(&foreign).unwrap().count(), 1);
            assert_eq!(
                fs::read(foreign.join("keep")).unwrap(),
                b"foreign ancestor subtree"
            );
        } else {
            assert_eq!(fs::read(foreign).unwrap(), expected);
        }
        assert_eq!(image(&source), expected);
        assert_eq!(fs::read(&archive).unwrap(), expected);
    }
}

#[test]
fn native_registry_workers_reject_pre_and_post_publication_substitution() {
    let _serial = PROCESS_TESTS.blocking_lock();
    for restoring in [false, true] {
        for selected in [false, true] {
            for replace_parent in [false, true] {
                let temporary = tempfile::tempdir().unwrap();
                let source = temporary.path().join("source");
                let credentials = seed(&source);
                let expected = image(&source);
                let archive = temporary.path().join("source.backup");
                ProjectStore::open_existing(&source)
                    .unwrap()
                    .backup(&archive)
                    .unwrap();
                let parent = temporary.path().join("outputs");
                fs::create_dir(&parent).unwrap();
                let output = parent.join("selected");
                let point = match (restoring, selected) {
                    (false, false) => "registry_backup_file_synced",
                    (false, true) => "registry_backup_renamed",
                    (true, false) => "registry_restore_stage_synced",
                    (true, true) => "registry_restore_renamed",
                };
                let worker_archive = if restoring { &archive } else { &output };
                let mut worker = super::Worker::start(
                    &source,
                    worker_archive,
                    &output,
                    if restoring {
                        "owned-restore"
                    } else {
                        "owned-backup"
                    },
                    point,
                );
                worker.reach(point);
                let name = if selected {
                    std::ffi::OsString::from("selected")
                } else {
                    staging(
                        &parent,
                        if restoring {
                            ".emilybase-registry-restore-"
                        } else {
                            ".emilybase-registry-backup-"
                        },
                    )
                };
                let detached = temporary.path().join("detached");
                if replace_parent {
                    fs::rename(&parent, &detached).unwrap();
                    fs::create_dir(&parent).unwrap();
                } else {
                    fs::rename(parent.join(&name), &detached).unwrap();
                }
                let foreign = parent.join(&name);
                if restoring {
                    fs::create_dir(&foreign).unwrap();
                    fs::write(foreign.join("keep"), b"foreign native registry directory").unwrap();
                } else {
                    fs::copy(&archive, &foreign).unwrap();
                }
                worker.release();
                assert!(worker.finish().contains("REGISTRY_REFUSED"));
                let original = if replace_parent {
                    detached.join(name)
                } else {
                    detached.clone()
                };
                if selected || !replace_parent {
                    if restoring {
                        super::usable(&original, &credentials, &expected);
                    } else {
                        assert_eq!(fs::read(original).unwrap(), expected);
                    }
                } else {
                    assert_eq!(fs::read_dir(detached).unwrap().count(), 0);
                }
                if restoring {
                    assert_eq!(
                        fs::read(foreign.join("keep")).unwrap(),
                        b"foreign native registry directory"
                    );
                } else {
                    assert_eq!(fs::read(foreign).unwrap(), expected);
                }
                if !selected {
                    assert!(!output.exists());
                }
                assert_eq!(image(&source), expected);
                assert_eq!(fs::read(&archive).unwrap(), expected);
            }
        }
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(32))]
    #[test]
    fn generated_registry_failures_preserve_epochs_scopes_and_independent_rows(
        restoring in proptest::bool::ANY,
        after_sync in proptest::bool::ANY,
        values in proptest::collection::vec((-1000i64..1000, 0u8..3), 0..4),
        phase_number in 0usize..3,
    ) {
        use crate::durability::inject;
        use emilybase_catalog::Value;
        let _serial = PROCESS_TESTS.blocking_lock();
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("source");
        let mut store = ProjectStore::open(&source).unwrap();
        let mut model = Vec::new();
        for (i, (value, rotations)) in values.iter().enumerate() {
            let created = store.create("synthetic fault model").unwrap();
            let id = created.project.id;
            let original = created.api_key;
            let mut current = original.clone();
            store.authorize(&id, &current).unwrap().execute("CREATE TABLE t(id INT PRIMARY KEY,n INT);INSERT INTO t VALUES(1,$1)", &[Value::Integer(*value)]).unwrap();
            for _ in 0..*rotations { current = store.rotate(&id).unwrap().api_key; }
            if i % 2 == 0 {
                emilybase_transactions::Database::open(source.join(&id).join("data")).unwrap().compact().unwrap();
            }
            model.push((id, current, original, u64::from(*rotations) + 1, *value));
        }
        let expected = store.backup_image().unwrap();
        let archive = temporary.path().join("source.backup");
        store.backup(&archive).unwrap();
        drop(store);
        let phases = if restoring {
            [("registry_restore_wal_sync", "registry_restore_wal_sync_after"),
             ("registry_restore_stage_sync", "registry_restore_stage_sync_after"),
             ("registry_restore_parent_sync", "registry_restore_parent_sync_after")]
        } else {
            [("registry_backup_file_sync", "registry_backup_file_sync_after"),
             ("registry_backup_file_sync", "registry_backup_file_sync_after"),
             ("registry_backup_parent_sync", "registry_backup_parent_sync_after")]
        };
        // Empty registries perform no WAL write; choose an actually reached phase.
        let position = if model.is_empty() && restoring && phase_number == 0 { 1 } else { phase_number };
        let (phase, after) = phases[position];
        inject(if after_sync { after } else { phase });
        let target = temporary.path().join("selected");
        let result = if restoring { restore_registry_backup(&archive, &target) } else { ProjectStore::open_existing(&source).unwrap().backup(&target) };
        proptest::prop_assert!(result.is_err());
        proptest::prop_assert_eq!(target.exists(), phase.ends_with("parent_sync"));
        proptest::prop_assert_eq!(image(&source), expected.clone());
        proptest::prop_assert_eq!(fs::read(&archive).unwrap(), expected.clone());
        let restored_path = temporary.path().join("verified");
        restore_registry_backup(&archive, &restored_path).unwrap();
        let mut restored = ProjectStore::open_existing(&restored_path).unwrap();
        proptest::prop_assert_eq!(restored.backup_image().unwrap(), expected);
        proptest::prop_assert_eq!(restored.list().unwrap().len(), model.len());
        for (id, key, old, epoch, value) in &model {
            proptest::prop_assert_eq!(restored.list().unwrap().iter().find(|p| &p.id == id).unwrap().key_epoch, *epoch);
            if epoch > &1 { proptest::prop_assert!(restored.authorize(id, old).is_err()); }
            for (other, wrong, _, _, _) in &model { if other != id { proptest::prop_assert!(restored.authorize(id, wrong).is_err()); } }
            let rows = restored.authorize(id, key).unwrap().execute("SELECT id,n FROM t", &[]).unwrap();
            proptest::prop_assert_eq!(&rows.results[0].rows, &vec![vec![Value::Integer(1), Value::Integer(*value)]]);
            let replacement = restored.rotate(id).unwrap().api_key;
            proptest::prop_assert!(restored.authorize(id, key).is_err());
            restored.authorize(id, &replacement).unwrap().execute("INSERT INTO t VALUES(2,99)", &[]).unwrap();
        }
        drop(restored);
        proptest::prop_assert_eq!(image(&source), fs::read(archive).unwrap());
        let restored = ProjectStore::open_existing(restored_path).unwrap();
        for (id, _, _, epoch, _) in model {
            proptest::prop_assert_eq!(restored.list().unwrap().iter().find(|p| p.id == id).unwrap().key_epoch, epoch + 1);
        }
    }
}

pub(super) fn verify_descriptor_cycles(source: &Path, archive: &Path, output_root: &Path) {
    // This runs in its own child, so other test threads cannot alter the FD count.
    let descriptor_count = || fs::read_dir("/proc/self/fd").unwrap().count();
    let expected = image(source);
    let initial = descriptor_count();
    for number in 0..64 {
        let parent = output_root.join(format!("case-{number}"));
        fs::create_dir(&parent).unwrap();
        let target = parent.join("selected");
        let restoring = number & 1 != 0;
        let after = number & 2 != 0;
        let phase = match (restoring, after) {
            (false, false) => "registry_backup_file_synced",
            (false, true) => "registry_backup_renamed",
            (true, false) => "registry_restore_stage_synced",
            (true, true) => "registry_restore_renamed",
        };
        let callback_parent = parent.clone();
        let moved = output_root.join(format!("moved-{number}"));
        let action = on_boundary(phase, move || {
            fs::rename(&callback_parent, moved).unwrap();
            fs::create_dir(&callback_parent).unwrap();
        });
        let result = if restoring {
            restore_registry_backup(archive, &target)
        } else {
            ProjectStore::open_existing(source).unwrap().backup(&target)
        };
        drop(action);
        if after {
            assert!(matches!(result, Err(Error::PublicationUnknown(_))));
        } else {
            assert!(matches!(result, Err(Error::Path)));
        }
        assert_eq!(
            descriptor_count(),
            initial,
            "descriptor leak in cycle {number}"
        );
        assert_eq!(image(source), expected);
        assert_eq!(fs::read(archive).unwrap(), expected);
    }
    println!("REGISTRY_FD_CYCLES_VERIFIED");
}

#[test]
fn repeated_refusal_and_uncertainty_release_all_owned_descriptors() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    seed(&source);
    let archive = temporary.path().join("source.backup");
    ProjectStore::open_existing(&source)
        .unwrap()
        .backup(&archive)
        .unwrap();
    let output = temporary.path().join("cycles");
    fs::create_dir(&output).unwrap();
    let worker = super::Worker::start(&source, &archive, &output, "descriptor-cycles", "");
    assert!(worker.finish().contains("REGISTRY_FD_CYCLES_VERIFIED"));
}
