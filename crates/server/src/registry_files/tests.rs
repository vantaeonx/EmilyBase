use super::*;
use crate::durability::{PROCESS_TESTS, inject};
use emilybase_catalog::Value;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::thread::JoinHandle;
use std::time::Duration;

fn seed(source: &Path) -> Vec<(String, String)> {
    let mut store = ProjectStore::open(source).unwrap();
    let mut credentials = Vec::new();
    for value in [17, 29] {
        let created = store.create("synthetic recovery project").unwrap();
        store
            .authorize(&created.project.id, &created.api_key)
            .unwrap()
            .execute(
                "CREATE TABLE t(id INT PRIMARY KEY,v INT); INSERT INTO t VALUES(1,$1)",
                &[Value::Integer(value)],
            )
            .unwrap();
        let rotated = store.rotate(&created.project.id).unwrap();
        credentials.push((created.project.id, rotated.api_key));
    }
    emilybase_transactions::Database::open(source.join(&credentials[0].0).join("data"))
        .unwrap()
        .compact()
        .unwrap();
    credentials
}

fn image(source: &Path) -> Vec<u8> {
    ProjectStore::open_existing(source)
        .unwrap()
        .backup_image()
        .unwrap()
}

fn usable(root: &Path, credentials: &[(String, String)], expected: &[u8]) {
    let mut restored = ProjectStore::open_existing(root).unwrap();
    assert_eq!(restored.backup_image().unwrap(), expected);
    for (i, (id, key)) in credentials.iter().enumerate() {
        let report = restored
            .authorize(id, key)
            .unwrap()
            .execute("SELECT * FROM t", &[])
            .unwrap();
        assert_eq!(report.transaction, 2);
        assert_eq!(
            report.results[0].rows,
            [vec![Value::Integer(1), Value::Integer([17, 29][i])]]
        );
        assert_eq!(
            restored
                .list()
                .unwrap()
                .iter()
                .find(|p| &p.id == id)
                .unwrap()
                .key_epoch,
            2
        );
        assert!(restored.authorize(id, &credentials[1 - i].1).is_err());
        let changed = restored
            .authorize(id, key)
            .unwrap()
            .execute("INSERT INTO t VALUES(2,41)", &[])
            .unwrap();
        assert_eq!(changed.transaction, 3);
    }
}

struct Worker {
    child: Child,
    lines: Receiver<String>,
    reader: Option<JoinHandle<()>>,
}
impl Worker {
    fn start(source: &Path, archive: &Path, target: &Path, action: &str, point: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "registry_files::tests::publication_worker",
                "--nocapture",
            ])
            .env("EMILYBASE_ARCHIVE_TEST_SOURCE", source)
            .env("EMILYBASE_ARCHIVE_TEST_FILE", archive)
            .env("EMILYBASE_ARCHIVE_TEST_TARGET", target)
            .env("EMILYBASE_ARCHIVE_TEST_ACTION", action)
            .env("EMILYBASE_REGISTRY_KILL_POINT", point)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, lines) = channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            lines,
            reader: Some(reader),
        }
    }
    fn reach(&self, point: &str) {
        let expected = format!("REGISTRY_BOUNDARY {point}");
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            let line = self
                .lines
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("registry archive boundary deadline");
            if line == expected {
                break;
            }
        }
    }
    fn kill(mut self) {
        self.child.kill().unwrap();
        assert!(!self.child.wait().unwrap().success());
        self.reader.take().unwrap().join().unwrap();
    }
    fn release(&mut self) {
        self.child.stdin.take().unwrap().write_all(b"1").unwrap();
    }
    fn finish(mut self) -> String {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "archive worker exit deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        self.reader.take().unwrap().join().unwrap();
        self.lines.try_iter().collect::<Vec<_>>().join("\n")
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[test]
#[ignore = "archive publication helper invoked by parent tests"]
fn publication_worker() {
    let Some(source) = std::env::var_os("EMILYBASE_ARCHIVE_TEST_SOURCE") else {
        return;
    };
    let source = PathBuf::from(source);
    let archive = PathBuf::from(std::env::var_os("EMILYBASE_ARCHIVE_TEST_FILE").unwrap());
    let target = PathBuf::from(std::env::var_os("EMILYBASE_ARCHIVE_TEST_TARGET").unwrap());
    match std::env::var("EMILYBASE_ARCHIVE_TEST_ACTION")
        .unwrap()
        .as_str()
    {
        "backup" => {
            ProjectStore::open_existing(source)
                .unwrap()
                .backup(&archive)
                .unwrap();
            checkpoint("registry_backup_ack");
        }
        "restore" => {
            restore_registry_backup(archive, target).unwrap();
            checkpoint("registry_restore_ack");
        }
        "race" => {
            println!("REGISTRY_BOUNDARY race_ready");
            std::io::stdout().flush().unwrap();
            std::io::stdin().read_exact(&mut [0; 1]).unwrap();
            match restore_registry_backup(archive, target) {
                Ok(_) => println!("RESTORE_OK"),
                Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    println!("RESTORE_CONFLICT")
                }
                Err(_) => panic!("unexpected restore race failure"),
            }
        }
        _ => panic!("invalid archive worker action"),
    }
}

#[test]
fn backup_kills_before_and_after_publication_preserve_every_source_commit() {
    let _serial = PROCESS_TESTS.blocking_lock();
    for point in [
        "registry_capture_owners_locked",
        "registry_backup_file_synced",
        "registry_backup_renamed",
        "registry_backup_parent_synced",
        "registry_backup_ack",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let credentials = seed(&source);
        let expected = image(&source);
        let archive = temp.path().join("projects.backup");
        Worker::start(
            &source,
            &archive,
            &temp.path().join("unused"),
            "backup",
            point,
        )
        .reach_then_kill(point);
        assert_eq!(image(&source), expected);
        if matches!(
            point,
            "registry_capture_owners_locked" | "registry_backup_file_synced"
        ) {
            assert!(!archive.exists());
            assert!(inspect_registry_backup(&archive).is_err());
        } else {
            assert_eq!(read(&archive).unwrap(), expected);
            let target = temp.path().join("restored");
            restore_registry_backup(&archive, &target).unwrap();
            usable(&target, &credentials, &expected);
        }
        // A killed archive publisher does not poison or modify its source registry.
        let mut reopened = ProjectStore::open_existing(&source).unwrap();
        reopened.create("after publisher kill").unwrap();
        assert_eq!(reopened.list().unwrap().len(), 3);
    }
}

#[test]
fn capture_holds_every_database_owner_together_and_releases_them_after_kill() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let credentials = seed(&source);
    let expected = image(&source);
    let archive = temp.path().join("unpublished.backup");
    let worker = Worker::start(
        &source,
        &archive,
        &temp.path().join("unused"),
        "backup",
        "registry_capture_owners_locked",
    );
    worker.reach("registry_capture_owners_locked");
    assert!(matches!(
        ProjectStore::open_existing(&source),
        Err(Error::Busy)
    ));
    for (id, _) in &credentials {
        let path = source.join(id).join("data");
        assert!(emilybase_transactions::Database::open(&path).is_err());
        assert!(!archive.exists());
    }
    worker.kill();
    assert_eq!(image(&source), expected);
    for (id, _) in &credentials {
        assert_eq!(
            emilybase_transactions::Database::open(source.join(id).join("data"))
                .unwrap()
                .last_transaction(),
            2
        );
    }
}
impl Worker {
    fn reach_then_kill(self, point: &str) {
        self.reach(point);
        self.kill();
    }
}

#[test]
fn restore_kills_never_expose_partial_registries_or_adopt_abandoned_staging() {
    let _serial = PROCESS_TESTS.blocking_lock();
    for point in [
        "registry_restore_wal_synced",
        "registry_restore_project_synced",
        "registry_restore_stage_synced",
        "registry_restore_renamed",
        "registry_restore_parent_synced",
        "registry_restore_ack",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let credentials = seed(&source);
        let expected = image(&source);
        let archive = temp.path().join("projects.backup");
        ProjectStore::open_existing(&source)
            .unwrap()
            .backup(&archive)
            .unwrap();
        let target = temp.path().join("restored");
        Worker::start(&source, &archive, &target, "restore", point).reach_then_kill(point);
        let selected = matches!(
            point,
            "registry_restore_renamed" | "registry_restore_parent_synced" | "registry_restore_ack"
        );
        assert_eq!(target.exists(), selected);
        if selected {
            usable(&target, &credentials, &expected);
        } else {
            assert!(ProjectStore::open_existing(&target).is_err());
            assert!(!target.exists());
            assert!(std::fs::read_dir(temp.path()).unwrap().any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".emilybase-registry-restore-")
            }));
            restore_registry_backup(&archive, &target).unwrap();
            usable(&target, &credentials, &expected);
        }
        assert_eq!(image(&source), expected);
        assert_eq!(read(&archive).unwrap(), expected);
    }
}

#[test]
fn sync_failures_before_and_after_sync_distinguish_unchanged_from_uncertain_publication() {
    let _serial = PROCESS_TESTS.blocking_lock();
    for boundary in [
        "registry_backup_file_sync",
        "registry_backup_file_sync_after",
        "registry_backup_parent_sync",
        "registry_backup_parent_sync_after",
        "registry_restore_wal_sync",
        "registry_restore_wal_sync_after",
        "registry_restore_data_sync",
        "registry_restore_data_sync_after",
        "registry_restore_project_sync",
        "registry_restore_project_sync_after",
        "registry_restore_stage_sync",
        "registry_restore_stage_sync_after",
        "registry_restore_parent_sync",
        "registry_restore_parent_sync_after",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let credentials = seed(&source);
        let expected = image(&source);
        let archive = temp.path().join("projects.backup");
        let target = temp.path().join("restored");
        let backup = boundary.starts_with("registry_backup");
        if !backup {
            ProjectStore::open_existing(&source)
                .unwrap()
                .backup(&archive)
                .unwrap();
        }
        inject(boundary);
        let outcome = if backup {
            ProjectStore::open_existing(&source)
                .unwrap()
                .backup(&archive)
        } else {
            restore_registry_backup(&archive, &target)
        };
        let selected = boundary.contains("parent_sync");
        if selected {
            assert!(matches!(outcome, Err(Error::PublicationUnknown(_))));
        } else {
            assert!(matches!(outcome, Err(Error::Io(_))));
        }
        let output = if backup { &archive } else { &target };
        assert_eq!(output.exists(), selected);
        assert_eq!(image(&source), expected);
        assert!(!std::fs::read_dir(temp.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".emilybase-registry-")
        }));
        if selected {
            if backup {
                assert_eq!(read(&archive).unwrap(), expected);
                restore_registry_backup(&archive, &target).unwrap();
            }
            usable(&target, &credentials, &expected);
        } else if backup {
            ProjectStore::open_existing(&source)
                .unwrap()
                .backup(&archive)
                .unwrap();
        } else {
            restore_registry_backup(&archive, &target).unwrap();
            usable(&target, &credentials, &expected);
        }
    }
}

#[test]
fn two_synchronized_process_restorers_publish_exactly_one_complete_destination() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let credentials = seed(&source);
    let expected = image(&source);
    let archive = temp.path().join("projects.backup");
    ProjectStore::open_existing(&source)
        .unwrap()
        .backup(&archive)
        .unwrap();
    let target = temp.path().join("restored");
    let mut a = Worker::start(&source, &archive, &target, "race", "");
    let mut b = Worker::start(&source, &archive, &target, "race", "");
    a.reach("race_ready");
    b.reach("race_ready");
    a.release();
    b.release();
    let outputs = [a.finish(), b.finish()];
    assert_eq!(
        outputs
            .iter()
            .filter(|out| out.contains("RESTORE_OK"))
            .count(),
        1
    );
    assert_eq!(
        outputs
            .iter()
            .filter(|out| out.contains("RESTORE_CONFLICT"))
            .count(),
        1
    );
    usable(&target, &credentials, &expected);
    assert_eq!(image(&source), expected);
}
