use super::*;
use std::fs;

thread_local! {
    static FAULT: std::cell::Cell<Option<(&'static str,bool)>> = const {std::cell::Cell::new(None)};
}
struct FaultGuard;
impl FaultGuard {
    fn new(phase: &'static str, after: bool) -> Self {
        assert!(FAULT.replace(Some((phase, after))).is_none());
        Self
    }
}
impl Drop for FaultGuard {
    fn drop(&mut self) {
        FAULT.set(None);
    }
}
pub(crate) fn sync_failure(phase: &'static str, after: bool) -> std::io::Result<()> {
    if FAULT.get() == Some((phase, after)) {
        Err(std::io::Error::other(
            "synthetic initialization sync failure",
        ))
    } else {
        Ok(())
    }
}

fn foreign(path: &Path) -> Vec<u8> {
    let mut database = Database::create(path).unwrap();
    let bytes = database.committed_wal().unwrap();
    drop(database);
    bytes
}

#[test]
fn creation_cannot_follow_a_replacement_after_acquiring_directory_ownership() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("database");
    let detached = temporary.path().join("owned-directory");
    let result = Database::create_with(
        &path,
        || {
            fs::rename(&path, &detached).unwrap();
            fs::create_dir(&path).unwrap();
            fs::write(path.join("protected"), b"foreign directory content").unwrap();
        },
        || {},
    );
    assert!(
        result.is_err(),
        "replacement directory was initialized and acknowledged"
    );
    assert!(!path.join("redo.wal").exists());
    assert_eq!(
        fs::read(path.join("protected")).unwrap(),
        b"foreign directory content"
    );
    assert!(!detached.join("redo.wal").exists());
}

#[test]
fn opening_cannot_pair_original_directory_ownership_with_foreign_journal_data() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("database");
    let detached = temporary.path().join("owned-directory");
    let other = temporary.path().join("other");
    let original = foreign(&path);
    let substituted = foreign(&other);
    let result = Database::open_with(
        &path,
        None,
        || {
            fs::rename(&path, &detached).unwrap();
            fs::rename(&other, &path).unwrap();
        },
        || {},
    );
    assert!(
        result.is_err(),
        "foreign journal was paired with original ownership"
    );
    assert_eq!(fs::read(detached.join("redo.wal")).unwrap(), original);
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), substituted);
    drop(Database::open(&path).unwrap());
    drop(Database::open(&detached).unwrap());
}

#[test]
fn initialization_or_recovery_cannot_acknowledge_a_changed_named_directory() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for opening in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("database");
        let detached = temporary.path().join("detached");
        let other = temporary.path().join("other");
        let substituted = foreign(&other);
        if opening {
            foreign(&path);
        }
        let change = || {
            fs::rename(&path, &detached).unwrap();
            fs::rename(&other, &path).unwrap();
        };
        let result = if opening {
            Database::open_with(&path, None, || {}, change)
        } else {
            Database::create_with(&path, || {}, change)
        };
        assert!(result.is_err(), "changed named directory was acknowledged");
        assert_eq!(fs::read(path.join("redo.wal")).unwrap(), substituted);
        drop(Database::open(&detached).unwrap());
        drop(Database::open(&path).unwrap());
    }
}

#[test]
fn before_and_after_directory_sync_failures_retain_an_inspectable_initial_root() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for phase in ["directory_sync", "parent_sync"] {
        for after in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let path = temporary.path().join("database");
            let fault = FaultGuard::new(phase, after);
            assert!(matches!(
                Database::create(&path),
                Err(Error::InitializationUnknown(_))
            ));
            drop(fault);
            let before = fs::read(path.join("redo.wal")).unwrap();
            assert!(Database::create(&path).is_err());
            let mut reopened = Database::open(&path).unwrap();
            assert_eq!(reopened.last_transaction(), 1);
            assert!(reopened.view().unwrap().schemas().is_empty());
            assert_eq!(reopened.committed_wal().unwrap(), before);
            reopened.checkpoint().unwrap();
        }
    }
}

#[test]
fn managed_final_and_parent_aliases_and_wal_links_cannot_select_foreign_data() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    let before = foreign(&source);
    let alias = temporary.path().join("directory-alias");
    std::os::unix::fs::symlink(&source, &alias).unwrap();
    assert!(Database::open(&alias).is_err());
    assert!(Database::create(alias.join("unpublished")).is_err());
    assert!(!source.join("unpublished").exists());
    let copy = temporary.path().join("wal-copy");
    fs::rename(source.join("redo.wal"), &copy).unwrap();
    for symbolic in [false, true] {
        if symbolic {
            std::os::unix::fs::symlink(&copy, source.join("redo.wal")).unwrap();
        } else {
            fs::hard_link(&copy, source.join("redo.wal")).unwrap();
        }
        assert!(Database::open(&source).is_err());
        assert_eq!(fs::read(&copy).unwrap(), before);
        assert_eq!(fs::read(source.join("redo.wal")).unwrap(), before);
        fs::remove_file(source.join("redo.wal")).unwrap();
    }
    fs::rename(copy, source.join("redo.wal")).unwrap();
    assert_eq!(Database::open(&source).unwrap().last_transaction(), 1);
}

#[test]
fn changed_new_directory_or_journal_admission_cannot_return_a_private_database() {
    use std::os::unix::fs::PermissionsExt;
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for mode in ["directory", "wal", "link"] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("database");
        let alias = temporary.path().join("outside-link");
        let result = Database::create_with(
            &path,
            || {},
            || match mode {
                "directory" => {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap()
                }
                "wal" => {
                    fs::set_permissions(path.join("redo.wal"), fs::Permissions::from_mode(0o644))
                        .unwrap()
                }
                _ => fs::hard_link(path.join("redo.wal"), &alias).unwrap(),
            },
        );
        assert!(matches!(result, Err(Error::InitializationUnknown(_))));
        let bytes = fs::read(path.join("redo.wal")).unwrap();
        let recovered = crate::recover_image(&bytes, None).unwrap();
        assert_eq!(recovered.last_transaction, 1);
        assert_eq!(recovered.snapshot.row_count(), 0);
        if mode == "link" {
            assert_eq!(fs::read(&alias).unwrap(), bytes);
            fs::remove_file(alias).unwrap();
        }
        assert_eq!(Database::open(&path).unwrap().last_transaction(), 1);
    }
}

#[test]
#[ignore = "initialization namespace subprocess helper invoked by its parent"]
fn initialization_worker() {
    use std::io::{Read, Write};
    let path = PathBuf::from(std::env::var_os("EMILYBASE_INITIALIZATION_PATH").unwrap());
    let action = std::env::var("EMILYBASE_INITIALIZATION_ACTION").unwrap();
    let phase = std::env::var("EMILYBASE_INITIALIZATION_PHASE").unwrap();
    let pause = || {
        println!("READY");
        std::io::stdout().flush().unwrap();
        assert_eq!(std::io::stdin().read(&mut [0]).unwrap(), 1);
    };
    let result = if action == "create" {
        Database::create_with(
            &path,
            || {
                if phase == "owned" {
                    pause();
                }
            },
            || {
                if phase == "initialized" {
                    pause();
                }
            },
        )
    } else {
        Database::open_with(
            &path,
            None,
            || {
                if phase == "owned" {
                    pause();
                }
            },
            || {
                if phase == "initialized" {
                    pause();
                }
            },
        )
    };
    match result {
        Ok(database) => {
            println!("RESULT_OK");
            if phase == "returned" {
                pause();
            }
            drop(database);
        }
        Err(Error::DirectoryChanged) => println!("RESULT_REFUSED"),
        Err(Error::InitializationUnknown(_)) => println!("RESULT_UNKNOWN"),
        Err(error) => panic!("unexpected test result: {error}"),
    }
}

struct Worker(std::process::Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn worker(
    path: &Path,
    action: &str,
    phase: &str,
) -> (
    Worker,
    std::sync::mpsc::Receiver<()>,
    std::thread::JoinHandle<Vec<String>>,
) {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let mut worker = Worker(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "database::initialization_tests::initialization_worker",
                "--ignored",
                "--nocapture",
            ])
            .env("EMILYBASE_INITIALIZATION_PATH", path)
            .env("EMILYBASE_INITIALIZATION_ACTION", action)
            .env("EMILYBASE_INITIALIZATION_PHASE", phase)
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
    (worker, receiver, reader)
}

#[test]
fn native_kills_never_adopt_a_partial_root_and_release_initialized_ownership() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for phase in ["owned", "initialized", "returned"] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("database");
        let (mut worker, ready, reader) = worker(&path, "create", phase);
        ready
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        assert!(matches!(
            Database::open(&path),
            Err(Error::Wal(emilybase_wal::Error::Busy))
        ));
        worker.0.kill().unwrap();
        assert!(!worker.0.wait().unwrap().success());
        reader.join().unwrap();
        assert!(Database::create(&path).is_err());
        if phase == "owned" {
            assert!(Database::open(&path).is_err());
            assert!(!path.join("redo.wal").exists());
        } else {
            let mut database = Database::open(&path).unwrap();
            assert_eq!(database.last_transaction(), 1);
            assert_eq!(database.view().unwrap().row_count(), 0);
            database.checkpoint().unwrap();
            database.compact().unwrap();
            drop(database);
            assert_eq!(Database::open(&path).unwrap().last_transaction(), 1);
        }
    }
}

#[test]
fn native_namespace_changes_during_create_and_open_preserve_foreign_and_owned_roots() {
    use std::io::Write;
    use std::time::{Duration, Instant};
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for action in ["create", "open"] {
        for phase in ["owned", "initialized"] {
            for symbolic in [false, true] {
                let temporary = tempfile::tempdir().unwrap();
                let path = temporary.path().join("database");
                let detached = temporary.path().join("detached");
                let other = temporary.path().join("foreign");
                let before = foreign(&other);
                let original = if action == "open" {
                    Some(foreign(&path))
                } else {
                    None
                };
                let (mut worker, ready, reader) = worker(&path, action, phase);
                ready.recv_timeout(Duration::from_secs(10)).unwrap();
                fs::rename(&path, &detached).unwrap();
                if symbolic {
                    std::os::unix::fs::symlink(&other, &path).unwrap();
                } else {
                    fs::rename(&other, &path).unwrap();
                }
                worker.0.stdin.take().unwrap().write_all(b"c").unwrap();
                let start = Instant::now();
                loop {
                    if let Some(status) = worker.0.try_wait().unwrap() {
                        assert!(status.success());
                        break;
                    }
                    assert!(start.elapsed() < Duration::from_secs(10));
                    std::thread::sleep(Duration::from_millis(10));
                }
                let printed = reader.join().unwrap();
                assert!(printed.iter().any(|line| line
                    == if action == "create" && phase == "initialized" {
                        "RESULT_UNKNOWN"
                    } else {
                        "RESULT_REFUSED"
                    }));
                let foreign_path = if symbolic { &other } else { &path };
                assert_eq!(fs::read(foreign_path.join("redo.wal")).unwrap(), before);
                if let Some(original) = original {
                    assert_eq!(fs::read(detached.join("redo.wal")).unwrap(), original);
                }
                if action == "open" || phase == "initialized" {
                    drop(Database::open(detached).unwrap());
                } else {
                    assert!(!detached.join("redo.wal").exists());
                }
                drop(Database::open(foreign_path).unwrap());
            }
        }
    }
}

#[test]
fn initial_or_recovered_journal_substitution_preserves_both_inodes_and_releases_locks() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for opening in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("database");
        let other = temporary.path().join("other");
        let detached = temporary.path().join("detached-wal");
        let substituted = foreign(&other);
        if opening {
            foreign(&path);
        }
        let replace = || {
            fs::rename(path.join("redo.wal"), &detached).unwrap();
            fs::write(path.join("redo.wal"), &substituted).unwrap();
        };
        let result = if opening {
            Database::open_with(&path, None, || {}, replace)
        } else {
            Database::create_with(&path, || {}, replace)
        };
        if opening {
            assert!(matches!(result, Err(Error::JournalOwnership)));
        } else {
            assert!(matches!(result, Err(Error::InitializationUnknown(_))));
        }
        assert_eq!(fs::read(path.join("redo.wal")).unwrap(), substituted);
        let original = fs::read(&detached).unwrap();
        assert_eq!(
            crate::recover_image(&original, None)
                .unwrap()
                .last_transaction,
            1
        );
        assert_eq!(
            Database::open(&path).unwrap().database_id(),
            Database::open(&other).unwrap().database_id()
        );
        // Explicit synthetic operator repair after inspecting both retained images.
        fs::rename(detached, path.join("redo.wal")).unwrap();
        let mut reopened = Database::open(path).unwrap();
        assert_eq!(reopened.committed_wal().unwrap(), original);
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(32))]
    #[test]
    fn rejected_open_namespace_changes_preserve_independent_generated_row_models(
        values in proptest::collection::vec(proptest::num::i64::ANY,0..14),
        compacted in proptest::bool::ANY,
    ) {
        use emilybase_catalog::{Column,DataType,Schema,Value};
        let _serial=crate::PROCESS_TESTS.lock().unwrap();
        let temporary=tempfile::tempdir().unwrap();
        let source=temporary.path().join("source");
        let other=temporary.path().join("other");
        let detached=temporary.path().join("detached");
        let schema=Schema {name:"items".into(),columns:vec![
            Column{name:"id".into(),data_type:DataType::Integer,nullable:false},
            Column{name:"n".into(),data_type:DataType::Integer,nullable:false},
        ],primary_key:0};
        let rows=values.iter().enumerate().map(|(key,value)|vec![Value::Integer(key as i64),Value::Integer(*value)]).collect::<Vec<_>>();
        let foreign_rows=vec![vec![Value::Integer(9000),Value::Integer(222)]];
        let mut images=Vec::new();
        let mut identities=Vec::new();
        for (path,initial) in [(&source,&rows),(&other,&foreign_rows)] {
            let mut database=Database::create(path).unwrap();
            let mut write=database.begin().unwrap();write.create_table(schema.clone()).unwrap();
            for row in initial {write.insert("items",row.clone()).unwrap();}
            write.commit().unwrap();
            if compacted {database.compact().unwrap();}
            identities.push(database.database_id());
            images.push(database.committed_wal().unwrap());
        }
        let result=Database::open_with(&source,None,||{
            fs::rename(&source,&detached).unwrap();fs::rename(&other,&source).unwrap();
        },||{});
        proptest::prop_assert!(matches!(result,Err(Error::DirectoryChanged)));
        proptest::prop_assert!(fs::read(detached.join("redo.wal")).unwrap().as_slice()==images[0].as_slice());
        proptest::prop_assert!(fs::read(source.join("redo.wal")).unwrap().as_slice()==images[1].as_slice());
        let mut original=Database::open_bound(&detached,Some(identities[0])).unwrap();
        let foreign=Database::open_bound(&source,Some(identities[1])).unwrap();
        proptest::prop_assert_eq!(original.view().unwrap().scan("items",100).unwrap(),rows);
        proptest::prop_assert_eq!(foreign.view().unwrap().scan("items",100).unwrap(),foreign_rows);
        let mut next=original.begin().unwrap();
        next.insert("items",vec![Value::Integer(9000),Value::Integer(-222)]).unwrap();
        proptest::prop_assert_eq!(next.commit().unwrap(),3);
        original.checkpoint().unwrap();
        proptest::prop_assert!(fs::read(source.join("redo.wal")).unwrap().as_slice()==images[1].as_slice());
        drop(original);drop(foreign);
        proptest::prop_assert_eq!(Database::open_bound(detached,Some(identities[0])).unwrap().view().unwrap().row_count(),values.len()+1);
    }
}

#[test]
fn moved_creation_parent_cannot_initialize_or_sync_a_foreign_nested_database() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for initialized in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("outputs");
        let moved = temporary.path().join("owned-outputs");
        fs::create_dir(&parent).unwrap();
        let path = parent.join("database");
        let replace = || {
            fs::rename(&parent, &moved).unwrap();
            fs::create_dir(&parent).unwrap();
            fs::create_dir(&path).unwrap();
            fs::write(path.join("protected"), b"foreign nested database content").unwrap();
        };
        let result = Database::create_with(
            &path,
            || {
                if !initialized {
                    replace();
                }
            },
            || {
                if initialized {
                    replace();
                }
            },
        );
        if initialized {
            assert!(matches!(result, Err(Error::InitializationUnknown(_))));
        } else {
            assert!(matches!(result, Err(Error::DirectoryChanged)));
        }
        assert_eq!(
            fs::read(path.join("protected")).unwrap(),
            b"foreign nested database content"
        );
        assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
        let original = moved.join("database");
        if initialized {
            let before = fs::read(original.join("redo.wal")).unwrap();
            let mut reopened = Database::open(&original).unwrap();
            assert_eq!(reopened.last_transaction(), 1);
            assert_eq!(reopened.committed_wal().unwrap(), before);
        } else {
            assert!(Database::open(&original).is_err());
            assert!(!original.join("redo.wal").exists());
        }
        assert!(Database::create(&path).is_err());
    }
}
