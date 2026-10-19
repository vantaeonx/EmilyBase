use super::*;
use crate::{backup_account_file_root_v2, inspect_account_file_archive};
use std::os::unix::fs::OpenOptionsExt;

fn private_write(path: &Path, bytes: &[u8]) {
    File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

fn backup(path: &Path, target: &Path) -> crate::Result<crate::AccountFileArchiveReport> {
    let mut root = AccountRoot::open_v2(path, pool())?;
    backup_account_file_root_v2(&mut root, target)
}
#[test]
fn v2_backup_copies_every_binding_preserves_current_sessions_and_restores_independent_sources() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (1, false), (4, true)] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), count, compact);
        let target = path.parent().unwrap().join("archive");
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let mut sessions = Vec::new();
        for (id, key) in &s.root.credentials {
            sessions.push(root.sign_in(id, key, LOGIN, PASSWORD, 101).unwrap());
        }
        let before = copied_history(&s, &path);
        let snapshot = root.capture_v2().unwrap();
        assert_eq!(snapshot.manifest_bytes(), before[0]);
        assert_eq!(snapshot.files().len(), count);
        let expected = encode(&snapshot);
        let report = backup_account_file_root_v2(&mut root, &target).unwrap();
        assert_eq!(report, inspect_account_file_archive(&target).unwrap());
        assert_eq!(fs::read(&target).unwrap(), expected);
        assert_eq!(report.manifest(), snapshot.manifest());
        assert_eq!(copied_history(&s, &path), before);
        for ((id, key), token) in s.root.credentials.iter().zip(&sessions) {
            root.with_access(id, key, token.access.expose(), 101, |_| ())
                .unwrap();
        }
        drop(root);
        let second = path.parent().unwrap().join("second");
        let restored = crate::restore_account_file_archive(&target, &second, pool(), 40).unwrap();
        verify_copy(&s, &second, &restored);
        let mut second_root = AccountRoot::open_v2(&second, pool()).unwrap();
        for ((id, key), token) in s.root.credentials.iter().zip(&sessions) {
            assert!(
                second_root
                    .with_access(id, key, token.access.expose(), 101, |_| ())
                    .is_err()
            );
        }
        drop(second_root);
        assert_eq!(copied_history(&s, &path), before);
        verify_source(&s);
    }
}
#[test]
fn actual_internal_owners_payload_readers_and_selected_archive_span_v2_capture_proofs() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for point in [
        "joined_archive_selected",
        "root_capture_coordinated",
        "joined_backup_confirming",
    ] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, true);
        let target = path.parent().unwrap().join("archive");
        let original = path.clone();
        let selected = target.clone();
        let id = s.root.credentials[0].0.clone();
        let _hook = durability::on_boundary(point, move || {
            assert!(File::open(&original).unwrap().try_lock().is_err());
            assert!(AccountStore::open(original.join("private").join(&id), &id, pool()).is_err());
            assert!(
                NativeFileRoot::open(original.join("files").join(&id), id.parse().unwrap())
                    .is_err()
            );
            assert_eq!(
                Database::open(original.join("registry").join(&id).join("data")).is_err(),
                point != "joined_backup_confirming"
            );
            assert!(fds(std::process::id(), &selected) > 0);
            for orphan in [false, true] {
                assert_eq!(
                    fds(
                        std::process::id(),
                        &object(&original.join("files").join(&id), orphan)
                    ) > 0,
                    point != "joined_backup_confirming"
                );
            }
        });
        backup(&path, &target).unwrap();
        verify_source(&s);
    }
}
#[test]
fn v2_private_unacknowledged_tail_is_not_a_successful_backup_receipt() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, false);
    let target = path.parent().unwrap().join("archive");
    let changed = path
        .join("private")
        .join(&s.root.credentials[0].0)
        .join("redo.wal");
    let original = fs::read(&changed).unwrap();
    let external = changed.clone();
    let _hook = durability::on_boundary("joined_archive_selected", move || {
        File::options()
            .append(true)
            .open(external)
            .unwrap()
            .write_all(b"unacknowledged tail")
            .unwrap();
    });
    assert!(matches!(
        backup(&path, &target),
        Err(Error::ComponentPublicationUnknown(_))
    ));
    let mut expected = original;
    expected.extend_from_slice(b"unacknowledged tail");
    assert_eq!(fs::read(changed).unwrap(), expected);
    inspect_account_file_archive(target).unwrap();
    verify_source(&s);
}
#[test]
fn v1_coordinated_backup_also_refuses_private_tail_changes_after_archive_selection() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let s = source(t.path(), 1, true);
    let target = output(t.path()).join("archive");
    let changed = s
        .root
        .root
        .join("private")
        .join(&s.root.credentials[0].0)
        .join("redo.wal");
    let original = fs::read(&changed).unwrap();
    let external = changed.clone();
    let _hook = durability::on_boundary("joined_archive_selected", move || {
        File::options()
            .append(true)
            .open(external)
            .unwrap()
            .write_all(b"external tail")
            .unwrap();
    });
    let mut root = AccountRoot::open(&s.root.root, pool()).unwrap();
    let mut files = open(&s.paths);
    assert!(matches!(
        backup_account_file_components(&mut root, &mut files, &target),
        Err(Error::ComponentPublicationUnknown(_))
    ));
    let mut expected = original;
    expected.extend_from_slice(b"external tail");
    assert_eq!(fs::read(changed).unwrap(), expected);
    inspect_account_file_archive(target).unwrap();
}

#[test]
fn v2_source_changes_after_selection_preserve_external_bytes_and_withhold_the_receipt() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for point in ["joined_archive_selected", "root_capture_coordinated"] {
            for kind in 0..5 {
                let t = tempfile::tempdir().unwrap();
                let (s, path) = copy(t.path(), 1, compact);
                let target = path.parent().unwrap().join("archive");
                let id = &s.root.credentials[0].0;
                let native = path.join("files").join(id);
                let changed = match kind {
                    0 => object(&native, true),
                    1 => native.join("metadata/redo.wal"),
                    2 => path.join("private").join(id).join("redo.wal"),
                    3 => path.join("root.json"),
                    _ => path.join("registry").join(id).join("data/redo.wal"),
                };
                let original = fs::read(&changed).unwrap();
                let external = changed.clone();
                let held = t.path().join("original");
                let saved = held.clone();
                let _hook = durability::on_boundary(point, move || {
                    if kind == 3 {
                        fs::rename(&external, &saved).unwrap();
                        private_write(&external, &fs::read(saved).unwrap());
                    } else {
                        File::options()
                            .append(true)
                            .open(external)
                            .unwrap()
                            .write_all(b"external tail")
                            .unwrap();
                    }
                });
                assert!(
                    matches!(
                        backup(&path, &target),
                        Err(Error::ComponentPublicationUnknown(_))
                    ),
                    "{point} {kind}"
                );
                if kind == 3 {
                    assert_eq!(fs::read(held).unwrap(), original);
                    assert!(changed.exists());
                } else {
                    let mut expected = original;
                    expected.extend_from_slice(b"external tail");
                    assert_eq!(fs::read(changed).unwrap(), expected);
                }
                inspect_account_file_archive(target).unwrap();
                verify_source(&s);
            }
        }
    }
}
#[test]
fn contained_and_existing_v2_outputs_refuse_before_publication_and_sources_remain_usable() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, false);
    let before = copied_history(&s, &path);
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    let id = &s.root.credentials[0].0;
    for directory in [
        path.clone(),
        path.join("private"),
        path.join("private").join(id),
        path.join("registry").join(id).join("data"),
        path.join("files"),
        path.join("files").join(id),
        path.join("files").join(id).join("objects"),
    ] {
        let count = fs::read_dir(&directory).unwrap().count();
        assert!(backup_account_file_root_v2(&mut root, directory.join("archive")).is_err());
        assert_eq!(fs::read_dir(directory).unwrap().count(), count);
        assert_eq!(root.capture_v2().unwrap().files().len(), 1);
    }
    let existing = path.parent().unwrap().join("existing");
    private_write(&existing, b"existing");
    assert!(backup_account_file_root_v2(&mut root, &existing).is_err());
    assert_eq!(fs::read(existing).unwrap(), b"existing");
    drop(root);
    assert_eq!(copied_history(&s, &path), before);
    let mut v1 = AccountRoot::open(&s.root.root, pool()).unwrap();
    let absent = path.parent().unwrap().join("absent");
    assert!(backup_account_file_root_v2(&mut v1, &absent).is_err());
    assert!(v1.capture_v2().is_err());
    assert!(!absent.exists());
}
#[test]
fn actual_kills_at_v2_capture_publication_and_caller_return_preserve_sources_and_selected_archives()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (1, true)] {
        for point in [
            "components_retained",
            "joined_archive_prepared",
            "joined_archive_copying",
            "joined_archive_selected",
            "root_capture_coordinated",
            "joined_backup_confirming",
            "joined_v2_backup_returned",
        ] {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), count, compact);
            let before = copied_history(&s, &path);
            let expected = encode(
                &AccountRoot::open_v2(&path, pool())
                    .unwrap()
                    .capture_v2()
                    .unwrap(),
            );
            let target = path.parent().unwrap().join("archive");
            let worker = Worker::start(&path, &target, "", "root-v2-backup", point);
            worker.reach(point);
            assert!(File::open(&path).unwrap().try_lock().is_err());
            let selected = !matches!(
                point,
                "components_retained" | "joined_archive_prepared" | "joined_archive_copying"
            );
            assert_eq!(target.exists(), selected);
            if selected {
                assert_eq!(fs::read(&target).unwrap(), expected);
                assert_eq!(
                    fds(worker.pid(), &target) > 0,
                    point != "joined_v2_backup_returned"
                );
            }
            for (id, _) in &s.root.credentials {
                assert!(AccountStore::open(path.join("private").join(id), id, pool()).is_err());
                assert!(
                    NativeFileRoot::open(path.join("files").join(id), id.parse().unwrap()).is_err()
                );
                for orphan in [false, true] {
                    assert_eq!(
                        fds(worker.pid(), &object(&path.join("files").join(id), orphan)) > 0,
                        !matches!(
                            point,
                            "joined_backup_confirming" | "joined_v2_backup_returned"
                        )
                    );
                }
            }
            worker.kill();
            assert_eq!(copied_history(&s, &path), before);
            backup(&path, &path.parent().unwrap().join("retry")).unwrap();
            assert_eq!(copied_history(&s, &path), before);
            verify_source(&s);
        }
    }
}
#[test]
fn v2_late_archive_replacement_and_sync_failures_preserve_selected_copies_without_acknowledgement()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for kind in 0..3 {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, true);
        let target = path.parent().unwrap().join("archive");
        let selected = target.clone();
        let held = path.parent().unwrap().join("held");
        let original = held.clone();
        let _hook = durability::on_boundary("joined_backup_confirming", move || match kind {
            0 => {
                fs::rename(&selected, &original).unwrap();
                private_write(&selected, &fs::read(original).unwrap());
            }
            1 => {
                File::options()
                    .append(true)
                    .open(selected)
                    .unwrap()
                    .write_all(b"tail")
                    .unwrap();
            }
            _ => fs::set_permissions(selected, fs::Permissions::from_mode(0o644)).unwrap(),
        });
        assert!(matches!(
            backup(&path, &target),
            Err(Error::ComponentPublicationUnknown(_))
        ));
        assert!(target.exists());
        if kind == 0 {
            assert_eq!(fs::read(&held).unwrap(), fs::read(&target).unwrap());
        }
        verify_source(&s);
    }
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, false);
    for point in [
        "joined_archive_file_ack_sync",
        "joined_archive_file_ack_sync_after",
        "joined_archive_parent_ack_sync",
        "joined_archive_parent_ack_sync_after",
    ] {
        let target = path.parent().unwrap().join(point);
        durability::inject(point);
        assert!(matches!(
            backup(&path, &target),
            Err(Error::ComponentPublicationUnknown(_))
        ));
        inspect_account_file_archive(target).unwrap();
        verify_source(&s);
    }
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_v2_rebackup_keeps_exact_bindings_and_histories(count in 0usize..3, compact in any::<bool>()) {
        let _serial = durability::PROCESS_TESTS.blocking_lock();
        let t = tempfile::tempdir().unwrap(); let (s,path) = copy(t.path(),count,compact);
        let before = copied_history(&s,&path); let target=path.parent().unwrap().join("archive");
        let report=backup(&path,&target).unwrap();
        prop_assert_eq!(report.manifest().to_bytes().unwrap(),before[0].clone());
        prop_assert_eq!(report.manifest().file_stores().len(),count);
        prop_assert_eq!(copied_history(&s,&path),before);
        verify_source(&s);
    }
}

#[test]
fn v2_aggregate_object_budget_is_admitted_before_retaining_capture_readers_or_staging_output() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 2, false);
    for (id, _) in &s.root.credentials {
        let mut objects = ProjectDirectory::open(
            path.join("files").join(id).join("objects"),
            id.parse().unwrap(),
        )
        .unwrap();
        for n in 10..73 {
            objects
                .put(ObjectId::from_bytes([n; 16]), b"synthetic orphan")
                .unwrap();
        }
    }
    let before = copied_history(&s, &path);
    let target = path.parent().unwrap().join("archive");
    let _hook = durability::on_boundary("components_admitted", || {
        panic!("over-budget components admitted")
    });
    assert!(matches!(backup(&path, &target), Err(Error::Limit)));
    assert!(!target.exists());
    assert_eq!(copied_history(&s, &path), before);
    verify_source(&s);
}
#[test]
fn v2_backup_keeps_original_moved_parent_and_never_adopts_the_replacement_at_its_old_path() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let work = t.path().join("work");
    fs::DirBuilder::new().mode(0o700).create(&work).unwrap();
    let (s, path) = copy(&work, 1, true);
    let before = copied_history(&s, &path);
    let parent = path.parent().unwrap().to_path_buf();
    let mut root = AccountRoot::open_v2_at(&File::open(&parent).unwrap(), "v2", pool()).unwrap();
    let target = t.path().join("archive");
    fs::set_permissions(t.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let moved = t.path().join("moved");
    let original = moved.clone();
    let old = parent.clone();
    let _hook = durability::on_boundary("joined_archive_selected", move || {
        fs::rename(&old, &original).unwrap();
        copy_directory(&original, &old);
    });
    backup_account_file_root_v2(&mut root, &target).unwrap();
    drop(root);
    assert_eq!(copied_history(&s, &path), before);
    assert_eq!(copied_history(&s, &moved.join("v2")), before);
    inspect_account_file_archive(target).unwrap();
    verify_source(&s);
}
