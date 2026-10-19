use super::*;
use crate::MAX_ACCOUNT_FILE_READ_BYTES;
use emilybase_files::{FileId, FileQuota};

#[test]
fn declared_project_file_observations_and_complete_copies_preserve_all_source_histories() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (1, false), (4, true)] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), count, compact);
        let before = copied_history(&s, &path);
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
        for (id, key) in &s.root.credentials {
            let observed = root.inspect_project_files(id, key).unwrap();
            assert_eq!(observed.project().to_string(), *id);
            assert_eq!(observed.usage().references, 1);
            assert_eq!(observed.usage().orphans, 1);
            let list = root.list_project_files(id, key).unwrap();
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].id(), FILE);
            assert_eq!(
                root.project_file_info(id, key, FILE).unwrap(),
                Some(list[0].clone())
            );
            assert_eq!(
                root.project_file_info(id, key, FileId::from_bytes([99; 16]))
                    .unwrap(),
                None
            );
            let read = root
                .read_project_file(id, key, FILE, MAX_ACCOUNT_FILE_READ_BYTES)
                .unwrap();
            assert_eq!(read.info(), &list[0]);
            assert_eq!(read.payload(), b"synthetic-payload");
            let text = format!("{read:?}");
            assert!(!text.contains("synthetic"));
            assert!(!text.contains("synthetic-name"));
            assert_eq!(read.into_payload(), b"synthetic-payload");
            assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
            assert_eq!(
                fds(
                    std::process::id(),
                    &object(&path.join("files").join(id), false)
                ),
                0
            );
        }
        drop(root);
        assert_eq!(copied_history(&s, &path), before);
        verify_source(&s);
    }
}
#[test]
fn original_selected_payload_and_common_private_native_owners_span_final_key_and_root_checks() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for point in [
        "account_file_read_retained",
        "account_file_read_copied",
        "account_file_read_verified",
    ] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, true);
        let (id, key) = &s.root.credentials[0];
        let original = path.clone();
        let project = id.clone();
        let _hook = durability::on_boundary(point, move || {
            assert!(File::open(&original).unwrap().try_lock().is_err());
            assert!(ProjectStore::open_existing(original.join("registry")).is_err());
            assert!(
                AccountStore::open(original.join("private").join(&project), &project, pool())
                    .is_err()
            );
            let native = original.join("files").join(&project);
            assert!(NativeFileRoot::open(&native, project.parse().unwrap()).is_err());
            assert!(fds(std::process::id(), &object(&native, false)) > 0);
            assert!(
                fds(
                    std::process::id(),
                    &original
                        .join("registry")
                        .join(&project)
                        .join("project.json")
                ) > 0
            );
            // Project-service file reads do not acquire an unrelated public writer.
            assert!(Database::open(original.join("registry").join(&project).join("data")).is_ok());
        });
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        assert_eq!(
            root.read_project_file(id, key, FILE, 32).unwrap().payload(),
            b"synthetic-payload"
        );
        assert_eq!(
            fds(
                std::process::id(),
                &object(&path.join("files").join(id), false)
            ),
            0
        );
        drop(root);
        verify_source(&s);
    }
}
#[test]
fn current_project_key_is_required_before_file_lookup_reader_or_limit_admission() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 2, false);
    let (id, key) = &s.root.credentials[0];
    let sibling = &s.root.credentials[1].1;
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    let _hook = durability::on_boundary("account_file_read_retained", || {
        panic!("denied input reached payload reader")
    });
    for denied in [
        sibling.as_str(),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "malformed-key",
    ] {
        assert!(matches!(
            root.inspect_project_files(id, denied),
            Err(Error::Denied)
        ));
        assert!(matches!(
            root.list_project_files(id, denied),
            Err(Error::Denied)
        ));
        assert!(matches!(
            root.project_file_info(id, denied, FILE),
            Err(Error::Denied)
        ));
        assert!(matches!(
            root.read_project_file(id, denied, FILE, usize::MAX),
            Err(Error::Denied)
        ));
    }
    assert!(matches!(
        root.read_project_file("unknown", key, FILE, 32),
        Err(Error::Denied)
    ));
    drop(root);
    verify_source(&s);
}
#[test]
fn limits_empty_payloads_missing_references_and_physical_orphans_have_no_partial_results() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, false);
    let (id, key) = &s.root.credentials[0];
    let empty = FileId::from_bytes([8; 16]);
    let mut native =
        NativeFileRoot::open(path.join("files").join(id), id.parse().unwrap()).unwrap();
    native
        .publish(empty, ObjectId::from_bytes([9; 16]), [4; 16], "empty", b"")
        .unwrap();
    drop(native);
    let before = copied_history(&s, &path);
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    for limit in [0, 16, MAX_ACCOUNT_FILE_READ_BYTES + 1, usize::MAX] {
        assert!(matches!(
            root.read_project_file(id, key, FILE, limit),
            Err(Error::Limit)
        ));
    }
    assert_eq!(
        root.read_project_file(id, key, FILE, 17).unwrap().payload(),
        b"synthetic-payload"
    );
    assert!(
        root.read_project_file(id, key, empty, 0)
            .unwrap()
            .payload()
            .is_empty()
    );
    let orphan = FileId::from_bytes([5; 16]);
    assert!(root.project_file_info(id, key, orphan).unwrap().is_none());
    assert!(matches!(
        root.read_project_file(id, key, orphan, 32),
        Err(Error::Files(emilybase_files::Error::Missing))
    ));
    drop(root);
    assert_eq!(copied_history(&s, &path), before);
    verify_source(&s);
}
#[test]
fn per_call_project_key_metadata_replacement_with_equal_bytes_withholds_copied_payload() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, false);
    let (id, key) = &s.root.credentials[0];
    let changed = path.join("registry").join(id).join("project.json");
    let original = fs::read(&changed).unwrap();
    let selected = changed.clone();
    let held = t.path().join("original-metadata");
    let saved = held.clone();
    let _hook = durability::on_boundary("account_file_read_copied", move || {
        fs::rename(&selected, &saved).unwrap();
        fs::write(&selected, fs::read(saved).unwrap()).unwrap();
        fs::set_permissions(selected, fs::Permissions::from_mode(0o600)).unwrap();
    });
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    assert!(root.read_project_file(id, key, FILE, 32).is_err());
    assert_eq!(fs::read(changed).unwrap(), original);
    assert_eq!(fs::read(held).unwrap(), original);
}
#[test]
fn maximum_complete_copy_and_project_specific_references_keep_explicit_bounds_and_isolation() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 2, true);
    let (first, first_key) = &s.root.credentials[0];
    let (second, second_key) = &s.root.credentials[1];
    let big_id = FileId::from_bytes([8; 16]);
    let mut native =
        NativeFileRoot::open(path.join("files").join(second), second.parse().unwrap()).unwrap();
    let quota = native.quota_state().unwrap();
    native
        .set_quota(quota, FileQuota::new(128, 16 * 1024 * 1024).unwrap())
        .unwrap();
    let payload = vec![0x73; MAX_ACCOUNT_FILE_READ_BYTES];
    native
        .publish(
            big_id,
            ObjectId::from_bytes([9; 16]),
            [4; 16],
            "large",
            &payload,
        )
        .unwrap();
    drop(native);
    let before = copied_history(&s, &path);
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    assert!(
        root.project_file_info(first, first_key, big_id)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        root.read_project_file(first, first_key, big_id, MAX_ACCOUNT_FILE_READ_BYTES),
        Err(Error::Files(emilybase_files::Error::Missing))
    ));
    assert!(matches!(
        root.read_project_file(second, first_key, big_id, MAX_ACCOUNT_FILE_READ_BYTES),
        Err(Error::Denied)
    ));
    assert!(matches!(
        root.read_project_file(second, second_key, big_id, MAX_ACCOUNT_FILE_READ_BYTES - 1),
        Err(Error::Limit)
    ));
    assert_eq!(
        root.read_project_file(second, second_key, big_id, MAX_ACCOUNT_FILE_READ_BYTES)
            .unwrap()
            .payload(),
        payload
    );
    drop(root);
    assert_eq!(copied_history(&s, &path), before);
    verify_source(&s);
}
#[test]
fn v1_and_known_unbound_projects_refuse_before_payload_work_and_without_source_changes() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let s = source(t.path(), 1, false);
    let (id, key) = &s.root.credentials[0];
    let mut legacy = AccountRoot::open(&s.root.root, pool()).unwrap();
    let bytes = encode(&capture_account_file_components(&mut legacy, &mut []).unwrap());
    let _hook = durability::on_boundary("account_file_read_retained", || {
        panic!("unbound request opened a reader")
    });
    assert!(matches!(
        legacy.read_project_file(id, key, FILE, 32),
        Err(Error::Denied)
    ));
    drop(legacy);
    let out = output(t.path());
    restore_account_file_archive_bytes_at(
        &bytes,
        &File::open(&out).unwrap(),
        "unbound",
        pool(),
        40,
    )
    .unwrap();
    let mut root = AccountRoot::open_v2(out.join("unbound"), pool()).unwrap();
    assert!(matches!(
        root.inspect_project_files(id, key),
        Err(Error::Denied)
    ));
    assert!(matches!(
        root.read_project_file(id, key, FILE, 32),
        Err(Error::Denied)
    ));
    drop(root);
    verify_source(&s);
}
#[test]
fn current_key_rotation_revokes_waiting_readers_and_new_operations_use_the_new_metadata_owner() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, true);
    let (id, key) = &s.root.credentials[0];
    let root = Arc::new(Mutex::new(AccountRoot::open_v2(&path, pool()).unwrap()));
    let mut held = root.lock().unwrap();
    let waiting = Arc::clone(&root);
    let project = id.clone();
    let old = key.clone();
    let (send, receive) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        send.send(()).unwrap();
        waiting
            .lock()
            .unwrap()
            .read_project_file(&project, &old, FILE, 32)
    });
    receive
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let rotated = held.rotate_project_key(id).unwrap();
    drop(held);
    assert!(matches!(thread.join().unwrap(), Err(Error::Denied)));
    assert_eq!(
        root.lock()
            .unwrap()
            .read_project_file(id, &rotated.api_key, FILE, 32)
            .unwrap()
            .payload(),
        b"synthetic-payload"
    );
    drop(root);
    verify_source(&s);
}
#[test]
fn metadata_guard_preserves_accepted_whitespace_and_readonly_modes_and_refuses_unsafe_changes() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, false);
    let (id, key) = &s.root.credentials[0];
    let metadata = path.join("registry").join(id).join("project.json");
    let mut bytes = fs::read(&metadata).unwrap();
    bytes.extend_from_slice(b"\n  ");
    fs::write(&metadata, &bytes).unwrap();
    fs::set_permissions(&metadata, fs::Permissions::from_mode(0o400)).unwrap();
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    assert_eq!(
        root.read_project_file(id, key, FILE, 32).unwrap().payload(),
        b"synthetic-payload"
    );
    assert_eq!(fs::read(&metadata).unwrap(), bytes);
    fs::set_permissions(&metadata, fs::Permissions::from_mode(0o600)).unwrap();
    for kind in 0..4 {
        let original = fs::read(&metadata).unwrap();
        let alias = t.path().join("alias");
        match kind {
            0 => {
                fs::hard_link(&metadata, &alias).unwrap();
            }
            1 => {
                fs::set_permissions(&metadata, fs::Permissions::from_mode(0o644)).unwrap();
            }
            2 => {
                let mut changed = original.clone();
                changed[0] ^= 1;
                fs::write(&metadata, &changed).unwrap();
            }
            _ => {
                fs::write(
                    &metadata,
                    vec![b'x'; crate::MAX_METADATA_BYTES as usize + 1],
                )
                .unwrap();
            }
        }
        let altered = fs::read(&metadata).unwrap();
        assert!(root.read_project_file(id, key, FILE, 32).is_err());
        assert_eq!(fs::read(&metadata).unwrap(), altered);
        if kind == 0 {
            fs::remove_file(alias).unwrap();
        }
        fs::set_permissions(&metadata, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&metadata, &original).unwrap();
    }
    drop(root);
    verify_source(&s);
}
#[test]
fn late_payload_scope_and_project_metadata_changes_discard_the_entire_copy_and_preserve_external_files()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for point in [
            "account_file_read_retained",
            "account_file_read_copied",
            "account_file_read_verified",
        ] {
            for kind in 0..6 {
                let t = tempfile::tempdir().unwrap();
                let (s, path) = copy(t.path(), 1, compact);
                let (id, key) = &s.root.credentials[0];
                let changed = match kind {
                    0 | 1 => object(&path.join("files").join(id), false),
                    2 => path.join("files").join(id).join("metadata/redo.wal"),
                    3 => path.join("root.json"),
                    _ => path.join("registry").join(id).join("project.json"),
                };
                let before = fs::read(&changed).unwrap();
                let external = changed.clone();
                let held = t.path().join("held");
                let saved = held.clone();
                let _hook = durability::on_boundary(point, move || {
                    if matches!(kind, 1 | 3 | 5) {
                        fs::rename(&external, &saved).unwrap();
                        fs::write(&external, fs::read(saved).unwrap()).unwrap();
                        fs::set_permissions(external, fs::Permissions::from_mode(0o600)).unwrap();
                    } else {
                        File::options()
                            .append(true)
                            .open(external)
                            .unwrap()
                            .write_all(b"external tail")
                            .unwrap();
                    }
                });
                let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                assert!(
                    root.read_project_file(id, key, FILE, 32).is_err(),
                    "{compact} {point} {kind}"
                );
                if matches!(kind, 1 | 3 | 5) {
                    assert_eq!(fs::read(&changed).unwrap(), before);
                    assert_eq!(fs::read(held).unwrap(), before);
                } else {
                    let mut expected = before;
                    expected.extend_from_slice(b"external tail");
                    assert_eq!(fs::read(changed).unwrap(), expected);
                }
                drop(root);
                verify_source(&s);
            }
        }
    }
}
#[test]
fn native_parent_moves_do_not_adopt_replacements_and_readonly_access_still_allows_complete_backup()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    fs::set_permissions(t.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let work = t.path().join("work");
    fs::DirBuilder::new().mode(0o700).create(&work).unwrap();
    let (s, path) = copy(&work, 1, true);
    let (id, key) = &s.root.credentials[0];
    let before = copied_history(&s, &path);
    let parent = path.parent().unwrap().to_path_buf();
    let mut root = AccountRoot::open_v2_at(&File::open(&parent).unwrap(), "v2", pool()).unwrap();
    let moved = t.path().join("moved");
    let original = moved.clone();
    let old = parent.clone();
    let _hook = durability::on_boundary("account_file_read_copied", move || {
        fs::rename(&old, &original).unwrap();
        copy_directory(&original, &old);
    });
    assert_eq!(
        root.read_project_file(id, key, FILE, 32).unwrap().payload(),
        b"synthetic-payload"
    );
    let archive = t.path().join("after-read");
    crate::backup_account_file_root_v2(&mut root, &archive).unwrap();
    drop(root);
    assert_eq!(copied_history(&s, &path), before);
    assert_eq!(copied_history(&s, &moved.join("v2")), before);
    let restored = t.path().join("second");
    crate::restore_account_file_archive(&archive, &restored, pool(), 40).unwrap();
    let mut copy = AccountRoot::open_v2(&restored, pool()).unwrap();
    assert_eq!(
        copy.read_project_file(id, key, FILE, 32).unwrap().payload(),
        b"synthetic-payload"
    );
    drop(copy);
    verify_source(&s);
}
#[test]
fn actual_kills_release_original_readers_and_never_modify_file_or_private_histories() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for point in [
            "account_file_read_retained",
            "account_file_read_copied",
            "account_file_read_verified",
            "account_file_read_returned",
        ] {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), 1, compact);
            let (id, key) = &s.root.credentials[0];
            let before = copied_history(&s, &path);
            let key_file = t.path().join("project.key");
            fs::write(&key_file, key).unwrap();
            fs::set_permissions(&key_file, fs::Permissions::from_mode(0o600)).unwrap();
            let worker = Worker::start(&path, &key_file, id, "root-v2-file-read", point);
            worker.reach(point);
            assert!(File::open(&path).unwrap().try_lock().is_err());
            for selected in [
                object(&path.join("files").join(id), false),
                path.join("registry").join(id).join("project.json"),
            ] {
                assert_eq!(
                    fds(worker.pid(), &selected) > 0,
                    point != "account_file_read_returned"
                );
            }
            worker.kill();
            assert_eq!(copied_history(&s, &path), before);
            assert!(fs::read_to_string(key_file).unwrap().as_str() == key);
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            assert_eq!(
                root.read_project_file(id, key, FILE, 32).unwrap().payload(),
                b"synthetic-payload"
            );
            drop(root);
            verify_source(&s);
        }
    }
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_read_limits_and_rosters_preserve_source_bytes(count in 0usize..3,compact in any::<bool>(),limit in 0usize..25) {
        let _serial=durability::PROCESS_TESTS.blocking_lock();let t=tempfile::tempdir().unwrap();let (s,path)=copy(t.path(),count,compact);let before=copied_history(&s,&path);let mut root=AccountRoot::open_v2(&path,pool()).unwrap();
        for (id,key) in &s.root.credentials {let result=root.read_project_file(id,key,FILE,limit);if limit>=17 {let value=result.unwrap();prop_assert_eq!(value.payload(),b"synthetic-payload");} else {prop_assert!(matches!(result,Err(Error::Limit)));}}
        drop(root);prop_assert_eq!(copied_history(&s,&path),before);verify_source(&s);
    }
}
#[test]
fn metadata_fifo_symlink_hardlink_and_broad_mode_after_key_admission_refuse_in_bounded_workers() {
    use std::os::unix::fs::FileTypeExt;
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for kind in 0..4 {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, false);
        let (id, key) = &s.root.credentials[0];
        let metadata = path.join("registry").join(id).join("project.json");
        let bytes = fs::read(&metadata).unwrap();
        let held = t.path().join("held");
        let key_file = t.path().join("project.key");
        fs::write(&key_file, key).unwrap();
        fs::set_permissions(&key_file, fs::Permissions::from_mode(0o600)).unwrap();
        let unchanged = [
            path.join("root.json"),
            path.join("registry").join(id).join("data/redo.wal"),
            path.join("private").join(id).join("redo.wal"),
            path.join("files").join(id).join("metadata/redo.wal"),
            object(&path.join("files").join(id), false),
            object(&path.join("files").join(id), true),
        ]
        .map(|p| {
            let bytes = fs::read(&p).unwrap();
            (p, bytes)
        });
        let mut worker = Worker::start(
            &path,
            &key_file,
            id,
            "root-v2-file-read-refuse-race",
            "account_file_key_admitted",
        );
        worker.reach("account_file_key_admitted");
        fs::rename(&metadata, &held).unwrap();
        match kind {
            0 => rustix::fs::mkfifoat(
                rustix::fs::CWD,
                &metadata,
                rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            )
            .unwrap(),
            1 => std::os::unix::fs::symlink(&held, &metadata).unwrap(),
            2 => fs::hard_link(&held, &metadata).unwrap(),
            _ => {
                fs::write(&metadata, &bytes).unwrap();
                fs::set_permissions(&metadata, fs::Permissions::from_mode(0o644)).unwrap();
            }
        }
        worker.release();
        worker.finish();
        assert_eq!(fs::read(&held).unwrap(), bytes);
        if kind == 0 {
            assert!(
                fs::symlink_metadata(&metadata)
                    .unwrap()
                    .file_type()
                    .is_fifo()
            );
        }
        if kind == 1 {
            assert!(
                fs::symlink_metadata(&metadata)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
        for (p, before) in unchanged {
            assert_eq!(fs::read(p).unwrap(), before);
        }
        verify_source(&s);
    }
}
