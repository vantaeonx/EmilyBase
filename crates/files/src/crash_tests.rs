use super::*;
use crate::TEST_IO;
use crate::mutation::MetadataBoundary;
use crate::snapshot::CaptureBoundary;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
use std::time::Duration;

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FILE: FileId = FileId::from_bytes([2; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([3; 16]);

fn signal_and_wait() -> ! {
    println!("EB_FILES_READY");
    std::io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
#[ignore = "process-kill boundary helper invoked by parent test"]
fn worker() {
    let Some(parent) = std::env::var_os("EMILYBASE_FILE_TEST_PARENT") else {
        return;
    };
    let parent = std::path::PathBuf::from(parent);
    let phase = std::env::var("EMILYBASE_FILE_TEST_PHASE").unwrap();
    let payload = if std::env::var("EMILYBASE_FILE_TEST_EMPTY").unwrap() == "1" {
        vec![]
    } else {
        (0..8193).map(|index| (index % 251) as u8).collect()
    };
    let mut database = Database::create(parent.join("metadata")).unwrap();
    if std::env::var("EMILYBASE_FILE_TEST_COMPACT").unwrap() == "1" {
        database.compact().unwrap();
    }
    fs::DirBuilder::new()
        .mode(0o700)
        .create(parent.join("objects"))
        .unwrap();
    let objects = ProjectDirectory::initialize(parent.join("objects"), PROJECT).unwrap();
    let mut store =
        FileStore::initialize(database, objects, FileQuota::new(4, 32_768).unwrap()).unwrap();
    let initial = store
        .publish_with(FILE, OBJECT, [4; 16], "synthetic", &payload, |at| {
            if (phase == "blob" && at == PublishBoundary::Blob)
                || (phase == "metadata" && at == PublishBoundary::Metadata)
            {
                signal_and_wait();
            }
        })
        .unwrap();
    if let Some((operation, point)) = phase.split_once('-') {
        if operation == "capture" {
            let snapshot = store
                .capture_with(|at| {
                    if (point == "admitted" && at == CaptureBoundary::Admitted)
                        || (point == "metadata" && at == CaptureBoundary::Metadata)
                        || (point == "objects" && at == CaptureBoundary::Objects)
                    {
                        signal_and_wait();
                    }
                })
                .unwrap();
            assert_eq!(point, "ack");
            assert_eq!(snapshot.files(), std::slice::from_ref(&initial));
            assert_eq!(snapshot.objects().objects()[0].payload(), payload);
            signal_and_wait();
        }
        assert!(matches!(operation, "rename" | "remove" | "quota"));
        let checked = |at| {
            if (point == "staged" && at == MetadataBoundary::Staged)
                || (point == "committed" && at == MetadataBoundary::Committed)
            {
                signal_and_wait();
            }
        };
        if operation == "quota" {
            let state = store.quota_state().unwrap();
            store
                .set_quota_with(state, FileQuota::new(2, 16_384).unwrap(), checked)
                .unwrap();
        } else {
            store
                .mutate_with(
                    FILE,
                    initial.revision(),
                    if operation == "remove" {
                        None
                    } else {
                        Some("renamed-synthetic")
                    },
                    checked,
                )
                .unwrap();
        }
        assert_eq!(point, "ack");
        signal_and_wait();
    }
    assert_eq!(phase, "ack");
    signal_and_wait();
}

fn kill_at(path: &std::path::Path, phase: &str, empty: bool, compacted: bool) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::crash_tests::worker",
            "--ignored",
            "--nocapture",
        ])
        .env("EMILYBASE_FILE_TEST_PARENT", path)
        .env("EMILYBASE_FILE_TEST_PHASE", phase)
        .env("EMILYBASE_FILE_TEST_EMPTY", if empty { "1" } else { "0" })
        .env(
            "EMILYBASE_FILE_TEST_COMPACT",
            if compacted { "1" } else { "0" },
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let receiver = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if line == "EB_FILES_READY" {
                send.send(()).unwrap();
                return;
            }
        }
    });
    let ready = receive.recv_timeout(Duration::from_secs(15));
    child.kill().unwrap();
    let status = child.wait().unwrap();
    receiver.join().unwrap();
    assert!(ready.is_ok(), "worker did not reach phase {phase}");
    assert_eq!(status.signal(), Some(9));
}

#[test]
fn process_kills_preserve_acknowledged_references_and_keep_precommit_blobs_invisible_and_charged() {
    let _serial = TEST_IO.lock().unwrap();
    for phase in ["blob", "metadata", "ack"] {
        for empty in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            kill_at(temp.path(), phase, empty, false);
            let db = Database::open(temp.path().join("metadata")).unwrap();
            let objects = ProjectDirectory::open(temp.path().join("objects"), PROJECT).unwrap();
            let mut store = FileStore::open(db, objects).unwrap();
            let referenced = usize::from(phase != "blob");
            let bytes = if empty { 0 } else { 8193 };
            assert_eq!(
                store.usage().unwrap(),
                FileUsage {
                    physical_objects: 1,
                    payload_bytes: bytes,
                    references: referenced,
                    orphans: 1 - referenced,
                }
            );
            if referenced == 0 {
                assert!(matches!(store.reader(FILE), Err(Error::Missing)));
                assert!(store.info(FILE).unwrap().is_none());
            } else {
                let info = store.info(FILE).unwrap().unwrap();
                let mut reader = store.reader(FILE).unwrap();
                let mut payload = Vec::new();
                let mut scratch = [0; 257];
                loop {
                    let n = reader.read_payload(&mut scratch).unwrap();
                    if n == 0 {
                        break;
                    }
                    payload.extend_from_slice(&scratch[..n]);
                }
                assert_eq!(reader.finish().unwrap(), *info.report());
                let expected: Vec<_> = (0..bytes).map(|index| (index % 251) as u8).collect();
                assert_eq!(payload, expected);
            }
            let next = store
                .publish(
                    FileId::from_bytes([5; 16]),
                    ObjectId::from_bytes([6; 16]),
                    [7; 16],
                    "independent-after-recovery",
                    b"next",
                )
                .unwrap();
            assert!(next.revision() > 2);
            assert_eq!(store.usage().unwrap().physical_objects, 2);
            assert_eq!(store.usage().unwrap().orphans, 1 - referenced);
        }
    }
}

#[test]
fn mutation_kills_recover_only_committed_metadata_and_retain_physical_charge_on_both_wals() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for operation in ["rename", "remove", "quota"] {
            for point in ["staged", "committed", "ack"] {
                for empty in [false, true] {
                    let temp = tempfile::tempdir().unwrap();
                    kill_at(
                        temp.path(),
                        &format!("{operation}-{point}"),
                        empty,
                        compacted,
                    );
                    let mut database = Database::open(temp.path().join("metadata")).unwrap();
                    let version = emilybase_transactions::recover_image(
                        &database.committed_wal().unwrap(),
                        Some(database.database_id()),
                    )
                    .unwrap()
                    .wal_version;
                    assert_eq!(version, if compacted { 2 } else { 1 });
                    assert_eq!(
                        database.last_transaction(),
                        if point == "staged" { 3 } else { 4 }
                    );
                    let objects =
                        ProjectDirectory::open(temp.path().join("objects"), PROJECT).unwrap();
                    let mut store = FileStore::open(database, objects).unwrap();
                    assert_eq!(
                        store.quota().unwrap(),
                        if operation == "quota" && point != "staged" {
                            FileQuota::new(2, 16_384).unwrap()
                        } else {
                            FileQuota::new(4, 32_768).unwrap()
                        }
                    );
                    let removed = operation == "remove" && point != "staged";
                    let info = store.info(FILE).unwrap();
                    if removed {
                        assert!(info.is_none());
                        assert!(matches!(store.reader(FILE), Err(Error::Missing)));
                    } else {
                        let info = info.unwrap();
                        assert_eq!(
                            info.name(),
                            if point == "staged" || operation == "quota" {
                                "synthetic"
                            } else {
                                "renamed-synthetic"
                            }
                        );
                        assert_eq!(
                            info.revision(),
                            if point == "staged" || operation == "quota" {
                                3
                            } else {
                                4
                            }
                        );
                        store.reader(FILE).unwrap().finish().unwrap();
                    }
                    assert_eq!(
                        store.usage().unwrap(),
                        FileUsage {
                            physical_objects: 1,
                            payload_bytes: if empty { 0 } else { 8193 },
                            references: usize::from(!removed),
                            orphans: usize::from(removed)
                        }
                    );
                    let physical = emilybase_object_storage::inspect_file(
                        temp.path().join("objects").join(format!("{OBJECT}.object")),
                        PROJECT,
                        OBJECT,
                    )
                    .unwrap();
                    assert_eq!(physical.payload_bytes, if empty { 0 } else { 8193 });
                    store
                        .publish(
                            FileId::from_bytes([5; 16]),
                            ObjectId::from_bytes([6; 16]),
                            [7; 16],
                            "after-recovery",
                            b"next",
                        )
                        .unwrap();
                    assert_eq!(store.usage().unwrap().physical_objects, 2);
                    assert_eq!(store.usage().unwrap().orphans, usize::from(removed));
                }
            }
        }
    }
}

#[test]
fn capture_process_kills_preserve_exact_acknowledged_source_pair_and_allow_next_write() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for empty in [false, true] {
            for point in ["admitted", "metadata", "objects", "ack"] {
                let temp = tempfile::tempdir().unwrap();
                kill_at(temp.path(), &format!("capture-{point}"), empty, compacted);
                let mut db = Database::open(temp.path().join("metadata")).unwrap();
                let before = db.committed_wal().unwrap();
                let recovered =
                    emilybase_transactions::recover_image(&before, Some(db.database_id())).unwrap();
                assert_eq!(recovered.last_transaction, 3);
                assert_eq!(recovered.wal_version, if compacted { 2 } else { 1 });
                let objects = ProjectDirectory::open(temp.path().join("objects"), PROJECT).unwrap();
                let mut store = FileStore::open(db, objects).unwrap();
                assert_eq!(
                    store.usage().unwrap(),
                    FileUsage {
                        physical_objects: 1,
                        payload_bytes: if empty { 0 } else { 8193 },
                        references: 1,
                        orphans: 0
                    }
                );
                let snapshot = store.capture().unwrap();
                assert_eq!(
                    &snapshot.metadata_bytes()[emilybase_backup::HEADER_SIZE..],
                    before
                );
                assert_eq!(snapshot.files()[0].revision(), 3);
                let payload: Vec<_> = if empty {
                    vec![]
                } else {
                    (0..8193).map(|index| (index % 251) as u8).collect()
                };
                assert_eq!(snapshot.objects().objects()[0].payload(), payload);
                store
                    .publish(
                        FileId::from_bytes([5; 16]),
                        ObjectId::from_bytes([6; 16]),
                        [7; 16],
                        "after-recovery",
                        b"next",
                    )
                    .unwrap();
                assert_eq!(store.database.last_transaction(), 4);
                assert_eq!(store.usage().unwrap().references, 2);
            }
        }
    }
}
