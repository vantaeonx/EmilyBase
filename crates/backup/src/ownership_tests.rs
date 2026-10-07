use std::fs;
use std::path::Path;

use emilybase_transactions::Database;

use crate::{Error, create, files, inspect, restore};

thread_local! {
    static SYNC_FAILURE: std::cell::Cell<Option<(&'static str, bool)>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn sync_failure(phase: &'static str, after: bool) -> std::io::Result<()> {
    if SYNC_FAILURE.get() == Some((phase, after)) {
        Err(std::io::Error::other("synthetic backup sync failure"))
    } else {
        Ok(())
    }
}

struct FailureGuard;

impl FailureGuard {
    fn new(phase: &'static str, after: bool) -> Self {
        assert!(SYNC_FAILURE.replace(Some((phase, after))).is_none());
        Self
    }
}

impl Drop for FailureGuard {
    fn drop(&mut self) {
        SYNC_FAILURE.set(None);
    }
}

fn staging_name(parent: &Path) -> std::ffi::OsString {
    fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .find(|name| name.to_string_lossy().starts_with(".emilybase-backup-"))
        .unwrap()
}

#[test]
fn replaced_backup_parent_cannot_publish_or_delete_a_foreign_file() {
    let _guard = crate::publication_tests::PROCESS_TESTS.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("outputs");
        let moved = temp.path().join("moved");
        fs::create_dir(&parent).unwrap();
        let source = temp.path().join("source");
        let mut database = Database::create(&source).unwrap();
        if compacted {
            database.compact().unwrap();
        }
        let source_before = fs::read(source.join("redo.wal")).unwrap();
        let mut foreign = Database::create(temp.path().join("foreign")).unwrap();
        let foreign_bytes = crate::encode(&foreign.committed_wal().unwrap()).unwrap();
        let name = std::cell::RefCell::new(None);
        let outcome = files::create_with(
            &mut database,
            &parent.join("snapshot.backup"),
            || {
                let stage = staging_name(&parent);
                fs::rename(&parent, &moved).unwrap();
                fs::create_dir(&parent).unwrap();
                fs::write(parent.join(&stage), &foreign_bytes).unwrap();
                *name.borrow_mut() = Some(stage);
            },
            || {},
        );
        assert!(matches!(outcome, Err(Error::PathChanged)));
        assert!(!parent.join("snapshot.backup").exists());
        assert!(!moved.join("snapshot.backup").exists());
        assert_eq!(
            fs::read(parent.join(name.borrow().as_ref().unwrap())).unwrap(),
            foreign_bytes
        );
        assert_eq!(fs::read(source.join("redo.wal")).unwrap(), source_before);
    }
}

#[test]
fn replaced_restore_parent_cannot_publish_or_delete_a_foreign_directory() {
    let _guard = crate::publication_tests::PROCESS_TESTS.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("outputs");
        let moved = temp.path().join("moved");
        fs::create_dir(&parent).unwrap();
        let mut database = Database::create(temp.path().join("source")).unwrap();
        if compacted {
            database.compact().unwrap();
        }
        let archive = temp.path().join("source.backup");
        create(&mut database, &archive).unwrap();
        let archive_before = fs::read(&archive).unwrap();
        let name = std::cell::RefCell::new(None);
        let outcome = restore::restore_with(
            &archive,
            &parent.join("restored"),
            || {
                let stage = staging_name(&parent);
                fs::rename(&parent, &moved).unwrap();
                fs::create_dir(&parent).unwrap();
                fs::create_dir(parent.join(&stage)).unwrap();
                fs::write(parent.join(&stage).join("keep"), b"synthetic foreign file").unwrap();
                *name.borrow_mut() = Some(stage);
            },
            || {},
        );
        assert!(matches!(outcome, Err(Error::PathChanged)));
        assert!(!parent.join("restored").exists());
        assert!(!moved.join("restored").exists());
        assert_eq!(
            fs::read(parent.join(name.borrow().as_ref().unwrap()).join("keep")).unwrap(),
            b"synthetic foreign file"
        );
        assert_eq!(fs::read(archive).unwrap(), archive_before);
    }
}

#[test]
fn a_substituted_staging_entry_is_preserved_and_never_published() {
    let _guard = crate::publication_tests::PROCESS_TESTS.lock().unwrap();
    for restoring in [false, true] {
        for symbolic in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let parent = temp.path().join("outputs");
            fs::create_dir(&parent).unwrap();
            let mut database = Database::create(temp.path().join("source")).unwrap();
            let archive = temp.path().join("source.backup");
            create(&mut database, &archive).unwrap();
            let foreign = temp.path().join("foreign");
            if restoring {
                fs::create_dir(&foreign).unwrap();
                fs::write(foreign.join("keep"), b"foreign directory").unwrap();
            } else {
                fs::copy(&archive, &foreign).unwrap();
            }
            let detached = temp.path().join("detached-stage");
            let name = std::cell::RefCell::new(None);
            let substitute = || {
                let stage = staging_name(&parent);
                fs::rename(parent.join(&stage), &detached).unwrap();
                let replacement = parent.join(&stage);
                if symbolic {
                    std::os::unix::fs::symlink(&foreign, &replacement).unwrap();
                } else if restoring {
                    fs::create_dir(&replacement).unwrap();
                    fs::write(replacement.join("keep"), b"foreign directory").unwrap();
                } else {
                    fs::copy(&foreign, &replacement).unwrap();
                }
                *name.borrow_mut() = Some(stage);
            };
            let target = parent.join("selected");
            let result = if restoring {
                restore::restore_with(&archive, &target, substitute, || {})
            } else {
                files::create_with(&mut database, &target, substitute, || {})
            };
            assert!(matches!(result, Err(Error::PathChanged)));
            assert!(!target.exists());
            let replacement = parent.join(name.borrow().as_ref().unwrap());
            if symbolic {
                assert!(
                    fs::symlink_metadata(&replacement)
                        .unwrap()
                        .file_type()
                        .is_symlink()
                );
            }
            if restoring {
                assert_eq!(
                    fs::read(replacement.join("keep")).unwrap(),
                    b"foreign directory"
                );
                assert!(detached.join("redo.wal").is_file());
            } else {
                assert_eq!(fs::read(replacement).unwrap(), fs::read(&archive).unwrap());
                assert_eq!(inspect(&detached).unwrap(), inspect(&archive).unwrap());
            }
            // Cleanup must never chase an alias or guess where a detached inode went.
            assert!(detached.exists());
        }
    }
}

#[test]
fn changed_parent_after_publication_reports_unknown_and_preserves_the_selection() {
    let _guard = crate::publication_tests::PROCESS_TESTS.lock().unwrap();
    for restoring in [false, true] {
        for symbolic in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let parent = temp.path().join("outputs");
            let moved = temp.path().join("moved");
            fs::create_dir(&parent).unwrap();
            let mut database = Database::create(temp.path().join("source")).unwrap();
            database.compact().unwrap();
            let archive = temp.path().join("source.backup");
            let expected = create(&mut database, &archive).unwrap();
            let target = parent.join("selected");
            let substitute = || {
                fs::rename(&parent, &moved).unwrap();
                if symbolic {
                    std::os::unix::fs::symlink(&moved, &parent).unwrap();
                } else {
                    fs::create_dir(&parent).unwrap();
                    fs::write(parent.join("keep"), b"replacement parent").unwrap();
                }
            };
            let result = if restoring {
                restore::restore_with(&archive, &target, || {}, substitute)
            } else {
                files::create_with(&mut database, &target, || {}, substitute)
            };
            assert!(matches!(result, Err(Error::PublicationUnknown(_))));
            if restoring {
                let restored = Database::open(moved.join("selected")).unwrap();
                assert_eq!(restored.database_id(), expected.database_id);
                assert_eq!(restored.last_transaction(), expected.last_transaction);
            } else {
                assert_eq!(inspect(moved.join("selected")).unwrap(), expected);
            }
            if symbolic {
                assert!(
                    fs::symlink_metadata(parent)
                        .unwrap()
                        .file_type()
                        .is_symlink()
                );
            } else {
                assert!(!target.exists());
                assert_eq!(
                    fs::read(parent.join("keep")).unwrap(),
                    b"replacement parent"
                );
            }
        }
    }
}

#[test]
fn backup_and_restore_sync_failures_preserve_source_and_report_publication_boundary() {
    let _guard = crate::publication_tests::PROCESS_TESTS.lock().unwrap();
    for restoring in [false, true] {
        let phases: &[&str] = if restoring {
            &["restore_wal_sync", "restore_directory_sync", "parent_sync"]
        } else {
            &["backup_file_sync", "parent_sync"]
        };
        for &phase in phases {
            for after in [false, true] {
                for compacted in [false, true] {
                    let temp = tempfile::tempdir().unwrap();
                    let source = temp.path().join("source");
                    let mut database = Database::create(&source).unwrap();
                    if compacted {
                        database.compact().unwrap();
                    }
                    let archive = temp.path().join("source.backup");
                    let expected = create(&mut database, &archive).unwrap();
                    let source_before = fs::read(source.join("redo.wal")).unwrap();
                    let archive_before = fs::read(&archive).unwrap();
                    let target = temp.path().join("selected");
                    let failure = FailureGuard::new(phase, after);
                    let result = if restoring {
                        crate::restore(&archive, &target)
                    } else {
                        create(&mut database, &target)
                    };
                    drop(failure);
                    if phase == "parent_sync" {
                        assert!(matches!(result, Err(Error::PublicationUnknown(_))));
                        if restoring {
                            let restored = Database::open(&target).unwrap();
                            assert_eq!(restored.database_id(), expected.database_id);
                            assert_eq!(restored.last_transaction(), expected.last_transaction);
                        } else {
                            assert_eq!(inspect(&target).unwrap(), expected);
                        }
                        let retry = if restoring {
                            crate::restore(&archive, &target)
                        } else {
                            create(&mut database, &target)
                        };
                        assert!(retry.is_err());
                    } else {
                        assert!(matches!(result, Err(Error::Io(_))));
                        assert!(!target.exists());
                    }
                    assert_eq!(fs::read(source.join("redo.wal")).unwrap(), source_before);
                    assert_eq!(fs::read(&archive).unwrap(), archive_before);
                    assert!(!fs::read_dir(temp.path()).unwrap().any(|entry| {
                        entry
                            .unwrap()
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".emilybase-backup-")
                    }));
                    let retry = temp.path().join("verified-retry");
                    assert_eq!(
                        if restoring {
                            crate::restore(&archive, retry)
                        } else {
                            create(&mut database, retry)
                        }
                        .unwrap(),
                        expected
                    );
                    assert!(database.view().is_ok());
                }
            }
        }
    }
}

#[test]
fn public_file_inspection_rejects_links_directories_and_fifos_without_reading() {
    let _guard = crate::publication_tests::PROCESS_TESTS.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut database = Database::create(temp.path().join("source")).unwrap();
    let archive = temp.path().join("synthetic.backup");
    create(&mut database, &archive).unwrap();
    let link = temp.path().join("alias");
    std::os::unix::fs::symlink(&archive, &link).unwrap();
    assert!(inspect(&link).is_err());
    assert!(matches!(inspect(temp.path()), Err(Error::Path)));
    let fifo = temp.path().join("fifo");
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        0,
    )
    .unwrap();
    assert!(matches!(inspect(&fifo), Err(Error::Path)));
    assert!(matches!(inspect("/dev/null"), Err(Error::Path)));
    for input in [&link, &fifo] {
        let target = temp.path().join("must-not-exist");
        assert!(crate::restore(input, &target).is_err());
        assert!(!target.exists());
    }
}

#[test]
fn destination_symlinks_missing_parents_and_root_paths_cannot_stage() {
    let _guard = crate::publication_tests::PROCESS_TESTS.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("outputs");
    fs::create_dir(&parent).unwrap();
    let link = temp.path().join("alias");
    std::os::unix::fs::symlink(&parent, &link).unwrap();
    let mut database = Database::create(temp.path().join("source")).unwrap();
    let archive = temp.path().join("source.backup");
    create(&mut database, &archive).unwrap();
    for target in [
        link.join("selected"),
        temp.path().join("missing").join("selected"),
        Path::new("/").to_path_buf(),
        Path::new(".").to_path_buf(),
    ] {
        assert!(create(&mut database, &target).is_err());
        assert!(crate::restore(&archive, &target).is_err());
    }
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
    assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
}

#[test]
fn unicode_destinations_preserve_original_archive_bytes_and_private_modes() {
    let _guard = crate::publication_tests::PROCESS_TESTS.lock().unwrap();
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("каталог 界");
    fs::create_dir(&parent).unwrap();
    for compacted in [false, true] {
        let mut database = Database::create(parent.join(format!("исходник-{compacted}"))).unwrap();
        if compacted {
            database.compact().unwrap();
        }
        let expected = crate::encode(&database.committed_wal().unwrap()).unwrap();
        let archive = parent.join(format!("копия-{compacted} 🔑.backup"));
        let report = create(&mut database, &archive).unwrap();
        assert_eq!(fs::read(&archive).unwrap(), expected);
        assert_eq!(
            fs::metadata(&archive).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let target = parent.join(format!("восстановление-{compacted}"));
        assert_eq!(crate::restore(&archive, &target).unwrap(), report);
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::read(target.join("redo.wal")).unwrap(),
            expected[crate::HEADER_SIZE..]
        );
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(32))]
    #[test]
    fn generated_sync_failures_keep_exact_model_and_independent_restore(
        compacted in proptest::bool::ANY,
        restoring in proptest::bool::ANY,
        after in proptest::bool::ANY,
        phase_number in 0usize..3,
        entries in proptest::collection::vec((-500i64..500, ".{0,24}"), 1..18),
    ) {
        use emilybase_catalog::{Column, DataType, Schema, Value};
        use std::collections::BTreeMap;
        let _guard = crate::publication_tests::PROCESS_TESTS.lock().unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("source");
        let mut database = Database::create(&source).unwrap();
        let mut tx = database.begin().unwrap();
        tx.create_table(Schema {
            name: "items".into(),
            columns: vec![
                Column { name: "id".into(), data_type: DataType::Integer, nullable: false },
                Column { name: "text".into(), data_type: DataType::Text, nullable: false },
            ],
            primary_key: 0,
        }).unwrap();
        let model: BTreeMap<i64, String> = entries.into_iter().collect();
        for (id, text) in &model {
            tx.insert("items", vec![Value::Integer(*id), Value::Text(text.clone())]).unwrap();
        }
        tx.commit().unwrap();
        if compacted { database.compact().unwrap(); }
        let rows = model.iter().map(|(id, text)| vec![Value::Integer(*id), Value::Text(text.clone())]).collect::<Vec<_>>();
        let source_before = database.committed_wal().unwrap();
        let archive = temporary.path().join("source.backup");
        let expected = create(&mut database, &archive).unwrap();
        let archive_before = fs::read(&archive).unwrap();
        let phases: &[&str] = if restoring {
            &["restore_wal_sync", "restore_directory_sync", "parent_sync"]
        } else {
            &["backup_file_sync", "parent_sync"]
        };
        let phase = phases[phase_number % phases.len()];
        let target = temporary.path().join("selected");
        let failure = FailureGuard::new(phase, after);
        let result = if restoring { crate::restore(&archive, &target) } else { create(&mut database, &target) };
        drop(failure);
        proptest::prop_assert!(result.is_err());
        proptest::prop_assert_eq!(target.exists(), phase == "parent_sync");
        proptest::prop_assert_eq!(fs::read(&archive).unwrap(), archive_before);
        proptest::prop_assert_eq!(database.committed_wal().unwrap(), source_before);
        proptest::prop_assert_eq!(database.view().unwrap().scan("items", 100).unwrap(), rows.clone());
        let verified_archive = if restoring { archive } else {
            let path = temporary.path().join("verified.backup");
            proptest::prop_assert_eq!(create(&mut database, &path).unwrap(), expected.clone());
            path
        };
        let restored_path = temporary.path().join("verified-restore");
        proptest::prop_assert_eq!(crate::restore(verified_archive, &restored_path).unwrap(), expected);
        let mut restored = Database::open(&restored_path).unwrap();
        proptest::prop_assert_eq!(restored.view().unwrap().scan("items", 100).unwrap(), rows);
        let mut tx = restored.begin().unwrap();
        tx.insert("items", vec![Value::Integer(1000), Value::Text("independent subsequent write".into())]).unwrap();
        tx.commit().unwrap();
        drop(restored);
        proptest::prop_assert_eq!(Database::open(restored_path).unwrap().view().unwrap().row_count(), model.len() + 1);
        proptest::prop_assert_eq!(database.view().unwrap().row_count(), model.len());
    }
}

#[test]
fn a_waiting_native_publisher_rejects_external_parent_and_staging_substitution() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    struct Worker(Child);
    impl Drop for Worker {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _guard = crate::publication_tests::PROCESS_TESTS.lock().unwrap();
    for restoring in [false, true] {
        for compacted in [false, true] {
            for replace_parent in [false, true] {
                let temporary = tempfile::tempdir().unwrap();
                let source = temporary.path().join("source");
                let mut database = Database::create(&source).unwrap();
                if compacted {
                    database.compact().unwrap();
                }
                let archive = temporary.path().join("source.backup");
                let expected = create(&mut database, &archive).unwrap();
                let source_before = database.committed_wal().unwrap();
                let archive_before = fs::read(&archive).unwrap();
                drop(database);
                let parent = temporary.path().join("outputs");
                fs::create_dir(&parent).unwrap();
                let target = parent.join("selected");
                let input = if restoring { &archive } else { &source };
                let mut worker = Worker(
                    Command::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            "publication_tests::publication_worker",
                            "--nocapture",
                            "--ignored",
                        ])
                        .env("EMILYBASE_PUBLICATION_INPUT", input)
                        .env("EMILYBASE_PUBLICATION_TARGET", &target)
                        .env(
                            "EMILYBASE_PUBLICATION_KIND",
                            if restoring { "restore" } else { "backup" },
                        )
                        .env("EMILYBASE_PUBLICATION_PHASE", "synced")
                        .env("EMILYBASE_PUBLICATION_CHANGED", "1")
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .spawn()
                        .unwrap(),
                );
                let stdout = worker.0.stdout.take().unwrap();
                let (sender, receiver) = mpsc::channel();
                let reader = std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                        if line == "READY" {
                            let _ = sender.send(());
                        }
                    }
                });
                receiver.recv_timeout(Duration::from_secs(10)).unwrap();
                let stage = staging_name(&parent);
                let detached = temporary.path().join("detached");
                if replace_parent {
                    fs::rename(&parent, &detached).unwrap();
                    fs::create_dir(&parent).unwrap();
                } else {
                    fs::rename(parent.join(&stage), &detached).unwrap();
                }
                let foreign = parent.join(&stage);
                if restoring {
                    fs::create_dir(&foreign).unwrap();
                    fs::write(foreign.join("keep"), b"foreign synthetic directory").unwrap();
                } else {
                    fs::copy(&archive, &foreign).unwrap();
                }
                worker.0.stdin.take().unwrap().write_all(b"x").unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                let status = loop {
                    if let Some(status) = worker.0.try_wait().unwrap() {
                        break status;
                    }
                    assert!(Instant::now() < deadline, "publisher did not return");
                    std::thread::sleep(Duration::from_millis(5));
                };
                reader.join().unwrap();
                assert!(status.success());
                assert!(!target.exists());
                if restoring {
                    assert_eq!(
                        fs::read(foreign.join("keep")).unwrap(),
                        b"foreign synthetic directory"
                    );
                } else {
                    assert_eq!(inspect(&foreign).unwrap(), expected);
                }
                if replace_parent {
                    assert_eq!(fs::read_dir(&detached).unwrap().count(), 0);
                } else if restoring {
                    assert_eq!(
                        Database::open(&detached).unwrap().database_id(),
                        expected.database_id
                    );
                } else {
                    assert_eq!(inspect(&detached).unwrap(), expected);
                }
                assert_eq!(fs::read(source.join("redo.wal")).unwrap(), source_before);
                assert_eq!(fs::read(&archive).unwrap(), archive_before);
            }
        }
    }
}

#[test]
fn prepared_restore_sync_failures_preserve_source_and_the_transformed_publication_boundary() {
    let _guard = crate::publication_tests::PROCESS_TESTS.lock().unwrap();
    use crate::PreparedRestoreError;
    use emilybase_catalog::{Column, DataType, Schema};
    for phase in ["restore_wal_sync", "restore_directory_sync", "parent_sync"] {
        for after in [false, true] {
            for compacted in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let source = temp.path().join("source");
                let mut database = Database::create(&source).unwrap();
                if compacted {
                    database.compact().unwrap();
                }
                let archive = temp.path().join("source.backup");
                let original = create(&mut database, &archive).unwrap();
                let archive_before = fs::read(&archive).unwrap();
                let source_before = database.committed_wal().unwrap();
                let target = temp.path().join("selected");
                let transform = |path: &Path| {
                    let mut database = Database::open(path).unwrap();
                    let mut tx = database.begin().unwrap();
                    tx.create_table(Schema {
                        name: "synthetic".into(),
                        columns: vec![Column {
                            name: "id".into(),
                            data_type: DataType::Integer,
                            nullable: false,
                        }],
                        primary_key: 0,
                    })
                    .unwrap();
                    tx.commit().unwrap();
                    Ok::<(), std::convert::Infallible>(())
                };
                let failure = FailureGuard::new(phase, after);
                let result = crate::restore_prepared(&archive, &target, transform);
                drop(failure);
                if phase == "parent_sync" {
                    assert!(matches!(
                        result,
                        Err(PreparedRestoreError::Backup(Error::PublicationUnknown(_)))
                    ));
                    let restored = Database::open(&target).unwrap();
                    assert_eq!(restored.database_id(), original.database_id);
                    assert_eq!(restored.last_transaction(), original.last_transaction + 1);
                    assert_eq!(restored.view().unwrap().table_count(), 1);
                    assert!(crate::restore_prepared(&archive, &target, transform).is_err());
                } else {
                    assert!(matches!(
                        result,
                        Err(PreparedRestoreError::Backup(Error::Io(_)))
                    ));
                    assert!(!target.exists());
                }
                assert_eq!(fs::read(&archive).unwrap(), archive_before);
                assert_eq!(database.committed_wal().unwrap(), source_before);
                assert!(!fs::read_dir(temp.path()).unwrap().any(|entry| {
                    entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".emilybase-backup-")
                }));
                let retry = temp.path().join("verified-retry");
                let report = crate::restore_prepared(&archive, &retry, transform).unwrap();
                assert_eq!(report.last_transaction, original.last_transaction + 1);
                assert_eq!(
                    Database::open(retry).unwrap().view().unwrap().table_count(),
                    1
                );
            }
        }
    }
}
