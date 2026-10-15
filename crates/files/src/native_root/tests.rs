use super::*;
use crate::{FileArchiveReader, FileId, TEST_IO, initialize_file_root, restore_file_archive};
use emilybase_object_storage::ObjectId;
use proptest::prelude::*;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
use std::process::{Command, Stdio};

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FILE: FileId = FileId::from_bytes([2; 16]);
mod at;
mod coordinated;
fn fixture(parent: &Path, compacted: bool, payload: &[u8]) -> (PathBuf, Vec<u8>, FileInfo) {
    let path = parent.join("root");
    initialize_file_root(&path, PROJECT, FileQuota::new(8, 32768).unwrap()).unwrap();
    let mut database = Database::open(path.join("metadata")).unwrap();
    if compacted {
        database.compact().unwrap();
    }
    let objects = ProjectDirectory::open(path.join("objects"), PROJECT).unwrap();
    let mut store = FileStore::open(database, objects).unwrap();
    let info = store
        .publish(
            FILE,
            ObjectId::from_bytes([3; 16]),
            [4; 16],
            "private-display",
            payload,
        )
        .unwrap();
    store
        .objects
        .put(ObjectId::from_bytes([5; 16]), b"orphan")
        .unwrap();
    let state = store.quota_state().unwrap();
    store
        .set_quota(state, FileQuota::new(2, payload.len() as u64 + 6).unwrap())
        .unwrap();
    let bytes = store.database.committed_wal().unwrap();
    (path, bytes, info)
}
fn encoded(snapshot: &FileSnapshot) -> Vec<u8> {
    let mut bytes = Vec::new();
    FileArchiveReader::from_snapshot(snapshot)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}
fn copy_pair(from: &Path, to: &Path) {
    fs::DirBuilder::new().mode(0o700).create(to).unwrap();
    for child in ["metadata", "objects"] {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(to.join(child))
            .unwrap();
        for entry in fs::read_dir(from.join(child)).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), to.join(child).join(entry.file_name())).unwrap();
        }
    }
}
fn change(path: &Path, saved: &Path, kind: usize) {
    match kind {
        0 => {
            fs::rename(path, saved).unwrap();
            copy_pair(saved, path);
        }
        1 | 2 => {
            let child = if kind == 1 { "metadata" } else { "objects" };
            fs::rename(path.join(child), saved).unwrap();
            fs::DirBuilder::new()
                .mode(0o700)
                .create(path.join(child))
                .unwrap();
            for entry in fs::read_dir(saved).unwrap() {
                let entry = entry.unwrap();
                fs::copy(entry.path(), path.join(child).join(entry.file_name())).unwrap();
            }
        }
        3 | 4 => {
            let file = path.join(if kind == 3 {
                "metadata/redo.wal"
            } else {
                "objects/.emilybase-objects"
            });
            fs::rename(&file, saved).unwrap();
            fs::copy(saved, file).unwrap();
        }
        5 => fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap(),
        _ => {
            fs::write(path.join("unexpected"), b"foreign entry").unwrap();
        }
    }
}

#[test]
fn original_root_owners_span_metadata_and_capture_with_exact_history_quota_and_orphans() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for empty in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let payload = if empty { vec![] } else { vec![0x53; 8193] };
            let (path, wal, info) = fixture(temp.path(), compacted, &payload);
            let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
            let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
            let probe = File::open(path.join("metadata")).unwrap();
            assert!(matches!(
                probe.try_lock(),
                Err(std::fs::TryLockError::WouldBlock)
            ));
            drop(probe);
            assert!(matches!(
                ProjectDirectory::open(path.join("objects"), PROJECT),
                Err(emilybase_object_storage::Error::Busy)
            ));
            let observed = root.inspect().unwrap();
            assert_eq!(observed.project(), PROJECT);
            assert_eq!(observed.revision(), 4);
            assert_eq!(
                observed.quota(),
                FileQuota::new(2, payload.len() as u64 + 6).unwrap()
            );
            assert_eq!(
                observed.usage(),
                FileUsage {
                    physical_objects: 2,
                    payload_bytes: payload.len() as u64 + 6,
                    references: 1,
                    orphans: 1
                }
            );
            assert!(!format!("{observed:?}").contains("private-display"));
            assert_eq!(root.list().unwrap(), vec![info]);
            let snapshot = root.capture().unwrap();
            assert_eq!(
                snapshot.metadata_report().wal_version,
                if compacted { 2 } else { 1 }
            );
            assert_eq!(
                snapshot.metadata_report().database_id,
                *observed.database_id()
            );
            assert_eq!(
                &snapshot.metadata_bytes()[emilybase_backup::HEADER_SIZE..],
                wal
            );
            drop(root);
            assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
            let copy = temp.path().join("copy");
            restore_file_archive(&encoded(&snapshot), PROJECT, &copy).unwrap();
            assert_eq!(
                NativeFileRoot::open(&copy, PROJECT)
                    .unwrap()
                    .inspect()
                    .unwrap(),
                observed
            );
            assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
        }
    }
}

#[test]
fn root_child_wal_and_scope_identity_changes_refuse_at_every_owner_admission_boundary() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for phase in [
            OpenBoundary::Selected,
            OpenBoundary::Metadata,
            OpenBoundary::Owned,
        ] {
            for kind in 0..7 {
                let temp = tempfile::tempdir().unwrap();
                let (path, wal, _) = fixture(temp.path(), compacted, b"payload");
                let saved = temp.path().join("saved");
                let result = NativeFileRoot::open_with(&path, PROJECT, |at| {
                    if at == phase {
                        change(&path, &saved, kind);
                    }
                });
                assert!(result.is_err(), "kind={kind}");
                let original = if kind == 0 {
                    saved.join("metadata/redo.wal")
                } else if kind == 1 {
                    saved.join("redo.wal")
                } else if kind == 3 {
                    saved
                } else {
                    path.join("metadata/redo.wal")
                };
                assert_eq!(fs::read(original).unwrap(), wal);
            }
        }
    }
}

#[test]
fn path_selection_changes_around_reads_return_no_snapshot_and_poison_until_reopen() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for phase in [ReadBoundary::Admitted, ReadBoundary::Copied] {
            for kind in 0..7 {
                let temp = tempfile::tempdir().unwrap();
                let (path, _, _) = fixture(temp.path(), compacted, b"payload");
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                let result = root.observe(
                    |store, _| store.capture(),
                    |at| {
                        if at == phase {
                            change(&path, &temp.path().join("saved"), kind);
                        }
                    },
                );
                assert!(result.is_err(), "kind={kind}");
                assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                drop(root);
            }
        }
    }
}

#[test]
fn invalid_scope_root_mode_roster_and_aliases_preserve_every_existing_entry() {
    let _serial = TEST_IO.lock().unwrap();
    for kind in 0..6 {
        let temp = tempfile::tempdir().unwrap();
        let (path, wal, _) = fixture(temp.path(), false, b"payload");
        let mut project = PROJECT;
        let mut input = path.clone();
        match kind {
            0 => project = ProjectId::from_bytes([9; 16]),
            1 => fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap(),
            2 => {
                input = temp.path().join("alias");
                symlink(&path, &input).unwrap();
            }
            3 => {
                fs::write(path.join("unexpected"), b"protected").unwrap();
            }
            4 => {
                fs::rename(path.join("objects"), temp.path().join("saved")).unwrap();
                symlink(temp.path().join("saved"), path.join("objects")).unwrap();
            }
            _ => {
                fs::rename(path.join("metadata"), temp.path().join("saved")).unwrap();
                symlink(temp.path().join("saved"), path.join("metadata")).unwrap();
            }
        }
        let before = fs::read_dir(temp.path()).unwrap().count();
        assert!(NativeFileRoot::open(input, project).is_err());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), before);
        assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
    }
}

#[test]
fn moved_parent_never_adopts_its_identical_replacement_during_admission_or_observation() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for phase in 0..5 {
            let container = tempfile::tempdir().unwrap();
            let parent = container.path().join("parent");
            let moved = container.path().join("moved");
            fs::create_dir(&parent).unwrap();
            let (path, wal, _) = fixture(&parent, compacted, b"payload");
            let substitute = || {
                fs::rename(&parent, &moved).unwrap();
                fs::create_dir(&parent).unwrap();
                copy_pair(&moved.join("root"), &path);
            };
            if phase < 3 {
                let target = [
                    OpenBoundary::Selected,
                    OpenBoundary::Metadata,
                    OpenBoundary::Owned,
                ][phase];
                let result = NativeFileRoot::open_with(&path, PROJECT, |at| {
                    if at == target {
                        substitute();
                    }
                });
                assert!(result.is_err());
            } else {
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                let target = if phase == 3 {
                    ReadBoundary::Admitted
                } else {
                    ReadBoundary::Copied
                };
                let result = root.observe(
                    |store, _| store.capture(),
                    |at| {
                        if at == target {
                            substitute();
                        }
                    },
                );
                assert!(result.is_err());
                assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                drop(root);
            }
            assert_eq!(fs::read(moved.join("root/metadata/redo.wal")).unwrap(), wal);
            assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
            let original = NativeFileRoot::open(moved.join("root"), PROJECT)
                .unwrap()
                .inspect()
                .unwrap();
            let replacement = NativeFileRoot::open(&path, PROJECT)
                .unwrap()
                .inspect()
                .unwrap();
            assert_eq!(original, replacement);
        }
    }
}

#[test]
fn readonly_observations_preserve_abandoned_wal_tail_and_capture_only_acknowledged_prefix() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (path, wal, info) = fixture(temp.path(), false, b"payload");
    let mut store = FileStore::open(
        Database::open(path.join("metadata")).unwrap(),
        ProjectDirectory::open(path.join("objects"), PROJECT).unwrap(),
    )
    .unwrap();
    store
        .rename(FILE, info.revision(), "unacknowledged-test-tail")
        .unwrap();
    let newer = store.database.committed_wal().unwrap();
    drop(store);
    let mut physical = wal.clone();
    physical.extend_from_slice(&newer[wal.len()..wal.len() + 17]);
    fs::write(path.join("metadata/redo.wal"), &physical).unwrap();
    let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
    assert_eq!(root.inspect().unwrap().revision(), 4);
    assert_eq!(root.list().unwrap(), vec![info]);
    assert_eq!(
        &root.capture().unwrap().metadata_bytes()[emilybase_backup::HEADER_SIZE..],
        wal
    );
    drop(root);
    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), physical);
}

#[test]
#[ignore = "native common-root reader helper invoked by parent kill test"]
fn worker() {
    let Some(parent) = std::env::var_os("EMILYBASE_NATIVE_FILE_ROOT_TEST_PARENT") else {
        return;
    };
    let parent = PathBuf::from(parent);
    let phase = std::env::var("EMILYBASE_NATIVE_FILE_ROOT_TEST_PHASE").unwrap();
    let ready = || -> ! {
        println!("EB_NATIVE_FILE_ROOT_READY");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    };
    let boundary = |at| {
        if matches!(
            (phase.as_str(), at),
            ("selected", OpenBoundary::Selected)
                | ("metadata", OpenBoundary::Metadata)
                | ("owned", OpenBoundary::Owned)
        ) {
            ready();
        }
    };
    let mut root = if std::env::var("EMILYBASE_NATIVE_FILE_ROOT_TEST_AT").as_deref() == Ok("1") {
        let original = File::open(&parent).unwrap();
        original.try_lock().unwrap();
        NativeFileRoot::open_at_with(&original, "root", PROJECT, boundary)
    } else {
        NativeFileRoot::open_with(&parent.join("root"), PROJECT, boundary)
    }
    .unwrap();
    root.inspect().unwrap();
    if matches!(phase.as_str(), "coordinating" | "coordinated") {
        let snapshot = root
            .with_snapshot(|snapshot| {
                if phase == "coordinating" {
                    ready();
                }
                snapshot
            })
            .unwrap();
        assert_eq!(snapshot.files().len(), 1);
        ready();
    }
    root.capture().unwrap();
    assert_eq!(phase, "ack");
    ready();
}

#[test]
fn killed_native_readers_release_both_owners_without_changing_committed_source_pair() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for empty in [false, true] {
            for phase in ["metadata", "owned", "ack"] {
                let temp = tempfile::tempdir().unwrap();
                let payload = if empty { vec![] } else { vec![0x53; 8193] };
                let (path, wal, info) = fixture(temp.path(), compacted, &payload);
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "native_root::tests::worker",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("EMILYBASE_NATIVE_FILE_ROOT_TEST_PARENT", temp.path())
                    .env("EMILYBASE_NATIVE_FILE_ROOT_TEST_PHASE", phase)
                    .env_remove("EMILYBASE_NATIVE_FILE_ROOT_TEST_AT")
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
                child.kill().unwrap();
                let status = child.wait().unwrap();
                thread.join().unwrap();
                assert!(ready.is_ok());
                assert_eq!(status.signal(), Some(9));
                assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                assert_eq!(root.list().unwrap(), vec![info]);
                assert_eq!(root.inspect().unwrap().usage().orphans, 1);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_native_observations_and_snapshot_restore_match_payload_and_charge_model(compacted in any::<bool>(),payload in prop::collection::vec(any::<u8>(),0..2049)) {
        let _serial=TEST_IO.lock().unwrap();let temp=tempfile::tempdir().unwrap();let (path,wal,info)=fixture(temp.path(),compacted,&payload);
        let mut root=NativeFileRoot::open(&path,PROJECT).unwrap();prop_assert_eq!(root.list().unwrap(),vec![info]);let observed=root.inspect().unwrap();prop_assert_eq!(observed.usage().payload_bytes,payload.len() as u64+6);
        let snapshot=root.capture().unwrap();let copy=temp.path().join("copy");restore_file_archive(&encoded(&snapshot),PROJECT,&copy).unwrap();let mut restored=NativeFileRoot::open(&copy,PROJECT).unwrap();prop_assert_eq!(restored.inspect().unwrap(),observed);
        let captured=restored.capture().unwrap();prop_assert_eq!(captured.objects().objects().iter().find(|o|o.object()==ObjectId::from_bytes([3;16])).unwrap().payload(),payload);
        prop_assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(),wal);
    }
}
