use emilybase_files::{FileId, FileStore};
use emilybase_object_storage::{ObjectId, ProjectDirectory};
use emilybase_transactions::Database;
use serde_json::Value;
use std::fs::{self, File};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const PROJECT: &str = "01010101010101010101010101010101";
fn command(path: &Path, project: &str, objects: &str, bytes: &str) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    c.arg("file-root-init")
        .arg(path)
        .arg(project)
        .args(["--max-objects", objects, "--max-bytes", bytes])
        .stdin(Stdio::null());
    c
}
fn refused(output: Output) {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
}
fn open(path: &Path) -> FileStore {
    FileStore::open(
        Database::open(path.join("metadata")).unwrap(),
        ProjectDirectory::open(path.join("objects"), PROJECT.parse().unwrap()).unwrap(),
    )
    .unwrap()
}

#[test]
fn actual_cli_creates_private_root_and_reports_durable_initial_identity_and_quota() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("new");
    let output = command(Path::new("new"), PROJECT, "8", "32768")
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["format"], 1);
    assert_eq!(report["project"], PROJECT);
    assert_eq!(report["metadata"]["last_transaction"], "2");
    assert_eq!(report["metadata"]["wal_version"], 1);
    assert_eq!(report["references"], 0);
    assert_eq!(report["objects"]["count"], 0);
    assert_eq!(report["objects"]["bytes"], 0);
    assert_eq!(report["quota"]["max_objects"], 8);
    assert_eq!(report["quota"]["max_bytes"], 32768);
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o700);
    assert_eq!(fs::read_dir(&path).unwrap().count(), 2);
    let mut store = open(&path);
    let snapshot = store.capture().unwrap();
    let identity = snapshot
        .metadata_report()
        .database_id
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(report["metadata"]["database_id"], identity);
    store
        .publish(
            FileId::from_bytes([2; 16]),
            ObjectId::from_bytes([3; 16]),
            [4; 16],
            "synthetic-private",
            b"payload",
        )
        .unwrap();
    drop(store);
    let later = fs::read(path.join("metadata/redo.wal")).unwrap();
    refused(command(&path, PROJECT, "8", "32768").output().unwrap());
    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), later);
    assert_eq!(open(&path).list().unwrap().len(), 1);
}

#[test]
fn actual_cli_validates_project_quota_and_numbers_before_creating_any_entry() {
    for (project, objects, bytes) in [
        ("../foreign", "8", "32768"),
        (PROJECT, "129", "1"),
        (PROJECT, "8", "67108865"),
        (PROJECT, "invalid", "1"),
        (PROJECT, "1", "invalid"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("new");
        refused(command(&target, project, objects, bytes).output().unwrap());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }
}

#[test]
fn actual_cli_failed_report_keeps_complete_zero_quota_root_and_refuses_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("new");
    let output = command(&target, PROJECT, "0", "0")
        .stdout(Stdio::from(
            File::options().write(true).open("/dev/full").unwrap(),
        ))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    refused(output);
    let mut store = open(&target);
    assert_eq!(store.quota().unwrap().objects(), 0);
    assert!(store.list().unwrap().is_empty());
    assert!(
        store
            .publish(
                FileId::from_bytes([2; 16]),
                ObjectId::from_bytes([3; 16]),
                [4; 16],
                "synthetic",
                &[]
            )
            .is_err()
    );
    drop(store);
    let wal = fs::read(target.join("metadata/redo.wal")).unwrap();
    refused(command(&target, PROJECT, "8", "32768").output().unwrap());
    assert_eq!(fs::read(target.join("metadata/redo.wal")).unwrap(), wal);
}
