use super::*;
use crate::TEST_IO;
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
    let database = Database::create(parent.join("metadata")).unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(parent.join("objects"))
        .unwrap();
    let objects = ProjectDirectory::initialize(parent.join("objects"), PROJECT).unwrap();
    let mut store =
        FileStore::initialize(database, objects, FileQuota::new(4, 32_768).unwrap()).unwrap();
    store
        .publish_with(FILE, OBJECT, [4; 16], "synthetic", &payload, |at| {
            if (phase == "blob" && at == PublishBoundary::Blob)
                || (phase == "metadata" && at == PublishBoundary::Metadata)
            {
                signal_and_wait();
            }
        })
        .unwrap();
    assert_eq!(phase, "ack");
    signal_and_wait();
}

#[test]
fn process_kills_preserve_acknowledged_references_and_keep_precommit_blobs_invisible_and_charged() {
    let _serial = TEST_IO.lock().unwrap();
    for phase in ["blob", "metadata", "ack"] {
        for empty in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "store::crash_tests::worker",
                    "--ignored",
                    "--nocapture",
                ])
                .env("EMILYBASE_FILE_TEST_PARENT", temp.path())
                .env("EMILYBASE_FILE_TEST_PHASE", phase)
                .env("EMILYBASE_FILE_TEST_EMPTY", if empty { "1" } else { "0" })
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
