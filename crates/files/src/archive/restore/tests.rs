use super::*;
use crate::{FileArchiveReader, FileId, FileQuota, TEST_IO, publish_file_archive};
use emilybase_object_storage::ObjectId;
use proptest::prelude::*;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::DirBuilderExt;
use std::process::{Command, Stdio};
const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FILE: FileId = FileId::from_bytes([2; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([3; 16]);
const ORPHAN: ObjectId = ObjectId::from_bytes([5; 16]);
fn source(path: &Path, compacted: bool, payload: &[u8]) -> FileStore {
    let mut db = Database::create(path.join("metadata")).unwrap();
    if compacted {
        db.compact().unwrap();
    }
    fs::DirBuilder::new()
        .mode(0o700)
        .create(path.join("objects"))
        .unwrap();
    let objects = ProjectDirectory::initialize(path.join("objects"), PROJECT).unwrap();
    let mut store = FileStore::initialize(db, objects, FileQuota::new(8, 32768).unwrap()).unwrap();
    store
        .publish(FILE, OBJECT, [4; 16], "synthetic-display", payload)
        .unwrap();
    store.objects.put(ORPHAN, b"orphan").unwrap();
    store
}
fn encoded(store: &mut FileStore) -> Vec<u8> {
    let snapshot = store.capture().unwrap();
    let mut bytes = Vec::new();
    FileArchiveReader::from_snapshot(&snapshot)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}
fn open(path: &Path) -> Result<FileStore> {
    FileStore::open(
        Database::open(path.join("metadata"))?,
        ProjectDirectory::open(path.join("objects"), PROJECT)?,
    )
}
fn stage(parent: &Path) -> PathBuf {
    let entries = fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".emilybase-directory-")
        })
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    entries[0].clone()
}

#[test]
fn common_root_round_trip_keeps_exact_identity_history_graph_orphans_quota_and_next_write() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for empty in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let destination = tempfile::tempdir().unwrap();
            let payload = if empty { vec![] } else { vec![0x53; 8193] };
            let mut store = source(temp.path(), compacted, &payload);
            let info = store.info(FILE).unwrap().unwrap();
            let state = store.quota_state().unwrap();
            let quota = FileQuota::new(2, payload.len() as u64 + 6).unwrap();
            store.set_quota(state, quota).unwrap();
            let snapshot = store.capture().unwrap();
            let backup = destination.path().join("archive");
            let expected = publish_file_archive(&snapshot, &backup).unwrap();
            let bytes = fs::read(&backup).unwrap();
            let before = store.database.committed_wal().unwrap();
            drop(store);
            fs::remove_dir_all(temp.path().join("metadata")).unwrap();
            fs::remove_dir_all(temp.path().join("objects")).unwrap();
            let target = destination.path().join("restored");
            let report = restore_file_archive(&bytes, PROJECT, &target).unwrap_or_else(|error| {
                let mut chain = Vec::new();
                let mut source: Option<&dyn std::error::Error> = Some(&error);
                while let Some(error) = source {
                    chain.push(error.to_string());
                    source = error.source();
                }
                panic!("synthetic restore error chain: {chain:?}");
            });
            assert_eq!(report, expected);
            assert_eq!(fs::metadata(&target).unwrap().mode() & 0o777, 0o700);
            assert_eq!(fs::read_dir(&target).unwrap().count(), 2);
            let mut restored = open(&target).unwrap();
            assert_eq!(restored.database.committed_wal().unwrap(), before);
            assert_eq!(restored.quota().unwrap(), quota);
            assert_eq!(restored.list().unwrap(), vec![info.clone()]);
            assert_eq!(restored.usage().unwrap().orphans, 1);
            assert_eq!(restored.usage().unwrap().physical_objects, 2);
            let mut reader = restored.reader(FILE).unwrap();
            let mut copy = Vec::new();
            let mut scratch = [0; 8192];
            loop {
                let n = reader.read_payload(&mut scratch).unwrap();
                if n == 0 {
                    break;
                }
                copy.extend_from_slice(&scratch[..n]);
            }
            reader.finish().unwrap();
            assert_eq!(copy, payload);
            restored
                .rename(FILE, info.revision(), "independent-restored-write")
                .unwrap();
            drop(restored);
            let mut restored = open(&target).unwrap();
            assert_eq!(
                restored.info(FILE).unwrap().unwrap().name(),
                "independent-restored-write"
            );
            let persisted = restored.database.committed_wal().unwrap();
            drop(restored);
            assert!(restore_file_archive(&bytes, PROJECT, &target).is_err());
            assert_eq!(
                open(&target).unwrap().database.committed_wal().unwrap(),
                persisted
            );
        }
    }
}

#[test]
fn invalid_whole_pair_or_wrong_project_creates_no_stage_or_destination() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let mut store = source(temp.path(), false, b"synthetic");
    let bytes = encoded(&mut store);
    for bad in [
        &bytes[..191],
        &bytes[..bytes.len() - 1],
        b"invalid".as_slice(),
    ] {
        assert!(restore_file_archive(bad, PROJECT, target.path().join("restored")).is_err());
        assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
    }
    assert!(
        restore_file_archive(
            &bytes,
            ProjectId::from_bytes([9; 16]),
            target.path().join("restored")
        )
        .is_err()
    );
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
}

#[test]
#[cfg(target_os = "linux")]
fn maximum_all_orphan_restore_holds128_actual_blob_descriptors_and_both_locks_until_selection() {
    let _serial = TEST_IO.lock().unwrap();
    let source_temp = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let db = Database::create(source_temp.path().join("metadata")).unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(source_temp.path().join("objects"))
        .unwrap();
    let objects =
        ProjectDirectory::initialize(source_temp.path().join("objects"), PROJECT).unwrap();
    let mut store = FileStore::initialize(db, objects, FileQuota::new(128, 0).unwrap()).unwrap();
    for index in 0..128 {
        store
            .objects
            .put(ObjectId::from_bytes([index; 16]), b"")
            .unwrap();
    }
    let bytes = encoded(&mut store);
    let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
    let mut held = 0;
    let target = destination.path().join("restored");
    let report = restore_with(&bytes, PROJECT, &target, |at| {
        if at == RestoreBoundary::Owned {
            let root = stage(destination.path());
            let objects = root.join("objects");
            held = fs::read_dir("/proc/self/fd")
                .unwrap()
                .filter_map(|e| fs::read_link(e.unwrap().path()).ok())
                .filter(|p| {
                    p.parent() == Some(objects.as_path())
                        && p.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .ends_with(".object")
                })
                .count();
            assert!(Database::open(root.join("metadata")).is_err());
            assert!(matches!(
                ProjectDirectory::open(&objects, PROJECT),
                Err(emilybase_object_storage::Error::Busy)
            ));
        }
    })
    .unwrap();
    assert_eq!(held, 128);
    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
    assert_eq!(report.references(), 0);
    assert_eq!(report.objects().objects, 128);
    assert_eq!(report.objects().payload_bytes, 0);
    let restored = open(&target).unwrap();
    assert_eq!(restored.usage().unwrap().orphans, 128);
    assert_eq!(restored.quota().unwrap(), FileQuota::new(128, 0).unwrap());
}

#[test]
fn original_restored_children_wal_scope_and_every_actual_blob_span_common_selection() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for selected in [false, true] {
            for change in 0..8 {
                let source_temp = tempfile::tempdir().unwrap();
                let target_temp = tempfile::tempdir().unwrap();
                let mut store = source(source_temp.path(), compacted, b"synthetic");
                let bytes = encoded(&mut store);
                let destination = target_temp.path().join("restored");
                let saved = target_temp.path().join("actual-original");
                let result = restore_with(&bytes, PROJECT, &destination, |at| {
                    if at
                        != if selected {
                            RestoreBoundary::Selected
                        } else {
                            RestoreBoundary::Owned
                        }
                    {
                        return;
                    }
                    let root = if selected {
                        destination.clone()
                    } else {
                        stage(target_temp.path())
                    };
                    match change {
                        0 | 1 => {
                            let path = root.join(if change == 0 { "metadata" } else { "objects" });
                            fs::rename(&path, &saved).unwrap();
                            fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
                            for entry in fs::read_dir(&saved).unwrap() {
                                let entry = entry.unwrap();
                                fs::copy(entry.path(), path.join(entry.file_name())).unwrap();
                            }
                        }
                        2 | 4 | 5 | 6 => {
                            let path = if change == 2 {
                                root.join("metadata/redo.wal")
                            } else if change == 6 {
                                fs::read_dir(root.join("objects"))
                                    .unwrap()
                                    .map(|e| e.unwrap().path())
                                    .find(|p| {
                                        p.file_name().unwrap()
                                            != format!("{OBJECT}.object").as_str()
                                            && p.file_name().unwrap()
                                                != format!("{ORPHAN}.object").as_str()
                                    })
                                    .unwrap()
                            } else {
                                root.join("objects").join(format!(
                                    "{}.object",
                                    if change == 4 { OBJECT } else { ORPHAN }
                                ))
                            };
                            fs::rename(&path, &saved).unwrap();
                            fs::copy(&saved, &path).unwrap();
                        }
                        3 => {
                            fs::write(root.join("metadata/redo.wal"), b"changed-original-wal")
                                .unwrap();
                        }
                        _ => {
                            fs::write(root.join("unexpected"), b"not-an-admitted-root-entry")
                                .unwrap();
                        }
                    }
                });
                if selected {
                    assert!(
                        matches!(result, Err(Error::OutcomeUnknown(_))),
                        "change={change}"
                    );
                    assert!(destination.is_dir());
                } else {
                    assert!(result.is_err());
                    assert!(!matches!(result, Err(Error::OutcomeUnknown(_))));
                    assert!(!destination.exists());
                    assert!(stage(target_temp.path()).is_dir());
                }
                assert_eq!(store.info(FILE).unwrap().unwrap().revision(), 3);
            }
        }
    }
}

#[test]
#[ignore = "process-kill boundary helper invoked by parent test"]
fn worker() {
    let Some(parent) = std::env::var_os("EMILYBASE_FILE_RESTORE_TEST_PARENT") else {
        return;
    };
    let parent = PathBuf::from(parent);
    let bytes = fs::read(parent.join("archive")).unwrap();
    let phase = std::env::var("EMILYBASE_FILE_RESTORE_TEST_PHASE").unwrap();
    let ready = || -> ! {
        println!("EB_FILE_RESTORE_READY");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    };
    restore_with(&bytes, PROJECT, &parent.join("restored"), |at| {
        if matches!(
            (phase.as_str(), at),
            ("metadata", RestoreBoundary::Metadata)
                | ("objects", RestoreBoundary::Objects)
                | ("owned", RestoreBoundary::Owned)
                | ("selected", RestoreBoundary::Selected)
        ) {
            ready();
        }
    })
    .unwrap();
    assert_eq!(phase, "ack");
    ready();
}

#[test]
#[cfg(target_os = "linux")]
fn common_restore_kills_leave_unselected_pair_private_and_recover_selected_or_acknowledged_root() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for empty in [false, true] {
            for phase in ["metadata", "objects", "owned", "selected", "ack"] {
                let source_temp = tempfile::tempdir().unwrap();
                let target = tempfile::tempdir().unwrap();
                let payload = if empty { vec![] } else { vec![0x53; 8193] };
                let mut store = source(source_temp.path(), compacted, &payload);
                let bytes = encoded(&mut store);
                fs::write(target.path().join("archive"), &bytes).unwrap();
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "archive::restore::tests::worker",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("EMILYBASE_FILE_RESTORE_TEST_PARENT", target.path())
                    .env("EMILYBASE_FILE_RESTORE_TEST_PHASE", phase)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let stdout = child.stdout.take().unwrap();
                let (send, receive) = std::sync::mpsc::channel();
                let thread = std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines() {
                        if line.unwrap() == "EB_FILE_RESTORE_READY" {
                            send.send(()).unwrap();
                            return;
                        }
                    }
                });
                let ready = receive.recv_timeout(std::time::Duration::from_secs(15));
                child.kill().unwrap();
                let status = child.wait().unwrap();
                thread.join().unwrap();
                assert!(ready.is_ok(), "phase={phase}");
                assert_eq!(status.signal(), Some(9));
                let destination = target.path().join("restored");
                if matches!(phase, "selected" | "ack") {
                    let mut restored = open(&destination).unwrap();
                    let image = restored.capture().unwrap();
                    assert_eq!(
                        image.metadata_report().wal_version,
                        if compacted { 2 } else { 1 }
                    );
                    assert_eq!(image.metadata_report().last_transaction, 3);
                    assert_eq!(restored.usage().unwrap().orphans, 1);
                    assert_eq!(restored.usage().unwrap().references, 1);
                    assert_eq!(
                        image
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
                        .rename(FILE, info.revision(), "after-recovery")
                        .unwrap();
                    assert_eq!(restored.database.last_transaction(), 4);
                } else {
                    assert!(!destination.exists());
                    let staged = stage(target.path());
                    assert!(staged.join("metadata").is_dir());
                    assert_eq!(staged.join("objects").exists(), phase != "metadata");
                }
                assert_eq!(store.info(FILE).unwrap().unwrap().revision(), 3);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_pair_restore_matches_independent_payload_quota_and_visibility_model(compacted in any::<bool>(),payload in prop::collection::vec(any::<u8>(),0..2049),removed in any::<bool>()) {
        let _serial=TEST_IO.lock().unwrap();let source_temp=tempfile::tempdir().unwrap();let target=tempfile::tempdir().unwrap();let mut store=source(source_temp.path(),compacted,&payload);let info=store.info(FILE).unwrap().unwrap();
        if removed {store.remove(FILE,info.revision()).unwrap();}
        let state=store.quota_state().unwrap();let quota=FileQuota::new(2,payload.len() as u64+6).unwrap();store.set_quota(state,quota).unwrap();let before=store.database.committed_wal().unwrap();let bytes=encoded(&mut store);
        let destination=target.path().join("restored");let report=restore_file_archive(&bytes,PROJECT,&destination).unwrap();let mut restored=open(&destination).unwrap();
        prop_assert_eq!(restored.database.committed_wal().unwrap(),before);prop_assert_eq!(restored.quota().unwrap(),quota);
        prop_assert_eq!(report.references(),usize::from(!removed));prop_assert_eq!(restored.usage().unwrap().orphans,1+usize::from(removed));
        prop_assert_eq!(restored.usage().unwrap().payload_bytes,payload.len() as u64+6);
        prop_assert_eq!(restored.info(FILE).unwrap(),if removed {None}else{Some(info)});
        let captured=restored.capture().unwrap();prop_assert_eq!(captured.objects().objects().iter().find(|o|o.object()==OBJECT).unwrap().payload(),payload);
    }
}
