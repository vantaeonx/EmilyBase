use super::*;
use crate::{FileArchiveReader, FileId, FileQuota, FileStore, TEST_IO, publish_file_archive};
use emilybase_object_storage::{ObjectId, ProjectDirectory};
use emilybase_transactions::Database;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink};
use std::path::PathBuf;
use std::process::{Command, Stdio};

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FILE: FileId = FileId::from_bytes([2; 16]);
fn fixture(parent: &Path, compacted: bool, payload: &[u8]) -> (PathBuf, Vec<u8>) {
    let engine = parent.join("engine");
    fs::create_dir(&engine).unwrap();
    let mut database = Database::create(engine.join("metadata")).unwrap();
    if compacted {
        database.compact().unwrap();
    }
    let objects = engine.join("objects");
    fs::DirBuilder::new().mode(0o700).create(&objects).unwrap();
    let objects = ProjectDirectory::initialize(objects, PROJECT).unwrap();
    let mut store =
        FileStore::initialize(database, objects, FileQuota::new(8, 32768).unwrap()).unwrap();
    store
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
    let input = parent.join("input");
    fs::create_dir(&input).unwrap();
    let archive = input.join("archive");
    publish_file_archive(&store.capture().unwrap(), &archive).unwrap();
    let bytes = fs::read(&archive).unwrap();
    (archive, bytes)
}
fn open(path: &Path) -> FileStore {
    FileStore::open(
        Database::open(path.join("metadata")).unwrap(),
        ProjectDirectory::open(path.join("objects"), PROJECT).unwrap(),
    )
    .unwrap()
}
fn image(store: &mut FileStore) -> Vec<u8> {
    let snapshot = store.capture().unwrap();
    let mut bytes = Vec::new();
    FileArchiveReader::from_snapshot(&snapshot)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}
fn private_stages(parent: &Path) -> Vec<PathBuf> {
    fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".emilybase-directory-")
        })
        .collect()
}

#[test]
fn readonly_file_restore_retains_exact_pair_and_releases_input_descriptors() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for empty in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let payload = if empty { vec![] } else { vec![0x53; 8193] };
            let (archive, bytes) = fixture(temp.path(), compacted, &payload);
            fs::set_permissions(&archive, fs::Permissions::from_mode(0o400)).unwrap();
            let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
            let destination = temp.path().join("restored");
            let identity = fs::metadata(&archive).unwrap();
            let report = restore_file_with(
                &archive,
                PROJECT,
                &destination,
                || {},
                |at| {
                    if at != RestoreBoundary::Owned {
                        return;
                    }
                    let held = fs::read_dir("/proc/self/fd")
                        .unwrap()
                        .filter(|entry| {
                            entry
                                .as_ref()
                                .ok()
                                .and_then(|e| fs::metadata(e.path()).ok())
                                .is_some_and(|m| {
                                    (m.dev(), m.ino()) == (identity.dev(), identity.ino())
                                })
                        })
                        .count();
                    assert_eq!(held, 1);
                },
            )
            .unwrap();
            assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
            assert_eq!(report.objects().objects, 2);
            assert_eq!(report.references(), 1);
            assert_eq!(report.metadata().wal_version, if compacted { 2 } else { 1 });
            let mut restored = open(&destination);
            assert_eq!(image(&mut restored), bytes);
            assert_eq!(restored.usage().unwrap().orphans, 1);
            let info = restored.info(FILE).unwrap().unwrap();
            restored
                .rename(FILE, info.revision(), "independent")
                .unwrap();
            let later = image(&mut restored);
            drop(restored);
            assert!(restore_file_archive_file(&archive, PROJECT, &destination).is_err());
            assert_eq!(image(&mut open(&destination)), later);
            assert_eq!(fs::read(&archive).unwrap(), bytes);
        }
    }
}

#[test]
fn original_input_identity_bytes_and_parent_are_checked_before_and_after_common_selection() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for phase in 0..3 {
            for change in 0..8 {
                let temp = tempfile::tempdir().unwrap();
                let (archive, bytes) = fixture(temp.path(), compacted, b"private-payload");
                let destination = temp.path().join("restored");
                let mutate = || match change {
                    0 => {
                        let saved = temp.path().join("saved");
                        fs::rename(&archive, &saved).unwrap();
                        fs::copy(saved, &archive).unwrap();
                    }
                    1 => {
                        let mut damaged = bytes.clone();
                        *damaged.last_mut().unwrap() ^= 1;
                        fs::write(&archive, damaged).unwrap();
                    }
                    2 => fs::set_permissions(&archive, fs::Permissions::from_mode(0o644)).unwrap(),
                    3 => fs::hard_link(&archive, temp.path().join("alias")).unwrap(),
                    4 => {
                        fs::rename(temp.path().join("input"), temp.path().join("moved")).unwrap();
                        fs::create_dir(temp.path().join("input")).unwrap();
                        fs::copy(temp.path().join("moved/archive"), &archive).unwrap();
                    }
                    5 => fs::write(&archive, &bytes).unwrap(),
                    6 => fs::OpenOptions::new()
                        .append(true)
                        .open(&archive)
                        .unwrap()
                        .write_all(b"extra")
                        .unwrap(),
                    _ => fs::OpenOptions::new()
                        .write(true)
                        .open(&archive)
                        .unwrap()
                        .set_len(7)
                        .unwrap(),
                };
                let result = restore_file_with(
                    &archive,
                    PROJECT,
                    &destination,
                    || {
                        if phase == 0 {
                            mutate();
                        }
                    },
                    |at| {
                        if (phase == 1 && at == RestoreBoundary::Owned)
                            || (phase == 2 && at == RestoreBoundary::Selected)
                        {
                            mutate();
                        }
                    },
                );
                assert!(result.is_err(), "phase={phase},change={change}");
                if phase == 2 {
                    assert!(matches!(result, Err(Error::OutcomeUnknown(_))));
                    assert_eq!(image(&mut open(&destination)), bytes);
                } else {
                    assert!(!matches!(result, Err(Error::OutcomeUnknown(_))));
                    assert!(!destination.exists());
                    assert_eq!(private_stages(temp.path()).len(), usize::from(phase == 1));
                }
                assert_eq!(image(&mut open(&temp.path().join("engine"))), bytes);
            }
        }
    }
}

#[test]
fn invalid_input_selection_size_scope_and_format_create_no_restore_entries() {
    let _serial = TEST_IO.lock().unwrap();
    for change in 0..9 {
        let temp = tempfile::tempdir().unwrap();
        let (archive, bytes) = fixture(temp.path(), false, b"private-payload");
        let destination = temp.path().join("restored");
        let mut project = PROJECT;
        match change {
            0 => {
                fs::rename(&archive, temp.path().join("saved")).unwrap();
                symlink(temp.path().join("saved"), &archive).unwrap();
            }
            1 => fs::hard_link(&archive, temp.path().join("alias")).unwrap(),
            2 => fs::set_permissions(&archive, fs::Permissions::from_mode(0o644)).unwrap(),
            3 => {
                fs::remove_file(&archive).unwrap();
                fs::create_dir(&archive).unwrap();
            }
            4 => {
                fs::OpenOptions::new()
                    .write(true)
                    .open(&archive)
                    .unwrap()
                    .set_len(super::super::MAX_FILE_ARCHIVE_BYTES as u64 + 1)
                    .unwrap();
            }
            5 => project = ProjectId::from_bytes([9; 16]),
            6 => fs::write(&archive, b"invalid").unwrap(),
            7 => {
                let mut future = bytes;
                future[8..10].copy_from_slice(&2u16.to_le_bytes());
                fs::write(&archive, future).unwrap();
            }
            _ => {
                fs::remove_file(&archive).unwrap();
                rustix::fs::mknodat(
                    rustix::fs::CWD,
                    &archive,
                    rustix::fs::FileType::Fifo,
                    rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
                    0,
                )
                .unwrap();
            }
        }
        let before = fs::read_dir(temp.path()).unwrap().count();
        assert!(restore_file_archive_file(&archive, project, &destination).is_err());
        assert!(!destination.exists());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), before);
        assert!(private_stages(temp.path()).is_empty());
    }
}

#[test]
#[ignore = "process-kill source retention helper invoked by parent test"]
fn worker() {
    let Some(parent) = std::env::var_os("EMILYBASE_FILE_SOURCE_TEST_PARENT") else {
        return;
    };
    let parent = PathBuf::from(parent);
    let phase = std::env::var("EMILYBASE_FILE_SOURCE_TEST_PHASE").unwrap();
    let ready = || -> ! {
        println!("EB_FILE_SOURCE_READY");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    };
    restore_file_with(
        &parent.join("input/archive"),
        PROJECT,
        &parent.join("restored"),
        || {
            if phase == "read" {
                ready();
            }
        },
        |at| {
            if matches!(
                (phase.as_str(), at),
                ("owned", RestoreBoundary::Owned) | ("selected", RestoreBoundary::Selected)
            ) {
                ready();
            }
        },
    )
    .unwrap();
    assert_eq!(phase, "ack");
    ready();
}

#[test]
fn killed_file_restore_never_changes_input_and_recovers_exact_selected_pair() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for empty in [false, true] {
            for phase in ["read", "owned", "selected", "ack"] {
                let temp = tempfile::tempdir().unwrap();
                let payload = if empty { vec![] } else { vec![0x53; 8193] };
                let (archive, bytes) = fixture(temp.path(), compacted, &payload);
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "archive::source::tests::worker",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("EMILYBASE_FILE_SOURCE_TEST_PARENT", temp.path())
                    .env("EMILYBASE_FILE_SOURCE_TEST_PHASE", phase)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let stdout = child.stdout.take().unwrap();
                let (send, receive) = std::sync::mpsc::channel();
                let thread = std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines() {
                        if line.unwrap() == "EB_FILE_SOURCE_READY" {
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
                assert_eq!(fs::read(&archive).unwrap(), bytes);
                let target = temp.path().join("restored");
                if matches!(phase, "selected" | "ack") {
                    let mut store = open(&target);
                    assert_eq!(image(&mut store), bytes);
                    let info = store.info(FILE).unwrap().unwrap();
                    store.rename(FILE, info.revision(), "after-kill").unwrap();
                    assert_eq!(store.database.last_transaction(), 4);
                } else {
                    assert!(!target.exists());
                    assert_eq!(
                        private_stages(temp.path()).len(),
                        usize::from(phase == "owned")
                    );
                }
                assert_eq!(image(&mut open(&temp.path().join("engine"))), bytes);
            }
        }
    }
}
