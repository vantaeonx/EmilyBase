use super::*;

fn parent_at(path: &Path) -> File {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    File::open(path).unwrap()
}

#[test]
fn relative_owners_keep_parent_lock_after_caller_release_and_return_exact_descriptors() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (path, wal, info) = fixture(temp.path(), compacted, b"original");
        let parent = parent_at(temp.path());
        parent.try_lock().unwrap();
        let fds = fs::read_dir("/proc/self/fd").unwrap().count();
        let mut root = NativeFileRoot::open_at(&parent, "root", PROJECT).unwrap();
        drop(parent);
        assert!(File::open(temp.path()).unwrap().try_lock().is_err());
        assert!(Database::open(path.join("metadata")).is_err());
        assert!(ProjectDirectory::open(path.join("objects"), PROJECT).is_err());
        assert_eq!(root.list().unwrap(), vec![info]);
        let snapshot = root.capture().unwrap();
        assert_eq!(
            &snapshot.metadata_bytes()[emilybase_backup::HEADER_SIZE..],
            wal
        );
        let observed = root.inspect().unwrap();
        let copy = temp.path().join("copy");
        restore_file_archive(&encoded(&snapshot), PROJECT, &copy).unwrap();
        assert_eq!(
            NativeFileRoot::open(&copy, PROJECT)
                .unwrap()
                .inspect()
                .unwrap(),
            observed
        );
        drop(root);
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds - 1);
        File::open(temp.path()).unwrap().try_lock().unwrap();
        assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
    }
}

#[test]
fn parent_moves_during_each_original_owner_admission_keep_exact_selected_component() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for phase in [
            OpenBoundary::Selected,
            OpenBoundary::Metadata,
            OpenBoundary::Owned,
        ] {
            let temp = tempfile::tempdir().unwrap();
            let original = temp.path().join("parent");
            fs::DirBuilder::new().mode(0o700).create(&original).unwrap();
            let (path, wal, info) = fixture(&original, compacted, b"original");
            let parent = parent_at(&original);
            let moved = temp.path().join("moved");
            let mut root = NativeFileRoot::open_at_with(&parent, "root", PROJECT, |at| {
                if at == phase {
                    fs::rename(&original, &moved).unwrap();
                    fs::DirBuilder::new().mode(0o700).create(&original).unwrap();
                    copy_pair(&moved.join("root"), &path);
                }
            })
            .unwrap();
            drop(parent);
            assert_eq!(root.list().unwrap(), vec![info.clone()]);
            root.rename(FILE, info.revision(), "selected original")
                .unwrap();
            assert_ne!(fs::read(moved.join("root/metadata/redo.wal")).unwrap(), wal);
            assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
            assert_eq!(
                NativeFileRoot::open(path, PROJECT).unwrap().list().unwrap(),
                vec![info]
            );
        }
    }
}

#[test]
fn moved_original_parent_routes_capture_and_guarded_mutation_without_adopting_old_path_copy() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let parent_path = temp.path().join("parent");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&parent_path)
            .unwrap();
        let (path, wal, info) = fixture(&parent_path, compacted, b"original");
        let parent = parent_at(&parent_path);
        let mut root = NativeFileRoot::open_at(&parent, "root", PROJECT).unwrap();
        drop(parent);
        let moved = temp.path().join("moved");
        fs::rename(&parent_path, &moved).unwrap();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&parent_path)
            .unwrap();
        copy_pair(&moved.join("root"), &path);
        root.check_directory_at(&File::open(&moved).unwrap(), "root")
            .unwrap();
        assert!(
            root.check_directory_at(&File::open(&parent_path).unwrap(), "root")
                .is_err()
        );
        assert_eq!(root.list().unwrap(), vec![info.clone()]);
        let renamed = root
            .rename(FILE, info.revision(), "original renamed")
            .unwrap();
        let original = root.inspect().unwrap();
        let state = root.quota_state().unwrap();
        let expanded = FileQuota::new(4, 32768).unwrap();
        root.set_quota(state, expanded).unwrap();
        let added = root
            .publish(
                FileId::from_bytes([6; 16]),
                ObjectId::from_bytes([7; 16]),
                [4; 16],
                "new original",
                b"after move",
            )
            .unwrap();
        assert!(root.set_quota(state, expanded).is_err());
        root.remove(FILE, renamed.revision()).unwrap();
        assert_eq!(root.list().unwrap(), vec![added.clone()]);
        let captured = root.capture().unwrap();
        let copy = temp.path().join("restored");
        restore_file_archive(&encoded(&captured), PROJECT, &copy).unwrap();
        let mut restored =
            NativeFileRoot::open_at(&parent_at(temp.path()), "restored", PROJECT).unwrap();
        assert_eq!(restored.list().unwrap(), vec![added.clone()]);
        assert_eq!(restored.inspect().unwrap(), root.inspect().unwrap());
        restored
            .rename(added.id(), added.revision(), "copy only")
            .unwrap();
        assert_eq!(root.list().unwrap(), vec![added.clone()]);
        root.rename(added.id(), added.revision(), "source only")
            .unwrap();
        assert_eq!(
            restored.file_info(added.id()).unwrap().unwrap().name(),
            "copy only"
        );
        assert_eq!(
            root.file_info(added.id()).unwrap().unwrap().name(),
            "source only"
        );
        assert!(root.inspect().unwrap().revision() > original.revision());
        drop(root);
        let mut replacement = NativeFileRoot::open(&path, PROJECT).unwrap();
        assert_eq!(replacement.list().unwrap(), vec![info]);
        assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
        assert_ne!(fs::read(moved.join("root/metadata/redo.wal")).unwrap(), wal);
    }
}

#[test]
fn borrowed_payload_cursor_keeps_original_parent_across_move_and_caller_release() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let parent_path = temp.path().join("parent");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&parent_path)
            .unwrap();
        let payload = (0..8193).map(|n| (n % 251) as u8).collect::<Vec<_>>();
        let (path, wal, _) = fixture(&parent_path, compacted, &payload);
        let parent = parent_at(&parent_path);
        let mut root = NativeFileRoot::open_at(&parent, "root", PROJECT).unwrap();
        drop(parent);
        let mut reader = root.reader(FILE).unwrap();
        let mut first = [0; 8192];
        assert_eq!(reader.read_payload(&mut first).unwrap(), 8192);
        assert_eq!(first, payload[..8192]);
        let moved = temp.path().join("moved");
        fs::rename(&parent_path, &moved).unwrap();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&parent_path)
            .unwrap();
        copy_pair(&moved.join("root"), &path);
        let mut last = [0; 1];
        assert_eq!(reader.read_payload(&mut last).unwrap(), 1);
        assert_eq!(last[0], payload[8192]);
        reader.seek_payload(SeekFrom::Start(7)).unwrap();
        assert_eq!(reader.read_payload(&mut last).unwrap(), 1);
        assert_eq!(last[0], payload[7]);
        reader.finish().unwrap();
        assert_eq!(root.inspect().unwrap().usage().references, 1);
        assert_eq!(fs::read(moved.join("root/metadata/redo.wal")).unwrap(), wal);
        assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
    }
}

#[test]
fn supplied_foreign_parent_leaf_copy_or_symlink_refusal_never_poisons_healthy_source() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (path, wal, info) = fixture(temp.path(), false, b"original");
    let parent = parent_at(temp.path());
    let mut root = NativeFileRoot::open_at(&parent, "root", PROJECT).unwrap();
    File::open(temp.path()).unwrap().try_lock().unwrap();
    copy_pair(&path, &temp.path().join("copy"));
    symlink(&path, temp.path().join("alias")).unwrap();
    fs::write(temp.path().join("ordinary"), b"foreign").unwrap();
    let other = tempfile::tempdir().unwrap();
    let foreign = parent_at(other.path());
    for (parent, leaf) in [
        (&foreign, "root"),
        (&parent, "copy"),
        (&parent, "alias"),
        (&parent, "ordinary"),
        (&parent, "missing"),
    ] {
        assert!(root.check_directory_at(parent, leaf).is_err());
        assert_eq!(root.list().unwrap(), vec![info.clone()]);
    }
    root.check_directory_at(&File::open(temp.path()).unwrap(), "root")
        .unwrap();
    for leaf in [
        "",
        ".",
        "..",
        "root/",
        "../root",
        "root\0x",
        &"я".repeat(128),
    ] {
        assert!(root.check_directory_at(&parent, leaf).is_err());
        assert_eq!(root.list().unwrap(), vec![info.clone()]);
    }
    fs::set_permissions(other.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(root.check_directory_at(&foreign, "root").is_err());
    assert_eq!(root.list().unwrap(), vec![info]);
    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
    drop(root);
    NativeFileRoot::open(&path, PROJECT)
        .unwrap()
        .check_directory_at(&parent, "root")
        .unwrap();
}

#[test]
fn invalid_leaf_parent_scope_missing_and_busy_admission_preserve_sources_and_descriptors() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (path, wal, _) = fixture(temp.path(), true, b"original");
    let parent = parent_at(temp.path());
    let fds = fs::read_dir("/proc/self/fd").unwrap().count();
    for leaf in [
        "",
        ".",
        "..",
        "root/",
        "./root",
        "../root",
        "root\0other",
        &"x".repeat(256),
        &"я".repeat(128),
    ] {
        assert!(NativeFileRoot::open_at(&parent, leaf, PROJECT).is_err());
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
    }
    assert!(NativeFileRoot::open_at(&parent, "missing", PROJECT).is_err());
    let file = File::open(path.join("metadata/redo.wal")).unwrap();
    assert!(NativeFileRoot::open_at(&file, "root", PROJECT).is_err());
    drop(file);
    assert!(NativeFileRoot::open_at(&parent, "root", ProjectId::from_bytes([9; 16])).is_err());
    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
    for objects in [false, true] {
        let database;
        let directory;
        if objects {
            directory = Some(ProjectDirectory::open(path.join("objects"), PROJECT).unwrap());
            database = None;
        } else {
            database = Some(Database::open(path.join("metadata")).unwrap());
            directory = None;
        }
        assert!(NativeFileRoot::open_at(&parent, "root", PROJECT).is_err());
        drop((database, directory));
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
        NativeFileRoot::open_at(&parent, "root", PROJECT).unwrap();
    }
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(NativeFileRoot::open_at(&parent, "root", PROJECT).is_err());
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
    let longest = "я".repeat(127) + "a";
    fs::rename(&path, temp.path().join(&longest)).unwrap();
    NativeFileRoot::open_at(&parent, &longest, PROJECT).unwrap();
}

#[test]
fn relative_selected_root_children_wal_and_scope_changes_refuse_at_all_open_boundaries() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for phase in [
            OpenBoundary::Selected,
            OpenBoundary::Metadata,
            OpenBoundary::Owned,
        ] {
            for kind in 0..8 {
                let temp = tempfile::tempdir().unwrap();
                let (path, wal, _) = fixture(temp.path(), compacted, b"original");
                let parent = parent_at(temp.path());
                let saved = temp.path().join("saved");
                let result = NativeFileRoot::open_at_with(&parent, "root", PROJECT, |at| {
                    if at == phase {
                        if kind == 7 {
                            fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755))
                                .unwrap();
                        } else {
                            change(&path, &saved, kind);
                        }
                    }
                });
                assert!(result.is_err(), "kind={kind}");
                let original = match kind {
                    0 => saved.join("metadata/redo.wal"),
                    1 => saved.join("redo.wal"),
                    3 => saved.clone(),
                    _ => path.join("metadata/redo.wal"),
                };
                assert_eq!(fs::read(original).unwrap(), wal);
                if kind == 6 {
                    assert_eq!(fs::read(path.join("unexpected")).unwrap(), b"foreign entry");
                }
                File::open(path.join("metadata"))
                    .unwrap()
                    .try_lock()
                    .unwrap();
                if kind == 1 {
                    File::open(&saved).unwrap().try_lock().unwrap();
                }
            }
        }
    }
}

#[test]
fn relative_source_changes_around_copied_results_refuse_and_require_reopen() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for phase in [ReadBoundary::Admitted, ReadBoundary::Copied] {
            for kind in 0..8 {
                let temp = tempfile::tempdir().unwrap();
                let (path, _, _) = fixture(temp.path(), compacted, b"original");
                let parent = parent_at(temp.path());
                let mut root = NativeFileRoot::open_at(&parent, "root", PROJECT).unwrap();
                let result = root.observe(
                    |store, _| store.capture(),
                    |at| {
                        if at == phase {
                            if kind == 7 {
                                fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755))
                                    .unwrap();
                            } else {
                                change(&path, &temp.path().join("saved"), kind);
                            }
                        }
                    },
                );
                assert!(result.is_err(), "kind={kind}");
                assert!(matches!(root.inspect(), Err(Error::Poisoned)));
            }
        }
    }
}

#[test]
fn source_failure_during_supplied_comparison_poisons_original_even_after_namespace_repair() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (path, wal, _) = fixture(temp.path(), false, b"original");
    let parent = parent_at(temp.path());
    let mut root = NativeFileRoot::open_at(&parent, "root", PROJECT).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(root.check_directory_at(&parent, "missing").is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(root.inspect(), Err(Error::Poisoned)));
    drop(root);
    NativeFileRoot::open_at(&parent, "root", PROJECT).unwrap();
    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
}

#[test]
fn relative_parent_privacy_failure_before_or_after_publication_never_loses_acknowledged_reference()
{
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for phase in [
            PublishBoundary::Blob,
            PublishBoundary::Metadata,
            PublishBoundary::Verified,
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = crate::native_root::publication_tests::fixture(temp.path(), compacted);
            let parent = parent_at(temp.path());
            let mut root = NativeFileRoot::open_at(&parent, "root", PROJECT).unwrap();
            let before = root.inspect().unwrap();
            let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
            let result = root.publish_with(
                FileId::from_bytes([6; 16]),
                ObjectId::from_bytes([7; 16]),
                [8; 16],
                "fresh",
                b"payload",
                |at| {
                    if at == phase {
                        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755))
                            .unwrap();
                    }
                },
            );
            assert!(matches!(result, Err(Error::OutcomeUnknown(_))));
            assert!(matches!(root.inspect(), Err(Error::Poisoned)));
            drop(root);
            fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let committed = phase != PublishBoundary::Blob;
            let mut reopened = NativeFileRoot::open_at(&parent, "root", PROJECT).unwrap();
            let observed = reopened.inspect().unwrap();
            assert_eq!(
                observed.revision(),
                before.revision() + u64::from(committed)
            );
            assert_eq!(observed.usage().physical_objects, 3);
            assert_eq!(observed.usage().orphans, if committed { 1 } else { 2 });
            assert_eq!(
                reopened
                    .file_info(FileId::from_bytes([6; 16]))
                    .unwrap()
                    .is_some(),
                committed
            );
            if !committed {
                assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
            }
            let snapshot = reopened.capture().unwrap();
            let copy = temp.path().join("copy");
            restore_file_archive(&encoded(&snapshot), PROJECT, &copy).unwrap();
            assert_eq!(
                NativeFileRoot::open(&copy, PROJECT)
                    .unwrap()
                    .inspect()
                    .unwrap(),
                observed
            );
        }
    }
}

#[test]
fn killed_relative_components_release_parent_and_original_owners_with_exact_history() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for empty in [false, true] {
            for phase in ["selected", "metadata", "owned", "ack"] {
                let temp = tempfile::tempdir().unwrap();
                let payload = if empty { vec![] } else { vec![0x53; 8193] };
                let (path, wal, info) = fixture(temp.path(), compacted, &payload);
                let parent = parent_at(temp.path());
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "native_root::tests::worker",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("EMILYBASE_NATIVE_FILE_ROOT_TEST_PARENT", temp.path())
                    .env("EMILYBASE_NATIVE_FILE_ROOT_TEST_PHASE", phase)
                    .env("EMILYBASE_NATIVE_FILE_ROOT_TEST_AT", "1")
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let stdout = child.stdout.take().unwrap();
                let (send, receive) = std::sync::mpsc::channel();
                let thread = std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines() {
                        if line.unwrap() == "EB_NATIVE_FILE_ROOT_READY" {
                            send.send(()).unwrap();
                            return;
                        }
                    }
                });
                let ready = receive.recv_timeout(std::time::Duration::from_secs(15));
                let locked = File::open(temp.path()).unwrap().try_lock().is_err();
                let data_locked =
                    phase == "selected" || Database::open(path.join("metadata")).is_err();
                let objects_locked = !matches!(phase, "owned" | "ack")
                    || ProjectDirectory::open(path.join("objects"), PROJECT).is_err();
                child.kill().unwrap();
                let status = child.wait().unwrap();
                thread.join().unwrap();
                assert!(ready.is_ok());
                assert!(locked);
                assert!(data_locked);
                assert!(objects_locked);
                assert_eq!(status.signal(), Some(9));
                File::open(temp.path()).unwrap().try_lock().unwrap();
                assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
                let mut root = NativeFileRoot::open_at(&parent, "root", PROJECT).unwrap();
                assert_eq!(root.list().unwrap(), vec![info]);
                assert_eq!(root.inspect().unwrap().usage().orphans, 1);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_relative_capture_and_rename_model_survives_parent_moves(
        compacted in any::<bool>(),
        payload in prop::collection::vec(any::<u8>(),0..2049),
        names in prop::collection::vec(prop_oneof![Just("same".to_owned()), "[a-z]{1,12}"],1..9),
    ) {
        let _serial=TEST_IO.lock().unwrap();
        let temp=tempfile::tempdir().unwrap();
        let parent_path=temp.path().join("parent");
        fs::DirBuilder::new().mode(0o700).create(&parent_path).unwrap();
        let (path,wal,info)=fixture(&parent_path,compacted,&payload);
        let parent=parent_at(&parent_path);
        let mut root=NativeFileRoot::open_at(&parent,"root",PROJECT).unwrap();
        drop(parent);
        let mut global=root.inspect().unwrap().revision();
        let mut logical=info.revision();
        let mut display=info.name().to_owned();
        let moved=temp.path().join("moved");
        fs::rename(&parent_path,&moved).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&parent_path).unwrap();
        copy_pair(&moved.join("root"),&path);
        for (step,name) in names.iter().enumerate() {
            let previous=fs::read(moved.join("root/metadata/redo.wal")).unwrap();
            let current=root.rename(FILE,logical,name).unwrap();
            if name!=&display {
                global+=1;
                logical=global;
                display=name.clone();
            } else {
                prop_assert_eq!(fs::read(moved.join("root/metadata/redo.wal")).unwrap(),previous);
            }
            prop_assert_eq!(current.name(),display.as_str());
            prop_assert_eq!(current.revision(),logical);
            let observed=root.inspect().unwrap();
            prop_assert_eq!(observed.revision(),global);
            prop_assert_eq!(observed.usage().orphans,1);
            prop_assert_eq!(observed.usage().payload_bytes,payload.len() as u64+6);
            let snapshot=root.capture().unwrap();
            prop_assert_eq!(snapshot.metadata_report().last_transaction,global);
            let copy=temp.path().join(format!("copy-{step}"));
            restore_file_archive(&encoded(&snapshot),PROJECT,&copy).unwrap();
            let mut restored=NativeFileRoot::open(&copy,PROJECT).unwrap();
            prop_assert_eq!(restored.list().unwrap(),vec![current]);
            let mut read=Vec::new();
            let mut reader=restored.reader(FILE).unwrap();
            let mut scratch=[0;8192];
            loop {
                let n=reader.read_payload(&mut scratch).unwrap();
                if n==0 {break;}
                read.extend_from_slice(&scratch[..n]);
            }
            reader.finish().unwrap();
            prop_assert_eq!(&read,&payload);
            prop_assert_eq!(&fs::read(path.join("metadata/redo.wal")).unwrap(),&wal);
        }
    }
}
