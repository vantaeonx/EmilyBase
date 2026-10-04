use super::*;
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use std::fs;
use std::path::Path;

fn seeded(path: &Path, compacted: bool) -> Database {
    let mut database = Database::create(path).unwrap();
    let mut transaction = database.begin().unwrap();
    transaction
        .create_table(Schema {
            name: "items".into(),
            columns: vec![
                Column {
                    name: "id".into(),
                    data_type: DataType::Integer,
                    nullable: false,
                },
                Column {
                    name: "value".into(),
                    data_type: DataType::Text,
                    nullable: false,
                },
            ],
            primary_key: 0,
        })
        .unwrap();
    transaction
        .insert(
            "items",
            vec![Value::Integer(7), Value::Text("synthetic seed".into())],
        )
        .unwrap();
    transaction
        .insert(
            "items",
            vec![Value::Integer(8), Value::Text("界".repeat(900))],
        )
        .unwrap();
    transaction.commit().unwrap();
    if compacted {
        database.compact().unwrap();
    }
    for number in 0..6 {
        let mut transaction = database.begin().unwrap();
        transaction
            .update(
                "items",
                &Key::Integer(7),
                vec![
                    Value::Integer(7),
                    Value::Text(format!("synthetic revision {number}")),
                ],
            )
            .unwrap();
        transaction.commit().unwrap();
    }
    database
}

fn foreign_image(database: &Database) -> Vec<u8> {
    emilybase_wal::encode_snapshot(
        [99; 16],
        database.last_transaction(),
        &database
            .view()
            .unwrap()
            .pages()
            .cloned()
            .collect::<Vec<_>>(),
    )
    .unwrap()
}

#[test]
fn moved_database_compaction_never_removes_or_overwrites_the_replacement_directory() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for compacted in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("database");
        let moved = temporary.path().join("owned-database");
        let mut database = seeded(&path, compacted);
        let id = database.database_id();
        let transaction = database.last_transaction();
        let expected = database
            .view()
            .unwrap()
            .pages()
            .cloned()
            .collect::<Vec<_>>();
        fs::rename(&path, &moved).unwrap();
        fs::create_dir(&path).unwrap();
        let foreign = foreign_image(&database);
        fs::write(path.join("redo.wal"), &foreign).unwrap();
        fs::write(path.join("redo-next.wal"), b"foreign pending journal").unwrap();
        let report = database.compact().unwrap();
        assert_eq!(fs::read(path.join("redo.wal")).unwrap(), foreign);
        assert_eq!(
            fs::read(path.join("redo-next.wal")).ok().as_deref(),
            Some(&b"foreign pending journal"[..])
        );
        assert_eq!(report.transaction, transaction);
        let selected = database.committed_wal().unwrap();
        assert_eq!(fs::read(moved.join("redo.wal")).unwrap(), selected);
        assert_eq!(recover_image(&selected, Some(id)).unwrap().wal_version, 2);
        assert!(!moved.join("redo-next.wal").exists());
        let mut write = database.begin().unwrap();
        write
            .insert(
                "items",
                vec![Value::Integer(9), Value::Text("independent write".into())],
            )
            .unwrap();
        assert_eq!(write.commit().unwrap(), transaction + 1);
        database.checkpoint().unwrap();
        drop(database);
        let reopened = Database::open_bound(&moved, Some(id)).unwrap();
        assert_eq!(reopened.last_transaction(), transaction + 1);
        assert_eq!(reopened.view().unwrap().row_count(), 3);
        assert_eq!(
            recover_image(&selected, Some(id))
                .unwrap()
                .snapshot
                .pages()
                .cloned()
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(fs::read(path.join("redo.wal")).unwrap(), foreign);
    }
}

#[test]
fn replaced_compaction_staging_cannot_be_selected_or_removed_by_cleanup() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for compacted in [false, true] {
        for symbolic in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let path = temporary.path().join("database");
            let detached = temporary.path().join("detached-baseline");
            let protected = temporary.path().join("foreign-baseline");
            let mut database = seeded(&path, compacted);
            let old = database.committed_wal().unwrap();
            let foreign = foreign_image(&database);
            fs::write(&protected, &foreign).unwrap();
            let result = database.compact_with(
                || {
                    fs::rename(path.join("redo-next.wal"), &detached).unwrap();
                    if symbolic {
                        std::os::unix::fs::symlink(&protected, path.join("redo-next.wal")).unwrap();
                    } else {
                        fs::write(path.join("redo-next.wal"), &foreign).unwrap();
                    }
                },
                || {},
                File::sync_all,
            );
            assert!(result.is_err(), "foreign staging was acknowledged");
            assert_eq!(fs::read(path.join("redo.wal")).unwrap(), old);
            assert_eq!(database.committed_wal().unwrap(), old);
            assert_eq!(fs::read(path.join("redo-next.wal")).unwrap(), foreign);
            assert_eq!(fs::read(&protected).unwrap(), foreign);
            let original =
                recover_image(&fs::read(detached).unwrap(), Some(database.database_id())).unwrap();
            assert_eq!(original.last_transaction, database.last_transaction());
            assert_eq!(
                original.snapshot.pages().cloned().collect::<Vec<_>>(),
                database
                    .view()
                    .unwrap()
                    .pages()
                    .cloned()
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn replaced_selected_baseline_reports_uncertainty_and_poisons_the_detached_owner() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for compacted in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("database");
        let detached = temporary.path().join("detached-selected");
        let mut database = seeded(&path, compacted);
        let id = database.database_id();
        let transaction = database.last_transaction();
        let expected = database
            .view()
            .unwrap()
            .pages()
            .cloned()
            .collect::<Vec<_>>();
        let foreign = foreign_image(&database);
        let result = database.compact_with(
            || {},
            || {
                fs::rename(path.join("redo.wal"), &detached).unwrap();
                fs::write(path.join("redo.wal"), &foreign).unwrap();
            },
            File::sync_all,
        );
        assert!(matches!(result, Err(Error::MaintenanceUnknown(_))));
        assert!(matches!(database.begin(), Err(Error::Poisoned)));
        assert!(matches!(database.committed_wal(), Err(Error::Poisoned)));
        assert_eq!(fs::read(path.join("redo.wal")).unwrap(), foreign);
        assert!(matches!(
            Database::open(&path),
            Err(Error::Wal(emilybase_wal::Error::Busy))
        ));
        drop(database);
        assert!(Database::open_bound(&path, Some(id)).is_err());
        let recovered = recover_image(&fs::read(&detached).unwrap(), Some(id)).unwrap();
        assert_eq!(recovered.last_transaction, transaction);
        assert_eq!(
            recovered.snapshot.pages().cloned().collect::<Vec<_>>(),
            expected
        );
        // Explicit synthetic operator repair after inspecting the selected outcome.
        fs::rename(detached, path.join("redo.wal")).unwrap();
        let reopened = Database::open_bound(path, Some(id)).unwrap();
        assert_eq!(reopened.last_transaction(), transaction);
        assert_eq!(
            reopened
                .view()
                .unwrap()
                .pages()
                .cloned()
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn same_inode_baseline_rewrites_cannot_change_the_acknowledged_snapshot() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for after_rename in [false, true] {
        for damage in 0..3 {
            let temporary = tempfile::tempdir().unwrap();
            let path = temporary.path().join("database");
            let mut database = seeded(&path, false);
            let old = database.committed_wal().unwrap();
            let mut changed = database.view().unwrap().clone();
            changed
                .apply(emilybase_database::Event {
                    table_id: 1,
                    kind: emilybase_database::EventKind::Replace(vec![
                        Value::Integer(7),
                        Value::Text("valid foreign revision".into()),
                    ]),
                })
                .unwrap();
            let changed_bytes = emilybase_wal::encode_snapshot(
                database.database_id(),
                database.last_transaction(),
                &changed.pages().cloned().collect::<Vec<_>>(),
            )
            .unwrap();
            let mutate = || {
                let selected = path.join(if after_rename {
                    "redo.wal"
                } else {
                    "redo-next.wal"
                });
                let mut bytes = fs::read(&selected).unwrap();
                match damage {
                    0 => bytes.truncate(bytes.len() - 1),
                    1 => {
                        let end = bytes.len() - 1;
                        bytes[end] ^= 1;
                    }
                    _ => bytes = changed_bytes.clone(),
                }
                fs::write(selected, bytes).unwrap();
            };
            let result = database.compact_with(
                || {
                    if !after_rename {
                        mutate();
                    }
                },
                || {
                    if after_rename {
                        mutate();
                    }
                },
                File::sync_all,
            );
            if after_rename {
                assert!(matches!(result, Err(Error::MaintenanceUnknown(_))));
                assert!(matches!(database.view(), Err(Error::Poisoned)));
                assert!(path.join("redo.wal").exists());
            } else {
                assert!(
                    result.is_err(),
                    "changed baseline contents were acknowledged"
                );
                assert_eq!(database.committed_wal().unwrap(), old);
                assert_eq!(fs::read(path.join("redo.wal")).unwrap(), old);
                assert!(!path.join("redo-next.wal").exists());
            }
        }
    }
}

#[test]
fn replaced_authoritative_journal_poisons_owner_and_preserves_both_histories() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for compacted in [false, true] {
        for staged in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let path = temporary.path().join("database");
            let detached = temporary.path().join("detached-authority");
            let mut database = seeded(&path, compacted);
            let id = database.database_id();
            let old = database.committed_wal().unwrap();
            let foreign = foreign_image(&database);
            let replace = || {
                fs::rename(path.join("redo.wal"), &detached).unwrap();
                fs::write(path.join("redo.wal"), &foreign).unwrap();
            };
            if !staged {
                replace();
                fs::write(path.join("redo-next.wal"), b"preserved unrelated staging").unwrap();
            }
            let result = database.compact_with(
                || {
                    if staged {
                        replace();
                    }
                },
                || {},
                File::sync_all,
            );
            assert!(matches!(result, Err(Error::JournalOwnership)));
            assert!(matches!(database.begin(), Err(Error::Poisoned)));
            assert!(matches!(database.checkpoint(), Err(Error::Poisoned)));
            assert_eq!(fs::read(path.join("redo.wal")).unwrap(), foreign);
            assert_eq!(fs::read(&detached).unwrap(), old);
            if staged {
                assert!(!path.join("redo-next.wal").exists());
            } else {
                assert_eq!(
                    fs::read(path.join("redo-next.wal")).unwrap(),
                    b"preserved unrelated staging"
                );
            }
            drop(database);
            fs::rename(detached, path.join("redo.wal")).unwrap();
            let mut reopened = Database::open_bound(path, Some(id)).unwrap();
            assert_eq!(reopened.committed_wal().unwrap(), old);
        }
    }
}

#[test]
#[ignore = "compaction namespace subprocess helper invoked by its parent"]
fn namespace_worker() {
    use std::io::{Read, Write};
    let path = std::env::var_os("EMILYBASE_COMPACTION_OWNED_PATH").unwrap();
    let phase = std::env::var("EMILYBASE_COMPACTION_OWNED_PHASE").unwrap();
    let pause = || {
        println!("READY");
        std::io::stdout().flush().unwrap();
        assert_eq!(std::io::stdin().read(&mut [0]).unwrap(), 1);
    };
    let mut database = Database::open(path).unwrap();
    let result = database.compact_with(
        || {
            if phase == "staged" {
                pause();
            }
        },
        || {
            if phase == "renamed" {
                pause();
            }
        },
        File::sync_all,
    );
    match result {
        Ok(_) => println!("RESULT_OK"),
        Err(Error::JournalOwnership) => println!("RESULT_REJECTED"),
        Err(Error::MaintenanceUnknown(_)) => {
            assert!(matches!(database.begin(), Err(Error::Poisoned)));
            println!("RESULT_UNKNOWN");
        }
        Err(error) => panic!("unexpected test result: {error}"),
    }
}

#[test]
fn staging_or_selected_link_and_mode_changes_preserve_history_without_false_success() {
    use std::os::unix::fs::PermissionsExt;
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for after_rename in [false, true] {
        for extra_link in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let path = temporary.path().join("database");
            let alias = temporary.path().join("preserved-baseline-link");
            let mut database = seeded(&path, true);
            let id = database.database_id();
            let transaction = database.last_transaction();
            let before = database.committed_wal().unwrap();
            let mutate = || {
                let selected = path.join(if after_rename {
                    "redo.wal"
                } else {
                    "redo-next.wal"
                });
                if extra_link {
                    fs::hard_link(selected, &alias).unwrap();
                } else {
                    fs::set_permissions(selected, fs::Permissions::from_mode(0o644)).unwrap();
                }
            };
            let result = database.compact_with(
                || {
                    if !after_rename {
                        mutate();
                    }
                },
                || {
                    if after_rename {
                        mutate();
                    }
                },
                File::sync_all,
            );
            if after_rename {
                assert!(matches!(result, Err(Error::MaintenanceUnknown(_))));
                assert!(matches!(database.begin(), Err(Error::Poisoned)));
                let selected = fs::read(path.join("redo.wal")).unwrap();
                assert_eq!(
                    recover_image(&selected, Some(id)).unwrap().last_transaction,
                    transaction
                );
                if extra_link {
                    assert_eq!(fs::read(&alias).unwrap(), selected);
                    fs::remove_file(&alias).unwrap();
                } else {
                    fs::set_permissions(path.join("redo.wal"), fs::Permissions::from_mode(0o600))
                        .unwrap();
                }
                drop(database);
                assert_eq!(
                    Database::open_bound(&path, Some(id))
                        .unwrap()
                        .last_transaction(),
                    transaction
                );
            } else {
                assert!(matches!(result, Err(Error::JournalOwnership)));
                assert_eq!(database.committed_wal().unwrap(), before);
                assert_eq!(fs::read(path.join("redo.wal")).unwrap(), before);
                assert!(!path.join("redo-next.wal").exists());
                if extra_link {
                    assert_eq!(
                        recover_image(&fs::read(alias).unwrap(), Some(id))
                            .unwrap()
                            .last_transaction,
                        transaction
                    );
                }
            }
        }
    }
}

#[test]
fn native_parent_and_staging_substitutions_preserve_owned_acknowledged_histories() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};
    struct Worker(Child);
    impl Drop for Worker {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for compacted in [false, true] {
        for phase in ["staged", "renamed"] {
            for action in ["parent", "regular", "symlink"] {
                let temporary = tempfile::tempdir().unwrap();
                let path = temporary.path().join("database");
                let detached = temporary.path().join("detached");
                let mut database = seeded(&path, compacted);
                let id = database.database_id();
                let transaction = database.last_transaction();
                let expected = database
                    .view()
                    .unwrap()
                    .pages()
                    .cloned()
                    .collect::<Vec<_>>();
                let original = database.committed_wal().unwrap();
                let foreign = foreign_image(&database);
                drop(database);
                let mut worker = Worker(
                    Command::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            "compaction::ownership_tests::namespace_worker",
                            "--ignored",
                            "--nocapture",
                        ])
                        .env("EMILYBASE_COMPACTION_OWNED_PATH", &path)
                        .env("EMILYBASE_COMPACTION_OWNED_PHASE", phase)
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .spawn()
                        .unwrap(),
                );
                let stdout = worker.0.stdout.take().unwrap();
                let (sender, receiver) = std::sync::mpsc::channel();
                let reader = std::thread::spawn(move || {
                    let mut lines = Vec::new();
                    for line in BufReader::new(stdout)
                        .lines()
                        .map_while(std::result::Result::ok)
                    {
                        if line == "READY" {
                            sender.send(()).unwrap();
                        }
                        lines.push(line);
                    }
                    lines
                });
                receiver.recv_timeout(Duration::from_secs(10)).unwrap();
                let target = if phase == "staged" {
                    "redo-next.wal"
                } else {
                    "redo.wal"
                };
                if action == "parent" {
                    fs::rename(&path, &detached).unwrap();
                    fs::create_dir(&path).unwrap();
                    fs::write(path.join("redo.wal"), &foreign).unwrap();
                    fs::write(path.join("redo-next.wal"), b"foreign staging").unwrap();
                } else {
                    fs::rename(path.join(target), &detached).unwrap();
                    if action == "symlink" {
                        let protected = temporary.path().join("protected");
                        fs::write(&protected, &foreign).unwrap();
                        std::os::unix::fs::symlink(protected, path.join(target)).unwrap();
                    } else {
                        fs::write(path.join(target), &foreign).unwrap();
                    }
                }
                worker.0.stdin.take().unwrap().write_all(b"c").unwrap();
                let started = Instant::now();
                loop {
                    if let Some(status) = worker.0.try_wait().unwrap() {
                        assert!(status.success());
                        break;
                    }
                    assert!(started.elapsed() < Duration::from_secs(10));
                    std::thread::sleep(Duration::from_millis(10));
                }
                let lines = reader.join().unwrap();
                let actual = if action == "parent" {
                    assert!(lines.iter().any(|line| line == "RESULT_OK"));
                    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), foreign);
                    assert_eq!(
                        fs::read(path.join("redo-next.wal")).unwrap(),
                        b"foreign staging"
                    );
                    detached.clone()
                } else {
                    assert_eq!(fs::read(path.join(target)).unwrap(), foreign);
                    let recovered = recover_image(&fs::read(&detached).unwrap(), Some(id)).unwrap();
                    assert_eq!(recovered.last_transaction, transaction);
                    assert_eq!(
                        recovered.snapshot.pages().cloned().collect::<Vec<_>>(),
                        expected
                    );
                    if phase == "staged" {
                        assert!(lines.iter().any(|line| line == "RESULT_REJECTED"));
                        assert_eq!(fs::read(path.join("redo.wal")).unwrap(), original);
                    } else {
                        assert!(lines.iter().any(|line| line == "RESULT_UNKNOWN"));
                        assert!(Database::open_bound(&path, Some(id)).is_err());
                        // Explicit repair of synthetic state after verifying the detached baseline.
                        fs::rename(&detached, path.join("redo.wal")).unwrap();
                    }
                    path.clone()
                };
                let mut reopened = Database::open_bound(actual, Some(id)).unwrap();
                assert_eq!(reopened.last_transaction(), transaction);
                assert_eq!(
                    reopened
                        .view()
                        .unwrap()
                        .pages()
                        .cloned()
                        .collect::<Vec<_>>(),
                    expected
                );
                let mut next = reopened.begin().unwrap();
                next.insert(
                    "items",
                    vec![Value::Integer(9), Value::Text("native later write".into())],
                )
                .unwrap();
                assert_eq!(next.commit().unwrap(), transaction + 1);
            }
        }
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(32))]
    #[test]
    fn relocated_compactions_match_an_independent_live_row_model(
        compacted in proptest::bool::ANY,
        changes in proptest::collection::vec(proptest::num::i64::ANY,0..12),
    ) {
        let _serial=crate::PROCESS_TESTS.lock().unwrap();
        let temporary=tempfile::tempdir().unwrap();
        let mut path=temporary.path().join("database");
        let mut database=seeded(&path,compacted);
        let id=database.database_id();
        let mut transaction=database.last_transaction();
        let mut model=std::collections::BTreeMap::from([(7i64,"synthetic revision 5".to_owned()),(8,"界".repeat(900))]);
        let mut copies=Vec::new();
        for (number,value) in changes.iter().enumerate() {
            let text=format!("synthetic generated {value}");
            let mut write=database.begin().unwrap();
            write.update("items",&Key::Integer(7),vec![Value::Integer(7),Value::Text(text.clone())]).unwrap();
            transaction+=1;
            proptest::prop_assert_eq!(write.commit().unwrap(),transaction);
            model.insert(7,text);
            let expected=database.view().unwrap().pages().cloned().collect::<Vec<_>>();
            let old=path.clone();
            let moved=temporary.path().join(format!("moved-{number}"));
            let report=database.compact_with(||{
                fs::rename(&old,&moved).unwrap();fs::create_dir(&old).unwrap();
                fs::write(old.join("redo.wal"),b"foreign generated authority").unwrap();
                fs::write(old.join("redo-next.wal"),b"foreign generated staging").unwrap();
            },||{},File::sync_all).unwrap();
            path=moved;
            copies.push(old);
            proptest::prop_assert_eq!(report.transaction,transaction);
            let recovered=recover_image(&database.committed_wal().unwrap(),Some(id)).unwrap();
            proptest::prop_assert_eq!(recovered.last_transaction,transaction);
            proptest::prop_assert_eq!(recovered.snapshot.pages().cloned().collect::<Vec<_>>(),expected);
            let expected_rows=model.iter().map(|(id,text)|vec![Value::Integer(*id),Value::Text(text.clone())]).collect::<Vec<_>>();
            proptest::prop_assert_eq!(database.view().unwrap().scan("items",100).unwrap(),expected_rows);
            database.checkpoint().unwrap();
        }
        drop(database);
        let mut reopened=Database::open_bound(&path,Some(id)).unwrap();
        proptest::prop_assert_eq!(reopened.last_transaction(),transaction);
        for (key,text) in &model {
            proptest::prop_assert_eq!(reopened.view().unwrap().get("items",&Key::Integer(*key)).unwrap(),Some(&vec![Value::Integer(*key),Value::Text(text.clone())]));
        }
        for old in copies {
            proptest::prop_assert_eq!(fs::read(old.join("redo.wal")).unwrap(),b"foreign generated authority");
            proptest::prop_assert_eq!(fs::read(old.join("redo-next.wal")).unwrap(),b"foreign generated staging");
        }
        let mut later=reopened.begin().unwrap();
        later.insert("items",vec![Value::Integer(9),Value::Text("independent model write".into())]).unwrap();
        proptest::prop_assert_eq!(later.commit().unwrap(),transaction+1);
    }
}
