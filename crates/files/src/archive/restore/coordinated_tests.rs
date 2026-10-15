use super::*;
use crate::with_restored_file_archive_at;
use std::cell::Cell;
use std::os::unix::fs::PermissionsExt;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn private_temp() -> tempfile::TempDir {
    let temporary = tempfile::tempdir().unwrap();
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
    temporary
}

fn private_directory(path: &Path) {
    fs::DirBuilder::new().mode(0o700).create(path).unwrap();
}

fn descriptors(path: &Path) -> usize {
    fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|entry| fs::metadata(entry.unwrap().path()).ok())
        .filter(|metadata| {
            let expected = fs::metadata(path).unwrap();
            (metadata.dev(), metadata.ino()) == (expected.dev(), expected.ino())
        })
        .count()
}

fn assert_owned(path: &Path) {
    assert!(Database::open(path.join("metadata")).is_err());
    assert!(matches!(
        File::open(path.join("metadata")).unwrap().try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    assert!(matches!(
        ProjectDirectory::open(path.join("objects"), PROJECT),
        Err(emilybase_object_storage::Error::Busy)
    ));
    for id in [OBJECT, ORPHAN] {
        assert!(descriptors(&path.join("objects").join(format!("{id}.object"))) > 0);
    }
    assert!(descriptors(&path.join("metadata/redo.wal")) > 0);
}

fn assert_released(path: &Path) {
    for id in [OBJECT, ORPHAN] {
        assert_eq!(
            descriptors(&path.join("objects").join(format!("{id}.object"))),
            0
        );
    }
    assert_eq!(descriptors(&path.join("metadata/redo.wal")), 0);
    drop(open(path).unwrap());
}

fn capture(path: &Path, compacted: bool, payload: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut store = source(path, compacted, payload);
    let bytes = encoded(&mut store);
    let wal = store.database.committed_wal().unwrap();
    (bytes, wal)
}

#[test]
fn nested_children_keep_actual_owners_and_every_payload_through_common_selection() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let source_a = private_temp();
        let source_b = private_temp();
        let output = private_temp();
        let (a, wal_a) = capture(source_a.path(), compacted, &[0x4a; 8193]);
        let (b, wal_b) = capture(source_b.path(), !compacted, b"");
        let expected_a =
            FileArchiveReport::from_verified(&verify_file_archive(&a, PROJECT).unwrap());
        let expected_b =
            FileArchiveReport::from_verified(&verify_file_archive(&b, PROJECT).unwrap());
        let target = output.path().join("common");
        let stage = StagedPrivateDirectory::new(&target).unwrap();
        let root = stage.directory().try_clone().unwrap();
        let path = descriptor_path(&root);
        let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
        let calls = Cell::new(0);
        let receipt = with_restored_file_archive_at(&a, PROJECT, &root, "first", |report_a| {
            assert_eq!(report_a, &expected_a);
            assert!(!target.exists());
            assert_owned(&path.join("first"));
            calls.set(calls.get() + 1);
            with_restored_file_archive_at(&b, PROJECT, &root, "second", |report_b| {
                assert_eq!(report_b, &expected_b);
                assert_owned(&path.join("first"));
                assert_owned(&path.join("second"));
                assert!(!target.exists());
                calls.set(calls.get() + 1);
                let selected = stage.publish().unwrap();
                selected.check().unwrap();
                assert_owned(&target.join("first"));
                assert_owned(&target.join("second"));
                (report_a.clone(), report_b.clone())
            })
        })
        .unwrap()
        .unwrap();
        assert_eq!(receipt, (expected_a, expected_b));
        assert_eq!(calls.get(), 2);
        // Consuming the enclosing stage releases its descriptor; the explicit
        // original root duplicate remains independently retained by this test.
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline - 2);
        assert_released(&target.join("first"));
        assert_released(&target.join("second"));
        for (name, wal, payload) in [
            ("first", wal_a.clone(), vec![0x4a; 8193]),
            ("second", wal_b, vec![]),
        ] {
            let mut restored = open(&target.join(name)).unwrap();
            assert_eq!(restored.database.committed_wal().unwrap(), wal);
            let snapshot = restored.capture().unwrap();
            assert_eq!(
                snapshot
                    .objects()
                    .objects()
                    .iter()
                    .find(|o| o.object() == OBJECT)
                    .unwrap()
                    .payload(),
                payload
            );
            let info = restored.info(FILE).unwrap().unwrap();
            restored
                .rename(FILE, info.revision(), "independent-after-common-restore")
                .unwrap();
        }
        assert_eq!(
            fs::read(source_a.path().join("metadata/redo.wal")).unwrap(),
            wal_a
        );
    }
}

#[test]
fn callback_refusal_is_a_value_and_releases_healthy_originals_without_deleting_child() {
    let _serial = TEST_IO.lock().unwrap();
    let source = private_temp();
    let output = private_temp();
    let (bytes, wal) = capture(source.path(), false, b"synthetic-value");
    assert_eq!(output.path().metadata().unwrap().mode() & 0o7777, 0o700);
    let root = File::open(output.path()).unwrap();
    let result = with_restored_file_archive_at(&bytes, PROJECT, &root, "child", |_| {
        assert_owned(&output.path().join("child"));
        Err::<(), _>("synthetic application refusal")
    })
    .unwrap();
    assert_eq!(result, Err("synthetic application refusal"));
    assert_released(&output.path().join("child"));
    let mut restored = open(&output.path().join("child")).unwrap();
    assert_eq!(restored.database.committed_wal().unwrap(), wal);
    assert_eq!(
        fs::read(source.path().join("metadata/redo.wal")).unwrap(),
        wal
    );
    assert!(
        with_restored_file_archive_at(&bytes, PROJECT, &root, "child", |_| panic!(
            "occupied child must refuse before work"
        ))
        .is_err()
    );
}

#[test]
fn moved_native_parent_keeps_original_selection_and_never_adopts_old_path() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let source = private_temp();
        let output = private_temp();
        let (bytes, wal) = capture(source.path(), compacted, b"moved-native-parent");
        let original = output.path().join("original");
        let moved = output.path().join("moved");
        private_directory(&original);
        let parent = File::open(&original).unwrap();
        with_restored_file_archive_at(&bytes, PROJECT, &parent, "child", |_| {
            fs::rename(&original, &moved).unwrap();
            private_directory(&original);
            private_directory(&original.join("child"));
            fs::write(original.join("child/keep"), b"foreign replacement").unwrap();
            assert_owned(&moved.join("child"));
        })
        .unwrap();
        assert_released(&moved.join("child"));
        assert_eq!(
            fs::read(moved.join("child/metadata/redo.wal")).unwrap(),
            wal
        );
        assert_eq!(
            fs::read(original.join("child/keep")).unwrap(),
            b"foreign replacement"
        );
    }
}

#[test]
fn explicit_private_parent_and_leaf_bounds_refuse_before_any_stage_or_work() {
    let _serial = TEST_IO.lock().unwrap();
    let source = private_temp();
    let output = private_temp();
    let (bytes, _) = capture(source.path(), false, b"bounds");
    let parent = File::open(output.path()).unwrap();
    let long = "x".repeat(256);
    for name in ["", ".", "..", "../escape", "a/b", "nul\0leaf", &long] {
        assert!(
            with_restored_file_archive_at(&bytes, PROJECT, &parent, name, |_| panic!(
                "invalid leaf ran work"
            ))
            .is_err()
        );
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    }
    for mode in [0o755, 0o770, 0o1700, 0o2700, 0o4700] {
        fs::set_permissions(output.path(), fs::Permissions::from_mode(mode)).unwrap();
        assert!(
            with_restored_file_archive_at(&bytes, PROJECT, &parent, "child", |_| panic!(
                "nonprivate parent ran work"
            ))
            .is_err()
        );
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    }
    fs::set_permissions(output.path(), fs::Permissions::from_mode(0o700)).unwrap();
    for invalid in [
        &bytes[..191],
        &bytes[..bytes.len() - 1],
        b"wrong".as_slice(),
    ] {
        assert!(
            with_restored_file_archive_at(invalid, PROJECT, &parent, "child", |_| panic!(
                "invalid bytes ran work"
            ))
            .is_err()
        );
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    }
    assert!(
        with_restored_file_archive_at(
            &bytes,
            ProjectId::from_bytes([9; 16]),
            &parent,
            "child",
            |_| ()
        )
        .is_err()
    );
    assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    fs::write(output.path().join("not-directory"), b"keep").unwrap();
    let file = File::open(output.path().join("not-directory")).unwrap();
    assert!(with_restored_file_archive_at(&bytes, PROJECT, &file, "child", |_| ()).is_err());
    let maximal = "x".repeat(255);
    with_restored_file_archive_at(&bytes, PROJECT, &parent, &maximal, |_| ()).unwrap();
    assert_released(&output.path().join(maximal));
}

#[test]
fn existing_file_directory_and_alias_are_preserved_without_coordination() {
    let _serial = TEST_IO.lock().unwrap();
    let source = private_temp();
    let (bytes, _) = capture(source.path(), false, b"no-replace");
    for kind in 0..3 {
        let output = private_temp();
        let parent = File::open(output.path()).unwrap();
        let target = output.path().join("child");
        match kind {
            0 => fs::write(&target, b"existing file").unwrap(),
            1 => {
                private_directory(&target);
                fs::write(target.join("keep"), b"existing directory").unwrap();
            }
            _ => std::os::unix::fs::symlink(source.path(), &target).unwrap(),
        }
        assert!(
            with_restored_file_archive_at(&bytes, PROJECT, &parent, "child", |_| panic!(
                "existing target ran work"
            ))
            .is_err()
        );
        match kind {
            0 => assert_eq!(fs::read(&target).unwrap(), b"existing file"),
            1 => assert_eq!(
                fs::read(target.join("keep")).unwrap(),
                b"existing directory"
            ),
            _ => assert_eq!(fs::read_link(&target).unwrap(), source.path()),
        }
    }
}

#[derive(Debug)]
struct Receipt(Arc<AtomicUsize>);
impl Drop for Receipt {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn same_byte_substitutions_corruption_topology_and_privacy_withhold_callback_values() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for change in 0..18 {
            let source = private_temp();
            let output = private_temp();
            let (bytes, wal) = capture(source.path(), compacted, b"original-payload");
            let parent = File::open(output.path()).unwrap();
            let child = output.path().join("child");
            let saved = output.path().join("original-saved");
            let drops = Arc::new(AtomicUsize::new(0));
            let result = with_restored_file_archive_at(&bytes, PROJECT, &parent, "child", |_| {
                assert_owned(&child);
                let path = match change {
                    0 => child.clone(),
                    1 => child.join("metadata"),
                    2 => child.join("objects"),
                    3 | 7 | 8 | 12 | 14 | 16 => child.join("metadata/redo.wal"),
                    4 | 13 | 15 | 17 => child.join("objects").join(format!("{OBJECT}.object")),
                    5 => child.join("objects").join(format!("{ORPHAN}.object")),
                    _ => child.join("objects/.emilybase-objects"),
                };
                match change {
                    0..=2 => {
                        fs::rename(&path, &saved).unwrap();
                        private_directory(&path);
                        fs::write(path.join("keep"), b"foreign directory").unwrap();
                    }
                    3..=6 => {
                        fs::rename(&path, &saved).unwrap();
                        fs::copy(&saved, &path).unwrap();
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
                        assert!(descriptors(&saved) > 0);
                    }
                    7 => fs::write(&path, b"changed original WAL").unwrap(),
                    8 => {
                        File::options()
                            .append(true)
                            .open(&path)
                            .unwrap()
                            .write_all(b"uncommitted tail")
                            .unwrap();
                    }
                    9 => fs::set_permissions(&child, fs::Permissions::from_mode(0o755)).unwrap(),
                    10 => fs::set_permissions(output.path(), fs::Permissions::from_mode(0o755))
                        .unwrap(),
                    11 => fs::write(child.join("unexpected"), b"foreign inventory member").unwrap(),
                    12 | 13 => fs::hard_link(&path, &saved).unwrap(),
                    14 | 15 => {
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap()
                    }
                    _ => {
                        fs::rename(&path, &saved).unwrap();
                        std::os::unix::fs::symlink(&saved, &path).unwrap();
                        assert!(descriptors(&saved) > 0);
                    }
                }
                Receipt(Arc::clone(&drops))
            });
            assert!(
                matches!(result, Err(Error::OutcomeUnknown(_))),
                "change={change}, result={result:?}"
            );
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            assert!(child.exists());
            if change <= 2 {
                assert_eq!(
                    fs::read(child.join(if change == 0 {
                        "keep"
                    } else if change == 1 {
                        "metadata/keep"
                    } else {
                        "objects/keep"
                    }))
                    .unwrap(),
                    b"foreign directory"
                );
                assert!(saved.is_dir());
            } else if change <= 6 {
                assert_eq!(descriptors(&saved), 0);
                assert_eq!(
                    fs::read(&saved).unwrap(),
                    fs::read(&match change {
                        3 => child.join("metadata/redo.wal"),
                        4 => child.join("objects").join(format!("{OBJECT}.object")),
                        5 => child.join("objects").join(format!("{ORPHAN}.object")),
                        _ => child.join("objects/.emilybase-objects"),
                    })
                    .unwrap()
                );
            } else if matches!(change, 12 | 13 | 16 | 17) {
                assert_eq!(descriptors(&saved), 0);
                assert!(saved.is_file());
            }
            assert_eq!(
                fs::read(source.path().join("metadata/redo.wal")).unwrap(),
                wal
            );
        }
    }
}

#[test]
fn trusted_callback_unwinding_releases_all_original_owners_and_preserves_selected_child() {
    let _serial = TEST_IO.lock().unwrap();
    let source = private_temp();
    let output = private_temp();
    let (bytes, wal) = capture(source.path(), true, b"unwind-safe-owner-release");
    let parent = File::open(output.path()).unwrap();
    let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = with_restored_file_archive_at(&bytes, PROJECT, &parent, "child", |_| {
            assert_owned(&output.path().join("child"));
            panic!("synthetic trusted callback unwind");
        });
    }));
    assert!(unwind.is_err());
    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
    assert_released(&output.path().join("child"));
    assert_eq!(
        fs::read(output.path().join("child/metadata/redo.wal")).unwrap(),
        wal
    );
    assert_eq!(
        fs::read(source.path().join("metadata/redo.wal")).unwrap(),
        wal
    );
}

#[test]
fn a_prepared_copy_has_independent_locks_history_and_future_native_writes() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let source_temp = private_temp();
        let output = private_temp();
        let mut original = source(source_temp.path(), compacted, b"independent-source");
        let bytes = encoded(&mut original);
        let wal = original.database.committed_wal().unwrap();
        let parent = File::open(output.path()).unwrap();
        with_restored_file_archive_at(&bytes, PROJECT, &parent, "child", |_| {
            assert_owned(&output.path().join("child"));
            let info = original.info(FILE).unwrap().unwrap();
            original
                .rename(FILE, info.revision(), "source-only-change")
                .unwrap();
        })
        .unwrap();
        let mut copy = open(&output.path().join("child")).unwrap();
        assert_eq!(copy.database.committed_wal().unwrap(), wal);
        assert_eq!(
            copy.info(FILE).unwrap().unwrap().name(),
            "synthetic-display"
        );
        assert_eq!(
            original.info(FILE).unwrap().unwrap().name(),
            "source-only-change"
        );
        let info = copy.info(FILE).unwrap().unwrap();
        copy.rename(FILE, info.revision(), "copy-only-change")
            .unwrap();
        assert_eq!(
            original.info(FILE).unwrap().unwrap().name(),
            "source-only-change"
        );
    }
}

#[test]
fn inner_work_cannot_substitute_an_earlier_prepared_owner_during_common_publication() {
    let _serial = TEST_IO.lock().unwrap();
    let source = private_temp();
    let output = private_temp();
    let (bytes, _) = capture(source.path(), true, b"nested-history");
    let target = output.path().join("common");
    let stage = StagedPrivateDirectory::new(&target).unwrap();
    let root = stage.directory().try_clone().unwrap();
    let saved = output.path().join("original-wal");
    let result = with_restored_file_archive_at(&bytes, PROJECT, &root, "first", |_| {
        with_restored_file_archive_at(&bytes, PROJECT, &root, "second", |_| {
            let selected = stage.publish().unwrap();
            selected.check().unwrap();
            let wal = target.join("first/metadata/redo.wal");
            fs::rename(&wal, &saved).unwrap();
            fs::copy(&saved, &wal).unwrap();
            fs::set_permissions(&wal, fs::Permissions::from_mode(0o600)).unwrap();
            assert!(descriptors(&saved) > 0);
            assert_owned(&target.join("second"));
        })
    });
    assert!(matches!(result, Err(Error::OutcomeUnknown(_))));
    assert!(target.is_dir());
    assert_eq!(descriptors(&saved), 0);
    assert_released(&target.join("second"));
    assert_eq!(
        fs::read(saved).unwrap(),
        fs::read(target.join("first/metadata/redo.wal")).unwrap()
    );
}

#[test]
fn failed_enclosing_no_replace_returns_application_error_and_preserves_both_roots() {
    let _serial = TEST_IO.lock().unwrap();
    let source = private_temp();
    let output = private_temp();
    let (bytes, _) = capture(source.path(), false, b"common-collision");
    let target = output.path().join("common");
    private_directory(&target);
    fs::write(target.join("keep"), b"existing common destination").unwrap();
    let stage = StagedPrivateDirectory::new(&target).unwrap();
    let root = stage.directory().try_clone().unwrap();
    let path = descriptor_path(&root).join("child");
    let result =
        with_restored_file_archive_at(&bytes, PROJECT, &root, "child", |_| stage.publish());
    assert!(result.unwrap().is_err());
    assert_released(&path);
    assert_eq!(
        fs::read(target.join("keep")).unwrap(),
        b"existing common destination"
    );
    assert!(path.join("metadata/redo.wal").is_file());
}

#[test]
fn maximum_orphan_inventory_retains128_actual_descriptors_across_native_work() {
    let _serial = TEST_IO.lock().unwrap();
    let source = private_temp();
    let output = private_temp();
    let db = Database::create(source.path().join("metadata")).unwrap();
    private_directory(&source.path().join("objects"));
    let objects = ProjectDirectory::initialize(source.path().join("objects"), PROJECT).unwrap();
    let mut store = FileStore::initialize(db, objects, FileQuota::new(128, 0).unwrap()).unwrap();
    for n in 0..128 {
        store
            .objects
            .put(ObjectId::from_bytes([n; 16]), b"")
            .unwrap();
    }
    let bytes = encoded(&mut store);
    let parent = File::open(output.path()).unwrap();
    let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
    with_restored_file_archive_at(&bytes, PROJECT, &parent, "child", |report| {
        assert_eq!(report.objects().objects, 128);
        assert_eq!(report.references(), 0);
        for n in 0..128 {
            assert!(
                descriptors(
                    &output
                        .path()
                        .join("child/objects")
                        .join(format!("{}.object", ObjectId::from_bytes([n; 16])))
                ) > 0
            );
        }
        assert!(Database::open(output.path().join("child/metadata")).is_err());
        assert!(ProjectDirectory::open(output.path().join("child/objects"), PROJECT).is_err());
    })
    .unwrap();
    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
    let restored = open(&output.path().join("child")).unwrap();
    assert_eq!(restored.usage().unwrap().orphans, 128);
}

pub(super) fn worker(parent: &Path, bytes: &[u8], phase: &str) {
    let ready = || {
        println!("EB_FILE_RESTORE_READY");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    };
    let target = parent.join("restored");
    let stage = StagedPrivateDirectory::new(&target).unwrap();
    let root = stage.directory().try_clone().unwrap();
    with_restored_file_archive_at(bytes, PROJECT, &root, "child", |_| {
        assert_owned(&descriptor_path(&root).join("child"));
        if phase == "coordinated-prepared" {
            ready();
        }
        let selected = stage.publish().unwrap();
        selected.check().unwrap();
        assert_owned(&target.join("child"));
        if phase == "coordinated-selected" {
            ready();
        }
    })
    .unwrap();
    assert_eq!(phase, "coordinated-returned");
    assert_released(&target.join("child"));
    ready();
}

#[test]
fn kills_during_coordination_preserve_private_stage_or_complete_selected_common_root() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for empty in [false, true] {
            for phase in [
                "coordinated-prepared",
                "coordinated-selected",
                "coordinated-returned",
            ] {
                let source = private_temp();
                let output = private_temp();
                let payload = if empty { vec![] } else { vec![0x38; 8193] };
                let (bytes, wal) = capture(source.path(), compacted, &payload);
                fs::write(output.path().join("archive"), &bytes).unwrap();
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "archive::restore::tests::worker",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("EMILYBASE_FILE_RESTORE_TEST_PARENT", output.path())
                    .env("EMILYBASE_FILE_RESTORE_TEST_PHASE", phase)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let stdout = child.stdout.take().unwrap();
                let (send, receive) = std::sync::mpsc::channel();
                let reader = std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines() {
                        if line.unwrap() == "EB_FILE_RESTORE_READY" {
                            send.send(()).unwrap();
                            return;
                        }
                    }
                });
                let reached = receive.recv_timeout(std::time::Duration::from_secs(30));
                let _ = child.kill();
                let status = child.wait().unwrap();
                reader.join().unwrap();
                let mut stderr = String::new();
                child
                    .stderr
                    .take()
                    .unwrap()
                    .read_to_string(&mut stderr)
                    .unwrap();
                assert!(reached.is_ok(), "phase={phase}, stderr={stderr}");
                assert_eq!(status.signal(), Some(9));
                let target = output.path().join("restored");
                let selected = phase != "coordinated-prepared";
                assert_eq!(target.exists(), selected);
                let path = if selected {
                    target
                } else {
                    stage(output.path())
                };
                let restored_path = path.join("child");
                let mut restored = open(&restored_path).unwrap();
                assert_eq!(restored.database.committed_wal().unwrap(), wal);
                assert_eq!(restored.usage().unwrap().orphans, 1);
                let snapshot = restored.capture().unwrap();
                assert_eq!(
                    snapshot
                        .objects()
                        .objects()
                        .iter()
                        .find(|o| o.object() == OBJECT)
                        .unwrap()
                        .payload(),
                    payload
                );
                let info = restored.info(FILE).unwrap().unwrap();
                restored
                    .rename(FILE, info.revision(), "after-coordination-kill")
                    .unwrap();
                assert_eq!(
                    fs::read(source.path().join("metadata/redo.wal")).unwrap(),
                    wal
                );
                assert_eq!(fs::read(output.path().join("archive")).unwrap(), bytes);
                let retry = output.path().join("retry");
                restore_file_archive(&bytes, PROJECT, &retry).unwrap();
                assert_eq!(fs::read(retry.join("metadata/redo.wal")).unwrap(), wal);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_coordinated_restore_matches_independent_history_and_payload_model(
        compacted in any::<bool>(), payload in prop::collection::vec(any::<u8>(), 0..4097), removed in any::<bool>()
    ) {
        let _serial = TEST_IO.lock().unwrap();
        let source_temp = private_temp(); let output = private_temp();
        let mut store = source(source_temp.path(), compacted, &payload);
        let info = store.info(FILE).unwrap().unwrap();
        if removed { store.remove(FILE, info.revision()).unwrap(); }
        let wal = store.database.committed_wal().unwrap(); let bytes = encoded(&mut store);
        let target = output.path().join("common"); let stage = StagedPrivateDirectory::new(&target).unwrap(); let root = stage.directory().try_clone().unwrap();
        let report = with_restored_file_archive_at(&bytes, PROJECT, &root, "child", |report| {
            assert_owned(&descriptor_path(&root).join("child"));
            let selected = stage.publish().unwrap(); selected.check().unwrap(); assert_owned(&target.join("child")); report.clone()
        }).unwrap();
        assert_released(&target.join("child")); let mut restored = open(&target.join("child")).unwrap();
        prop_assert_eq!(restored.database.committed_wal().unwrap(), wal);
        prop_assert_eq!(report.references(), usize::from(!removed));
        prop_assert_eq!(restored.usage().unwrap().orphans, 1 + usize::from(removed));
        prop_assert_eq!(restored.usage().unwrap().payload_bytes, payload.len() as u64 + 6);
        prop_assert_eq!(restored.info(FILE).unwrap(), if removed { None } else { Some(info) });
        let image = restored.capture().unwrap(); prop_assert_eq!(image.objects().objects().iter().find(|o| o.object() == OBJECT).unwrap().payload(), payload);
    }
}
