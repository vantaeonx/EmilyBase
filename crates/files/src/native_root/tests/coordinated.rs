use super::*;
use std::os::unix::fs::MetadataExt;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn object(path: &Path, orphan: bool) -> PathBuf {
    path.join(if orphan {
        "objects/05050505050505050505050505050505.object"
    } else {
        "objects/03030303030303030303030303030303.object"
    })
}
fn open(path: &Path, parent: &File, relative: bool) -> NativeFileRoot {
    if relative {
        NativeFileRoot::open_at(parent, "root", PROJECT).unwrap()
    } else {
        NativeFileRoot::open(path, PROJECT).unwrap()
    }
}
fn parent(path: &Path) -> File {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    File::open(path).unwrap()
}
fn payload_fds(pid: u32, path: &Path) -> usize {
    let expected = fs::metadata(path).unwrap();
    let Ok(entries) = fs::read_dir(format!("/proc/{pid}/fd")) else {
        return 0;
    };
    entries
        .filter_map(|entry| fs::metadata(entry.ok()?.path()).ok())
        .filter(|m| (m.dev(), m.ino()) == (expected.dev(), expected.ino()))
        .count()
}

#[test]
fn copied_object_substitution_during_later_coordination_withholds_the_first_result() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for relative in [false, true] {
            for orphan in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let (path, wal, _) = fixture(temp.path(), compacted, b"original");
                let parent = parent(temp.path());
                let object = object(&path, orphan);
                let saved = temp.path().join("saved");
                let mut root = open(&path, &parent, relative);
                let result = root.with_snapshot(|snapshot| {
                    fs::rename(&object, &saved).unwrap();
                    fs::copy(&saved, &object).unwrap();
                    snapshot
                });
                assert!(result.is_err());
                assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                assert_eq!(fs::read(object).unwrap(), fs::read(saved).unwrap());
                assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
            }
        }
    }
}

#[test]
fn every_original_payload_including_orphans_spans_work_and_copies_own_no_descriptors() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for relative in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let (path, wal, info) = fixture(temp.path(), compacted, b"original");
            let parent = parent(temp.path());
            let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
            let mut root = open(&path, &parent, relative);
            let expected = encoded(&root.capture().unwrap());
            for orphan in [false, true] {
                assert_eq!(payload_fds(std::process::id(), &object(&path, orphan)), 0);
            }
            let snapshot = root
                .with_snapshot(|snapshot| {
                    for orphan in [false, true] {
                        assert!(payload_fds(std::process::id(), &object(&path, orphan)) >= 1);
                    }
                    assert!(Database::open(path.join("metadata")).is_err());
                    assert!(ProjectDirectory::open(path.join("objects"), PROJECT).is_err());
                    assert_eq!(snapshot.files(), [info]);
                    snapshot
                })
                .unwrap();
            assert_eq!(encoded(&snapshot), expected);
            for orphan in [false, true] {
                assert_eq!(payload_fds(std::process::id(), &object(&path, orphan)), 0);
            }
            drop(root);
            assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
            let copy = temp.path().join("copy");
            restore_file_archive(&encoded(&snapshot), PROJECT, &copy).unwrap();
            assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
        }
    }
}

#[test]
fn empty_boundary_maximum_and_readonly_payloads_keep_existing_archive_bytes_and_fresh_restore() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for length in [
            0,
            1,
            8192,
            8193,
            emilybase_object_storage::MAX_PAYLOAD_BYTES,
        ] {
            let temp = tempfile::tempdir().unwrap();
            let payload = vec![0x53; length];
            let (path, _, _) = fixture(temp.path(), compacted, &[]);
            let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
            let quota = root.quota_state().unwrap();
            root.set_quota(quota, FileQuota::new(3, length as u64 + 6).unwrap())
                .unwrap();
            let large = FileId::from_bytes([6; 16]);
            root.publish(
                large,
                ObjectId::from_bytes([7; 16]),
                [8; 16],
                "bounded payload",
                &payload,
            )
            .unwrap();
            fs::set_permissions(
                path.join("objects/07070707070707070707070707070707.object"),
                fs::Permissions::from_mode(0o400),
            )
            .unwrap();
            let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
            let expected = encoded(&root.capture().unwrap());
            let before = root.inspect().unwrap();
            let snapshot = root.with_snapshot(|snapshot| snapshot).unwrap();
            assert_eq!(encoded(&snapshot), expected);
            assert_eq!(root.inspect().unwrap(), before);
            let copy = temp.path().join("copy");
            restore_file_archive(&encoded(&snapshot), PROJECT, &copy).unwrap();
            let mut restored = NativeFileRoot::open(copy, PROJECT).unwrap();
            assert_eq!(restored.inspect().unwrap(), before);
            let mut reader = restored.reader(large).unwrap();
            let mut scratch = [0; 8192];
            let mut position = 0;
            loop {
                let n = reader.read_payload(&mut scratch).unwrap();
                if n == 0 {
                    break;
                }
                assert_eq!(&scratch[..n], &payload[position..position + n]);
                position += n;
            }
            reader.finish().unwrap();
            assert_eq!(position, length);
            assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
        }
    }
}

#[test]
fn callback_error_is_a_value_while_failed_source_verification_drops_returned_value() {
    struct Tracked {
        _snapshot: FileSnapshot,
        drops: Arc<AtomicUsize>,
    }
    impl Drop for Tracked {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (path, wal, info) = fixture(temp.path(), false, b"original");
    let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
    let outcome: Result<Result<()>> = root.with_snapshot(|_| Err(Error::Name));
    assert!(matches!(outcome, Ok(Err(Error::Name))));
    assert_eq!(root.list().unwrap(), vec![info]);
    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
    let drops = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&drops);
    let result = root.with_snapshot(|snapshot| {
        let source = object(&path, true);
        let saved = temp.path().join("saved");
        fs::rename(&source, &saved).unwrap();
        fs::copy(saved, source).unwrap();
        Tracked {
            _snapshot: snapshot,
            drops: observed,
        }
    });
    assert!(result.is_err());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(matches!(root.inspect(), Err(Error::Poisoned)));
}

#[test]
fn payload_corruption_nonprivate_modes_hardlinks_and_missing_objects_refuse_without_repair() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for relative in [false, true] {
            for orphan in [false, true] {
                for kind in 0..4 {
                    let temp = tempfile::tempdir().unwrap();
                    let (path, wal, _) = fixture(temp.path(), compacted, b"original");
                    let parent = parent(temp.path());
                    let source = object(&path, orphan);
                    let original = fs::read(&source).unwrap();
                    let alias = temp.path().join("alias");
                    let mut root = open(&path, &parent, relative);
                    let result = root.with_snapshot(|snapshot| {
                        match kind {
                            0 => {
                                let mut bytes = original.clone();
                                *bytes.last_mut().unwrap() ^= 1;
                                fs::write(&source, bytes).unwrap();
                            }
                            1 => fs::set_permissions(&source, fs::Permissions::from_mode(0o644))
                                .unwrap(),
                            2 => fs::hard_link(&source, &alias).unwrap(),
                            _ => fs::rename(&source, &alias).unwrap(),
                        };
                        snapshot
                    });
                    assert!(result.is_err(), "kind={kind}");
                    assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                    drop(root);
                    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
                    if kind == 0 {
                        assert_ne!(fs::read(&source).unwrap(), original);
                    } else if kind == 3 {
                        assert!(!source.exists());
                        assert_eq!(fs::read(alias).unwrap(), original);
                    } else {
                        assert_eq!(fs::read(source).unwrap(), original);
                        if kind == 2 {
                            assert!(alias.exists());
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn original_namespace_and_relative_parent_changes_withhold_callback_results() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for relative in [false, true] {
            for kind in 0..8 {
                if kind == 7 && !relative {
                    continue;
                }
                let temp = tempfile::tempdir().unwrap();
                let (path, _, _) = fixture(temp.path(), compacted, b"original");
                let parent = parent(temp.path());
                let mut root = open(&path, &parent, relative);
                let saved = temp.path().join("saved");
                let result = root.with_snapshot(|snapshot| {
                    if kind == 7 {
                        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755))
                            .unwrap();
                    } else {
                        change(&path, &saved, kind);
                    }
                    snapshot
                });
                assert!(result.is_err(), "relative={relative} kind={kind}");
                assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                drop(root);
                if kind < 5 {
                    assert!(saved.exists());
                }
                if kind == 6 {
                    assert_eq!(fs::read(path.join("unexpected")).unwrap(), b"foreign entry");
                }
            }
        }
    }
}

#[test]
fn damaged_initial_source_never_invokes_coordination_work_or_repairs_the_wal() {
    use std::cell::Cell;
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (path, _, _) = fixture(temp.path(), false, b"original");
    let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
    let wal = path.join("metadata/redo.wal");
    fs::write(&wal, []).unwrap();
    let called = Cell::new(false);
    assert!(root.with_snapshot(|_| called.set(true)).is_err());
    assert!(!called.get());
    assert!(matches!(root.inspect(), Err(Error::Poisoned)));
    drop(root);
    assert_eq!(fs::metadata(wal).unwrap().len(), 0);
}

#[test]
fn parent_move_inside_work_keeps_relative_source_and_preserves_normal_absolute_refusal() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for relative in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let original = temp.path().join("parent");
            fs::DirBuilder::new().mode(0o700).create(&original).unwrap();
            let (path, wal, _) = fixture(&original, compacted, b"original");
            let parent = parent(&original);
            let mut root = open(&path, &parent, relative);
            drop(parent);
            let moved = temp.path().join("moved");
            let result = root.with_snapshot(|snapshot| {
                fs::rename(&original, &moved).unwrap();
                fs::DirBuilder::new().mode(0o700).create(&original).unwrap();
                copy_pair(&moved.join("root"), &path);
                assert!(payload_fds(std::process::id(), &object(&moved.join("root"), false)) >= 1);
                snapshot
            });
            if relative {
                let snapshot = result.unwrap();
                assert_eq!(
                    &snapshot.metadata_bytes()[emilybase_backup::HEADER_SIZE..],
                    wal
                );
                assert_eq!(root.list().unwrap().len(), 1);
            } else {
                assert!(result.is_err());
                assert!(matches!(root.inspect(), Err(Error::Poisoned)));
            }
            assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
            assert_eq!(fs::read(moved.join("root/metadata/redo.wal")).unwrap(), wal);
        }
    }
}

fn second(parent: &Path) -> (PathBuf, NativeFileRoot) {
    let project = ProjectId::from_bytes([9; 16]);
    let path = parent.join("second");
    initialize_file_root(&path, project, FileQuota::new(8, 32768).unwrap()).unwrap();
    let mut root = NativeFileRoot::open(&path, project).unwrap();
    root.publish(
        FILE,
        ObjectId::from_bytes([3; 16]),
        [4; 16],
        "second",
        b"second source",
    )
    .unwrap();
    (path, root)
}

#[test]
fn nested_components_keep_all_original_payload_descriptors_until_outer_work_finishes() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (path, _, _) = fixture(temp.path(), true, b"first source");
    let mut first = NativeFileRoot::open(&path, PROJECT).unwrap();
    let (other, mut second) = second(temp.path());
    let snapshots = first
        .with_snapshot(|a| {
            second.with_snapshot(|b| {
                for source in [
                    object(&path, false),
                    object(&path, true),
                    object(&other, false),
                ] {
                    assert!(payload_fds(std::process::id(), &source) >= 1);
                }
                assert!(Database::open(path.join("metadata")).is_err());
                assert!(Database::open(other.join("metadata")).is_err());
                vec![a, b]
            })
        })
        .unwrap()
        .unwrap();
    assert_eq!(snapshots[0].project(), PROJECT);
    assert_eq!(snapshots[1].project(), ProjectId::from_bytes([9; 16]));
    for source in [
        object(&path, false),
        object(&path, true),
        object(&other, false),
    ] {
        assert_eq!(payload_fds(std::process::id(), &source), 0);
    }
    for (index, snapshot) in snapshots.iter().enumerate() {
        let copy = temp.path().join(format!("copy-{index}"));
        restore_file_archive(&encoded(snapshot), snapshot.project(), &copy).unwrap();
        assert_eq!(
            NativeFileRoot::open(copy, snapshot.project())
                .unwrap()
                .list()
                .unwrap(),
            snapshot.files()
        );
    }
}

#[test]
fn nested_component_failure_never_returns_joint_images_and_poisons_only_changed_sources() {
    let _serial = TEST_IO.lock().unwrap();
    for target_first in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (path, _, _) = fixture(temp.path(), false, b"first source");
        let mut first = NativeFileRoot::open(&path, PROJECT).unwrap();
        let (other, mut second) = second(temp.path());
        let source = object(if target_first { &path } else { &other }, false);
        let saved = temp.path().join("saved");
        let bytes = fs::read(&source).unwrap();
        let result = first.with_snapshot(|a| {
            second.with_snapshot(|b| {
                fs::rename(&source, &saved).unwrap();
                fs::copy(&saved, &source).unwrap();
                vec![a, b]
            })
        });
        assert!(result.and_then(|inner| inner).is_err());
        assert_eq!(first.inspect().is_err(), target_first);
        assert_eq!(second.inspect().is_err(), !target_first);
        assert_eq!(fs::read(source).unwrap(), bytes);
        assert_eq!(fs::read(saved).unwrap(), bytes);
    }
}

#[test]
fn killed_coordination_retains_actual_objects_until_work_ends_then_reopens_exact_history() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for relative in [false, true] {
            for empty in [false, true] {
                for phase in ["coordinating", "coordinated"] {
                    let temp = tempfile::tempdir().unwrap();
                    let payload = if empty { vec![] } else { vec![0x53; 8193] };
                    let (path, wal, info) = fixture(temp.path(), compacted, &payload);
                    let parent = parent(temp.path());
                    let mut child = Command::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            "native_root::tests::worker",
                            "--ignored",
                            "--nocapture",
                        ])
                        .env("EMILYBASE_NATIVE_FILE_ROOT_TEST_PARENT", temp.path())
                        .env("EMILYBASE_NATIVE_FILE_ROOT_TEST_PHASE", phase)
                        .env(
                            "EMILYBASE_NATIVE_FILE_ROOT_TEST_AT",
                            if relative { "1" } else { "0" },
                        )
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
                    let references = payload_fds(child.id(), &object(&path, false));
                    let orphans = payload_fds(child.id(), &object(&path, true));
                    let data_busy = Database::open(path.join("metadata")).is_err();
                    let objects_busy =
                        ProjectDirectory::open(path.join("objects"), PROJECT).is_err();
                    let parent_busy = File::open(temp.path()).unwrap().try_lock().is_err();
                    child.kill().unwrap();
                    let status = child.wait().unwrap();
                    thread.join().unwrap();
                    assert!(ready.is_ok());
                    assert_eq!(status.signal(), Some(9));
                    assert!(data_busy);
                    assert!(objects_busy);
                    assert_eq!(parent_busy, relative);
                    if phase == "coordinating" {
                        assert!(references >= 1);
                        assert!(orphans >= 1);
                    } else {
                        assert_eq!(references, 0);
                        assert_eq!(orphans, 0);
                    }
                    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
                    let mut root = open(&path, &parent, relative);
                    assert_eq!(root.list().unwrap(), vec![info]);
                    root.with_snapshot(|_| ()).unwrap();
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_work_and_fresh_restore_preserve_payload_revision_and_orphan_model(
        compacted in any::<bool>(),relative in any::<bool>(),payload in prop::collection::vec(any::<u8>(),0..4097),
    ) {
        let _serial=TEST_IO.lock().unwrap();
        let temp=tempfile::tempdir().unwrap();
        let (path,wal,info)=fixture(temp.path(),compacted,&payload);
        let parent=parent(temp.path());
        let mut root=open(&path,&parent,relative);
        let snapshot=root.with_snapshot(|snapshot|{
            assert_eq!(snapshot.files(),std::slice::from_ref(&info));
            assert_eq!(snapshot.objects().inventory().payload_bytes(),payload.len() as u64+6);
            assert!(payload_fds(std::process::id(),&object(&path,true))>=1);
            snapshot
        }).unwrap();
        let copy=temp.path().join("copy");
        restore_file_archive(&encoded(&snapshot),PROJECT,&copy).unwrap();
        let mut restored=NativeFileRoot::open(copy,PROJECT).unwrap();
        prop_assert_eq!(restored.list().unwrap(),vec![info.clone()]);
        prop_assert_eq!(restored.inspect().unwrap().usage().orphans,1);
        let mut reader=restored.reader(FILE).unwrap();
        let mut scratch=[0;8192];let mut read=Vec::new();
        loop {let n=reader.read_payload(&mut scratch).unwrap();if n==0{break;}read.extend_from_slice(&scratch[..n]);}
        reader.finish().unwrap();prop_assert_eq!(read,payload);
        restored.rename(FILE,info.revision(),"restored only").unwrap();
        prop_assert_eq!(root.list().unwrap(),vec![info]);
        prop_assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(),wal);
    }
}
