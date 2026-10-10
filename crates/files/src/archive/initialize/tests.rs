use super::*;
use crate::{FileArchiveReader, FileId, TEST_IO, restore_file_archive};
use emilybase_object_storage::ObjectId;
use proptest::prelude::*;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::PathBuf;
use std::process::{Command, Stdio};

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FILE: FileId = FileId::from_bytes([2; 16]);
fn open(path: &Path) -> FileStore {
    FileStore::open(
        Database::open(path.join("metadata")).unwrap(),
        ProjectDirectory::open(path.join("objects"), PROJECT).unwrap(),
    )
    .unwrap()
}
fn stage(parent: &Path) -> PathBuf {
    let stages = fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".emilybase-directory-")
        })
        .collect::<Vec<_>>();
    assert_eq!(stages.len(), 1);
    stages[0].clone()
}

#[test]
fn new_root_is_complete_private_durable_and_accepts_independent_catalog_writes() {
    let _serial = TEST_IO.lock().unwrap();
    for quota in [
        FileQuota::new(0, 0).unwrap(),
        FileQuota::new(8, 32768).unwrap(),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("new");
        let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
        let report = initialize_with(&target, PROJECT, quota, |at| {
            if at != InitializeBoundary::Owned {
                return;
            }
            let private = stage(temp.path());
            let probe = fs::File::open(private.join("metadata")).unwrap();
            assert!(matches!(
                probe.try_lock(),
                Err(std::fs::TryLockError::WouldBlock)
            ));
            assert!(Database::open(private.join("metadata")).is_err());
            assert!(matches!(
                ProjectDirectory::open(private.join("objects"), PROJECT),
                Err(emilybase_object_storage::Error::Busy)
            ));
        })
        .unwrap();
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
        assert_eq!(fs::metadata(&target).unwrap().mode() & 0o777, 0o700);
        assert_eq!(fs::read_dir(&target).unwrap().count(), 2);
        assert_eq!(report.project(), PROJECT);
        assert_eq!(report.quota(), quota);
        assert_eq!(report.references(), 0);
        assert_eq!(report.objects().objects, 0);
        assert_eq!(report.metadata().last_transaction, 2);
        assert_eq!(report.metadata().wal_version, 1);
        let mut store = open(&target);
        assert_eq!(store.database.database_id(), report.metadata().database_id);
        assert_eq!(store.quota().unwrap(), quota);
        assert!(store.list().unwrap().is_empty());
        let wal = store.database.committed_wal().unwrap();
        let result = store.publish(
            FILE,
            ObjectId::from_bytes([3; 16]),
            [4; 16],
            "synthetic",
            b"payload",
        );
        if quota.objects() == 0 {
            assert!(result.is_err());
            assert_eq!(store.database.committed_wal().unwrap(), wal);
        } else {
            assert!(result.is_ok());
            assert_eq!(store.database.last_transaction(), 3);
        }
        drop(store);
        let later = fs::read(target.join("metadata/redo.wal")).unwrap();
        assert!(initialize_file_root(&target, PROJECT, quota).is_err());
        assert_eq!(fs::read(target.join("metadata/redo.wal")).unwrap(), later);
        assert_eq!(
            open(&target).list().unwrap().len(),
            usize::from(quota.objects() != 0)
        );
    }
}

#[test]
fn initialized_original_children_wal_and_marker_cannot_be_substituted_around_selection() {
    let _serial = TEST_IO.lock().unwrap();
    for selected in [false, true] {
        for change in 0..6 {
            let temp = tempfile::tempdir().unwrap();
            let target = temp.path().join("new");
            let result =
                initialize_with(&target, PROJECT, FileQuota::new(8, 32768).unwrap(), |at| {
                    if at
                        != if selected {
                            InitializeBoundary::Selected
                        } else {
                            InitializeBoundary::Owned
                        }
                    {
                        return;
                    }
                    let root = if selected {
                        target.clone()
                    } else {
                        stage(temp.path())
                    };
                    let saved = temp.path().join("saved");
                    match change {
                        0 | 1 => {
                            let name = if change == 0 { "metadata" } else { "objects" };
                            fs::rename(root.join(name), &saved).unwrap();
                            fs::DirBuilder::new()
                                .mode(0o700)
                                .create(root.join(name))
                                .unwrap();
                            let file = if change == 0 {
                                "redo.wal"
                            } else {
                                ".emilybase-objects"
                            };
                            fs::copy(saved.join(file), root.join(name).join(file)).unwrap();
                        }
                        2 | 4 => {
                            let file = if change == 2 {
                                root.join("metadata/redo.wal")
                            } else {
                                root.join("objects/.emilybase-objects")
                            };
                            fs::rename(&file, &saved).unwrap();
                            fs::copy(saved, &file).unwrap();
                        }
                        3 => {
                            fs::write(root.join("metadata/redo.wal"), b"damaged").unwrap();
                        }
                        _ => {
                            fs::write(root.join("unexpected"), b"foreign entry").unwrap();
                        }
                    }
                });
            assert!(result.is_err(), "change={change},selected={selected}");
            assert_eq!(matches!(result, Err(Error::OutcomeUnknown(_))), selected);
            assert_eq!(target.exists(), selected);
            if !selected {
                assert!(stage(temp.path()).is_dir());
            }
        }
    }
}

#[test]
#[ignore = "process-kill root initialization helper invoked by parent test"]
fn worker() {
    let Some(parent) = std::env::var_os("EMILYBASE_FILE_INIT_TEST_PARENT") else {
        return;
    };
    let parent = PathBuf::from(parent);
    let phase = std::env::var("EMILYBASE_FILE_INIT_TEST_PHASE").unwrap();
    let zero = std::env::var("EMILYBASE_FILE_INIT_TEST_ZERO").unwrap() == "1";
    let quota = if zero {
        FileQuota::new(0, 0).unwrap()
    } else {
        FileQuota::new(8, 32768).unwrap()
    };
    let ready = || -> ! {
        println!("EB_FILE_INIT_READY");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    };
    initialize_with(&parent.join("new"), PROJECT, quota, |at| {
        if matches!(
            (phase.as_str(), at),
            ("metadata", InitializeBoundary::Metadata)
                | ("objects", InitializeBoundary::Objects)
                | ("catalog", InitializeBoundary::Catalog)
                | ("owned", InitializeBoundary::Owned)
                | ("selected", InitializeBoundary::Selected)
        ) {
            ready();
        }
    })
    .unwrap();
    assert_eq!(phase, "ack");
    ready();
}

#[test]
fn initialization_kills_leave_unselected_private_stages_and_recover_acknowledged_root() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = TEST_IO.lock().unwrap();
    for zero in [false, true] {
        for phase in ["metadata", "objects", "catalog", "owned", "selected", "ack"] {
            let temp = tempfile::tempdir().unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "archive::initialize::tests::worker",
                    "--ignored",
                    "--nocapture",
                ])
                .env("EMILYBASE_FILE_INIT_TEST_PARENT", temp.path())
                .env("EMILYBASE_FILE_INIT_TEST_PHASE", phase)
                .env(
                    "EMILYBASE_FILE_INIT_TEST_ZERO",
                    if zero { "1" } else { "0" },
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
                    if line.unwrap() == "EB_FILE_INIT_READY" {
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
            let target = temp.path().join("new");
            let selected = matches!(phase, "selected" | "ack");
            assert_eq!(target.exists(), selected);
            let root = if selected { target } else { stage(temp.path()) };
            let database = Database::open(root.join("metadata")).unwrap();
            let catalog = matches!(phase, "catalog" | "owned" | "selected" | "ack");
            assert_eq!(database.last_transaction(), if catalog { 2 } else { 1 });
            drop(database);
            if catalog {
                let mut store = open(&root);
                assert!(store.list().unwrap().is_empty());
                assert_eq!(store.quota().unwrap().objects(), if zero { 0 } else { 8 });
                if !zero {
                    store
                        .publish(
                            FILE,
                            ObjectId::from_bytes([3; 16]),
                            [4; 16],
                            "after-kill",
                            b"payload",
                        )
                        .unwrap();
                    assert_eq!(store.database.last_transaction(), 3);
                }
            } else {
                assert_eq!(root.join("objects").exists(), phase == "objects");
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_fresh_quota_admission_and_backup_restore_match_independent_model(objects in 0usize..129,maximum in 0u64..4097,payload in prop::collection::vec(any::<u8>(),0..2049)) {
        let _serial=TEST_IO.lock().unwrap();let temp=tempfile::tempdir().unwrap();let target=temp.path().join("new");
        let quota=FileQuota::new(objects,maximum).unwrap();initialize_file_root(&target,PROJECT,quota).unwrap();
        let mut store=open(&target);let before=store.database.committed_wal().unwrap();
        let result=store.publish(FILE,ObjectId::from_bytes([3;16]),[4;16],"synthetic",&payload);
        let accepted=objects>0 && payload.len() as u64<=maximum;
        prop_assert_eq!(result.is_ok(),accepted);
        if !accepted {prop_assert_eq!(store.database.committed_wal().unwrap(),before);}
        prop_assert_eq!(store.usage().unwrap().references,usize::from(accepted));
        prop_assert_eq!(store.usage().unwrap().payload_bytes,if accepted {payload.len() as u64} else {0});
        let snapshot=store.capture().unwrap();let mut bytes=Vec::new();FileArchiveReader::from_snapshot(&snapshot).unwrap().read_to_end(&mut bytes).unwrap();
        let report=restore_file_archive(&bytes,PROJECT,temp.path().join("copy")).unwrap();
        prop_assert_eq!(report.quota(),quota);prop_assert_eq!(report.references(),usize::from(accepted));
        let mut restored=open(&temp.path().join("copy"));prop_assert_eq!(restored.database.committed_wal().unwrap(),store.database.committed_wal().unwrap());
        if accepted {let captured=restored.capture().unwrap();prop_assert_eq!(captured.objects().objects()[0].payload(),payload);}
    }
}
