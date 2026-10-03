//! Private test-only filesystem boundaries; release code reads no test environment.
use crate::{Database, Error, MAX_INDEX_IMAGE_BYTES};
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use std::cell::RefCell;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

thread_local! {
    static FAULT: RefCell<Option<(&'static str, bool)>> = const {RefCell::new(None)};
    static MOVE: RefCell<Option<(PathBuf, PathBuf)>> = const {RefCell::new(None)};
}
pub(crate) fn fail(point: &str, after: bool) -> std::io::Result<()> {
    FAULT.with_borrow_mut(|fault| {
        if fault
            .as_ref()
            .is_some_and(|selected| selected.0 == point && selected.1 == after)
        {
            *fault = None;
            Err(std::io::Error::other("injected optional cache sync error"))
        } else {
            Ok(())
        }
    })
}
pub(crate) fn boundary(point: &str) {
    if point == "file_synced" {
        MOVE.with_borrow_mut(|selected| {
            if let Some((root, moved)) = selected.take() {
                fs::rename(&root, &moved).unwrap();
                fs::create_dir(&root).unwrap();
                fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
                fs::write(
                    root.join("primary-1.table-index"),
                    b"synthetic untouched replacement",
                )
                .unwrap();
            }
        });
    }
    if std::env::var("EMILYBASE_CACHE_TEST_BOUNDARY").as_deref() == Ok(point) {
        println!("CACHE_BOUNDARY {point}");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    }
}
fn initialized(path: &Path) -> Database {
    let mut database = Database::create(path).unwrap();
    let mut transaction = database.begin().unwrap();
    transaction
        .create_table(Schema {
            name: "t".into(),
            columns: vec![Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            }],
            primary_key: 0,
        })
        .unwrap();
    transaction.insert("t", vec![Value::Integer(1)]).unwrap();
    transaction.commit().unwrap();
    database
}
fn active(root: &Path) -> PathBuf {
    root.join("primary-1.table-index")
}
fn staging(root: &Path) -> usize {
    fs::read_dir(root)
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".emilybase-table-index-")
        })
        .count()
}
fn append(database: &mut Database) {
    let mut transaction = database.begin().unwrap();
    transaction.insert("t", vec![Value::Integer(2)]).unwrap();
    transaction.commit().unwrap();
}

#[test]
fn private_save_load_refresh_and_missing_cache_preserve_authoritative_journal() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut database = initialized(&path);
    assert_eq!(database.load_primary_index_cache("t").unwrap(), None);
    let wal = database.committed_wal().unwrap();
    let image = database.primary_index_image("t").unwrap();
    assert_eq!(database.save_primary_index_cache("t").unwrap().entries, 1);
    assert_eq!(
        fs::metadata(active(&path)).unwrap().permissions().mode() & 0o7777,
        0o600
    );
    assert_eq!(fs::read(active(&path)).unwrap(), image);
    assert_eq!(staging(&path), 0);
    assert_eq!(
        database
            .load_primary_index_cache("t")
            .unwrap()
            .unwrap()
            .entries,
        1
    );
    assert_eq!(database.committed_wal().unwrap(), wal);
    append(&mut database);
    assert!(database.load_primary_index_cache("t").is_err());
    assert_eq!(
        database.view().unwrap().get("t", &Key::Integer(2)).unwrap(),
        Some(&vec![Value::Integer(2)])
    );
    let expected = database.primary_index_image("t").unwrap();
    database.save_primary_index_cache("t").unwrap();
    assert_eq!(fs::read(active(&path)).unwrap(), expected);
    drop(database);
    let mut database = Database::open(&path).unwrap();
    assert_eq!(
        database
            .load_primary_index_cache("t")
            .unwrap()
            .unwrap()
            .entries,
        2
    );
    assert_eq!(staging(&path), 0);
}

#[test]
fn damaged_foreign_and_oversized_cache_preserve_files_and_never_poison_data() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut database = initialized(&path);
    let other = initialized(&dir.path().join("other"));
    database.save_primary_index_cache("t").unwrap();
    let valid = fs::read(active(&path)).unwrap();
    let mut damaged = valid.clone();
    damaged[150] ^= 1;
    for bytes in [
        vec![],
        valid[..100].to_vec(),
        damaged,
        other.primary_index_image("t").unwrap(),
    ] {
        fs::write(active(&path), &bytes).unwrap();
        assert!(database.load_primary_index_cache("t").is_err());
        assert!(database.save_primary_index_cache("t").is_err());
        assert_eq!(fs::read(active(&path)).unwrap(), bytes);
        assert_eq!(database.view().unwrap().row_count(), 1);
        assert_eq!(staging(&path), 0);
    }
    fs::OpenOptions::new()
        .write(true)
        .open(active(&path))
        .unwrap()
        .set_len(MAX_INDEX_IMAGE_BYTES as u64 + 1)
        .unwrap();
    assert!(database.load_primary_index_cache("t").is_err());
    assert!(database.save_primary_index_cache("t").is_err());
    assert_eq!(
        fs::metadata(active(&path)).unwrap().len(),
        MAX_INDEX_IMAGE_BYTES as u64 + 1
    );
    fs::remove_file(active(&path)).unwrap();
    append(&mut database);
    database.save_primary_index_cache("t").unwrap();
    assert_eq!(
        database
            .load_primary_index_cache("t")
            .unwrap()
            .unwrap()
            .entries,
        2
    );
}

#[test]
fn symlink_hardlink_directory_fifo_and_public_mode_are_rejected_without_following() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut database = initialized(&path);
    database.save_primary_index_cache("t").unwrap();
    let original = fs::read(active(&path)).unwrap();
    fs::set_permissions(active(&path), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(database.load_primary_index_cache("t").is_err());
    assert!(database.save_primary_index_cache("t").is_err());
    fs::set_permissions(active(&path), fs::Permissions::from_mode(0o600)).unwrap();
    let hard = dir.path().join("hard");
    fs::hard_link(active(&path), &hard).unwrap();
    assert!(database.load_primary_index_cache("t").is_err());
    assert!(database.save_primary_index_cache("t").is_err());
    assert_eq!(fs::read(&hard).unwrap(), original);
    fs::remove_file(hard).unwrap();
    fs::remove_file(active(&path)).unwrap();
    let outside = dir.path().join("outside");
    fs::write(&outside, b"synthetic private sentinel").unwrap();
    symlink(&outside, active(&path)).unwrap();
    assert!(database.load_primary_index_cache("t").is_err());
    assert!(database.save_primary_index_cache("t").is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"synthetic private sentinel");
    fs::remove_file(active(&path)).unwrap();
    fs::create_dir(active(&path)).unwrap();
    assert!(database.load_primary_index_cache("t").is_err());
    assert!(database.save_primary_index_cache("t").is_err());
    fs::remove_dir(active(&path)).unwrap();
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        active(&path),
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .unwrap();
    let start = Instant::now();
    assert!(database.load_primary_index_cache("t").is_err());
    assert!(database.save_primary_index_cache("t").is_err());
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(database.view().unwrap().row_count(), 1);
    fs::remove_file(active(&path)).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(database.save_primary_index_cache("t").is_err());
    assert!(database.load_primary_index_cache("t").is_err());
    assert!(!active(&path).exists());
}

#[test]
fn moved_root_after_file_sync_cannot_redirect_publication_or_cleanup() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let moved = dir.path().join("moved");
    let database = initialized(&path);
    let wal = fs::read(path.join("redo.wal")).unwrap();
    MOVE.with_borrow_mut(|selected| *selected = Some((path.clone(), moved.clone())));
    assert!(database.save_primary_index_cache("t").is_err());
    assert_eq!(
        fs::read(active(&path)).unwrap(),
        b"synthetic untouched replacement"
    );
    assert_eq!(fs::read(moved.join("redo.wal")).unwrap(), wal);
    assert!(!active(&moved).exists());
    assert_eq!(staging(&moved), 0);
    assert_eq!(database.view().unwrap().row_count(), 1);
}

#[test]
fn cache_sync_failures_preserve_wal_and_allow_relational_writes() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for existing in [false, true] {
        for point in ["file_sync", "directory_sync"] {
            for after in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("db");
                let mut database = initialized(&path);
                let old = if existing {
                    database.save_primary_index_cache("t").unwrap();
                    Some(fs::read(active(&path)).unwrap())
                } else {
                    None
                };
                append(&mut database);
                let wal = database.committed_wal().unwrap();
                FAULT.with_borrow_mut(|fault| *fault = Some((point, after)));
                let error = database.save_primary_index_cache("t").unwrap_err();
                if point == "file_sync" {
                    assert!(matches!(error, Error::Io(_)));
                    assert_eq!(fs::read(active(&path)).ok(), old);
                } else {
                    assert!(matches!(error, Error::CachePublicationUnknown(_)));
                    assert_eq!(
                        database
                            .load_primary_index_cache("t")
                            .unwrap()
                            .unwrap()
                            .entries,
                        2
                    );
                }
                assert_eq!(database.committed_wal().unwrap(), wal);
                assert_eq!(staging(&path), 0);
                let mut transaction = database.begin().unwrap();
                transaction.insert("t", vec![Value::Integer(3)]).unwrap();
                transaction.commit().unwrap();
                database.save_primary_index_cache("t").unwrap();
                assert_eq!(
                    database
                        .load_primary_index_cache("t")
                        .unwrap()
                        .unwrap()
                        .entries,
                    3
                );
            }
        }
    }
}

struct Worker(std::process::Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn kill_at(path: &Path, point: &str) {
    let mut worker = Worker(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "index_cache_tests::cache_worker",
                "--nocapture",
            ])
            .env("EMILYBASE_CACHE_TEST_PATH", path)
            .env("EMILYBASE_CACHE_TEST_BOUNDARY", point)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let output = worker.0.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            if send.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let reached = loop {
        match receive.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(line)) if line == format!("CACHE_BOUNDARY {point}") => break true,
            Ok(Ok(_)) => (),
            _ => break false,
        }
    };
    worker.0.kill().unwrap();
    assert!(!worker.0.wait().unwrap().success());
    drop(receive);
    reader.join().unwrap();
    assert!(reached, "cache boundary was not reached");
}

#[test]
fn eight_creation_replacement_kills_preserve_acknowledged_data_and_whole_optional_images() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    for existing in [false, true] {
        for point in ["file_synced", "renamed", "directory_synced", "ack"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("db");
            let mut database = initialized(&path);
            let old = if existing {
                database.save_primary_index_cache("t").unwrap();
                Some(fs::read(active(&path)).unwrap())
            } else {
                None
            };
            append(&mut database);
            let expected = database.primary_index_image("t").unwrap();
            let wal = database.committed_wal().unwrap();
            drop(database);
            kill_at(&path, point);
            assert_eq!(fs::read(path.join("redo.wal")).unwrap(), wal);
            let mut database = Database::open(&path).unwrap();
            assert_eq!(database.view().unwrap().row_count(), 2);
            if point == "file_synced" {
                assert_eq!(fs::read(active(&path)).ok(), old);
                if existing {
                    assert!(database.load_primary_index_cache("t").is_err());
                } else {
                    assert_eq!(database.load_primary_index_cache("t").unwrap(), None);
                }
            } else {
                assert_eq!(fs::read(active(&path)).unwrap(), expected);
                assert_eq!(
                    database
                        .load_primary_index_cache("t")
                        .unwrap()
                        .unwrap()
                        .entries,
                    2
                );
            }
            database.save_primary_index_cache("t").unwrap();
            assert_eq!(fs::read(active(&path)).unwrap(), expected);
        }
    }
}

#[test]
#[ignore = "subprocess helper invoked by cache publication kill tests"]
fn cache_worker() {
    let path = std::env::var_os("EMILYBASE_CACHE_TEST_PATH").unwrap();
    if std::env::var("EMILYBASE_CACHE_TEST_ACTION").as_deref() == Ok("counter") {
        for _ in 0..10 {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut database = loop {
                match Database::open(PathBuf::from(&path)) {
                    Ok(database) => break database,
                    Err(Error::Wal(emilybase_wal::Error::Busy)) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("private cache worker failed: {error}"),
                }
            };
            let key = database.view().unwrap().row_count() as i64 + 1;
            let mut transaction = database.begin().unwrap();
            transaction.insert("t", vec![Value::Integer(key)]).unwrap();
            transaction.commit().unwrap();
            database.save_primary_index_cache("t").unwrap();
        }
        return;
    }
    let database = Database::open(PathBuf::from(path)).unwrap();
    database.save_primary_index_cache("t").unwrap();
    boundary("ack");
}

#[test]
fn two_processes_publish_current_caches_under_the_same_relational_owner() {
    let _serial = crate::PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let database = initialized(&path);
    let initial_transaction = database.last_transaction();
    database.save_primary_index_cache("t").unwrap();
    drop(database);
    let spawn = || {
        Worker(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "index_cache_tests::cache_worker",
                    "--nocapture",
                ])
                .env("EMILYBASE_CACHE_TEST_PATH", &path)
                .env("EMILYBASE_CACHE_TEST_ACTION", "counter")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        )
    };
    let mut first = spawn();
    let mut second = spawn();
    let deadline = Instant::now() + Duration::from_secs(15);
    for worker in [&mut first, &mut second] {
        loop {
            if let Some(status) = worker.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "cache writers did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let mut database = Database::open(&path).unwrap();
    assert_eq!(database.last_transaction(), initial_transaction + 20);
    assert_eq!(database.view().unwrap().row_count(), 21);
    assert_eq!(
        database
            .load_primary_index_cache("t")
            .unwrap()
            .unwrap()
            .entries,
        21
    );
    for key in 1..=21 {
        assert_eq!(
            database
                .view()
                .unwrap()
                .get("t", &Key::Integer(key))
                .unwrap(),
            Some(&vec![Value::Integer(key)])
        );
    }
    assert_eq!(staging(&path), 0);
}
