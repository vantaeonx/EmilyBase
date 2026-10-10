use super::*;
use crate::{FileId, FileStore, TEST_IO};
use emilybase_object_storage::{ObjectId, ProjectDirectory};
use emilybase_transactions::Database;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
use std::process::{Command, Stdio};
const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FILE: FileId = FileId::from_bytes([2; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([3; 16]);
fn snapshot(path: &Path, compacted: bool, empty: bool) -> FileSnapshot {
    let mut db = Database::create(path.join("metadata")).unwrap();
    if compacted {
        db.compact().unwrap();
    }
    fs::DirBuilder::new()
        .mode(0o700)
        .create(path.join("objects"))
        .unwrap();
    let objects = ProjectDirectory::initialize(path.join("objects"), PROJECT).unwrap();
    let mut store = FileStore::initialize(db, objects, FileQuota::new(4, 32768).unwrap()).unwrap();
    let payload: Vec<_> = if empty {
        vec![]
    } else {
        (0..8193).map(|n| (n % 251) as u8).collect()
    };
    store
        .publish(FILE, OBJECT, [4; 16], "synthetic-display", &payload)
        .unwrap();
    store
        .objects
        .put(ObjectId::from_bytes([5; 16]), b"orphan")
        .unwrap();
    store.capture().unwrap()
}

#[test]
fn canonical_private_file_publication_is_no_replace_and_independently_inspected_on_both_wals() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for empty in [false, true] {
            let source = tempfile::tempdir().unwrap();
            let target = tempfile::tempdir().unwrap();
            let snapshot = snapshot(source.path(), compacted, empty);
            let path = target.path().join("snapshot.ebfiles");
            let report = publish_file_archive(&snapshot, &path).unwrap();
            assert_eq!(report.project(), PROJECT);
            assert_eq!(report.metadata(), snapshot.metadata_report());
            assert_eq!(report.quota(), snapshot.quota());
            assert_eq!(report.references(), 1);
            assert_eq!(report.objects().objects, 2);
            assert_eq!(report.objects().payload_bytes, if empty { 6 } else { 8199 });
            let image = fs::read(&path).unwrap();
            let view = verify_file_archive(&image, PROJECT).unwrap();
            assert_eq!(view.files(), snapshot.files());
            assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
            assert_eq!(inspect_file_archive(&path, PROJECT).unwrap(), report);
            assert!(publish_file_archive(&snapshot, &path).is_err());
            assert_eq!(fs::read(&path).unwrap(), image);
            fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
            assert_eq!(inspect_file_archive(&path, PROJECT).unwrap(), report);
            assert!(inspect_file_archive(&path, ProjectId::from_bytes([9; 16])).is_err());
            assert!(!format!("{report:?}").contains("synthetic-display"));
            assert!(!format!("{report:?}").contains(&PROJECT.to_string()));
        }
    }
}

#[test]
fn selected_actual_inode_parent_and_exact_canonical_bytes_survive_no_silent_adoption() {
    let _serial = TEST_IO.lock().unwrap();
    for case in 0..5 {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let snapshot = snapshot(source.path(), false, false);
        let alternate_source = tempfile::tempdir().unwrap();
        let alternate = self::snapshot(alternate_source.path(), false, false);
        let mut alternate_bytes = Vec::new();
        FileArchiveReader::from_snapshot(&alternate)
            .unwrap()
            .read_to_end(&mut alternate_bytes)
            .unwrap();
        assert!(verify_file_archive(&alternate_bytes, PROJECT).is_ok());
        let parent = target.path().join("output");
        fs::DirBuilder::new().mode(0o700).create(&parent).unwrap();
        let path = parent.join("snapshot");
        let saved = target.path().join("original");
        let result = publish_with(
            &snapshot,
            &path,
            || {},
            || match case {
                0 => {
                    fs::rename(&path, &saved).unwrap();
                    fs::copy(&saved, &path).unwrap();
                }
                1 => {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
                }
                2 => {
                    fs::write(&path, b"changed-selected-inode").unwrap();
                }
                3 => {
                    fs::rename(&parent, &saved).unwrap();
                    fs::DirBuilder::new().mode(0o700).create(&parent).unwrap();
                }
                _ => {
                    fs::write(&path, &alternate_bytes).unwrap();
                }
            },
        );
        assert!(matches!(result, Err(Error::OutcomeUnknown(_))));
        if case == 0 {
            assert_eq!(fs::read(&path).unwrap(), fs::read(&saved).unwrap());
        }
        if case == 2 {
            assert_eq!(fs::read(&path).unwrap(), b"changed-selected-inode");
        }
        if case == 3 {
            assert!(saved.join("snapshot").exists());
            assert!(!path.exists());
        }
        if case == 4 {
            assert_eq!(fs::read(&path).unwrap(), alternate_bytes);
        }
    }
}

#[test]
fn inspection_refuses_symlink_hardlink_public_mode_directory_and_detected_late_rewrite() {
    let _serial = TEST_IO.lock().unwrap();
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let snapshot = snapshot(source.path(), false, false);
    let path = target.path().join("snapshot");
    publish_file_archive(&snapshot, &path).unwrap();
    let alias = target.path().join("alias");
    symlink(&path, &alias).unwrap();
    assert!(inspect_file_archive(&alias, PROJECT).is_err());
    fs::remove_file(&alias).unwrap();
    fs::hard_link(&path, &alias).unwrap();
    assert!(inspect_file_archive(&path, PROJECT).is_err());
    fs::remove_file(&alias).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(inspect_file_archive(&path, PROJECT).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(inspect_file_archive(target.path(), PROJECT).is_err());
    let oversized = target.path().join("oversized");
    fs::write(&oversized, b"").unwrap();
    fs::set_permissions(&oversized, fs::Permissions::from_mode(0o600)).unwrap();
    File::options()
        .write(true)
        .open(&oversized)
        .unwrap()
        .set_len(MAX_FILE_ARCHIVE_BYTES as u64 + 1)
        .unwrap();
    assert!(matches!(
        inspect_file_archive(&oversized, PROJECT),
        Err(Error::Destination)
    ));
    assert_eq!(
        fs::metadata(&oversized).unwrap().len(),
        MAX_FILE_ARCHIVE_BYTES as u64 + 1
    );
    let original = fs::read(&path).unwrap();
    for after_decode in [false, true] {
        fs::write(&path, &original).unwrap();
        let target = Target::new(&path).unwrap();
        let mut file = File::open(&path).unwrap();
        let rewrite = || {
            fs::write(&path, b"synthetic-late-rewrite").unwrap();
        };
        let result = if after_decode {
            inspect_with(&target, &mut file, PROJECT, None, || {}, rewrite)
        } else {
            inspect_with(&target, &mut file, PROJECT, None, rewrite, || {})
        };
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"synthetic-late-rewrite");
    }
}

#[test]
#[ignore = "process-kill boundary helper invoked by parent test"]
fn worker() {
    let Some(path) = std::env::var_os("EMILYBASE_FILE_ARCHIVE_TEST_PARENT") else {
        return;
    };
    let path = PathBuf::from(path);
    let compacted = std::env::var("EMILYBASE_FILE_ARCHIVE_TEST_COMPACT").unwrap() == "1";
    let empty = std::env::var("EMILYBASE_FILE_ARCHIVE_TEST_EMPTY").unwrap() == "1";
    let phase = std::env::var("EMILYBASE_FILE_ARCHIVE_TEST_PHASE").unwrap();
    let snapshot = snapshot(&path, compacted, empty);
    let ready = || -> ! {
        println!("EB_FILE_ARCHIVE_READY");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    };
    publish_with(
        &snapshot,
        &path.join("snapshot"),
        || {
            if phase == "prepared" {
                ready();
            }
        },
        || {
            if phase == "selected" {
                ready();
            }
        },
    )
    .unwrap();
    assert_eq!(phase, "ack");
    ready();
}

#[test]
#[cfg(target_os = "linux")]
fn process_kills_keep_prepared_target_absent_and_selected_or_acknowledged_pair_verifiable() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for empty in [false, true] {
            for phase in ["prepared", "selected", "ack"] {
                let temp = tempfile::tempdir().unwrap();
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "archive::publication::tests::worker",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("EMILYBASE_FILE_ARCHIVE_TEST_PARENT", temp.path())
                    .env(
                        "EMILYBASE_FILE_ARCHIVE_TEST_COMPACT",
                        if compacted { "1" } else { "0" },
                    )
                    .env(
                        "EMILYBASE_FILE_ARCHIVE_TEST_EMPTY",
                        if empty { "1" } else { "0" },
                    )
                    .env("EMILYBASE_FILE_ARCHIVE_TEST_PHASE", phase)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let stdout = child.stdout.take().unwrap();
                let (send, receive) = std::sync::mpsc::channel();
                let thread = std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines() {
                        if line.unwrap() == "EB_FILE_ARCHIVE_READY" {
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
                let db = Database::open(temp.path().join("metadata")).unwrap();
                let objects = ProjectDirectory::open(temp.path().join("objects"), PROJECT).unwrap();
                let mut store = FileStore::open(db, objects).unwrap();
                assert_eq!(store.usage().unwrap().references, 1);
                assert_eq!(store.usage().unwrap().orphans, 1);
                let path = temp.path().join("snapshot");
                if phase == "prepared" {
                    assert!(!path.exists());
                } else {
                    let report = inspect_file_archive(&path, PROJECT).unwrap();
                    assert_eq!(report.metadata().wal_version, if compacted { 2 } else { 1 });
                    assert_eq!(report.metadata().last_transaction, 3);
                    assert_eq!(report.references(), 1);
                    assert_eq!(report.objects().objects, 2);
                    assert_eq!(report.objects().payload_bytes, if empty { 6 } else { 8199 });
                }
                let info = store.info(FILE).unwrap().unwrap();
                store
                    .rename(FILE, info.revision(), "after-recovery")
                    .unwrap();
                assert_eq!(store.database.last_transaction(), 4);
            }
        }
    }
}
