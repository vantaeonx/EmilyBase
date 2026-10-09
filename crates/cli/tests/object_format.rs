use emilybase_object_storage::{ObjectId, ProjectId, encode};
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
const PROJECT: &str = "01010101010101010101010101010101";
const OBJECT: &str = "02020202020202020202020202020202";
const PAYLOAD: &[u8] = b"synthetic-private-object-payload";
fn command(path: &Path, project: &str, object: &str) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    c.arg("object-verify").arg(path).arg(project).arg(object);
    c
}
fn run(mut command: Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("object inspector deadline");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.wait_with_output().unwrap()
}
fn fixture(path: &Path) -> Vec<u8> {
    let bytes = encode(
        PROJECT.parse::<ProjectId>().unwrap(),
        OBJECT.parse::<ObjectId>().unwrap(),
        PAYLOAD,
    )
    .unwrap();
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
    bytes
}
fn refused(out: &Output) {
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(!text.contains("synthetic-private-object-payload"));
    assert!(!text.contains("panicked"));
}

#[test]
fn actual_cli_reports_only_verified_metadata_and_preserves_complete_source_bytes() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("synthetic.object");
    let original = fixture(&path);
    let out = run(command(&path, PROJECT, OBJECT));
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report.as_object().unwrap().len(), 3);
    assert_eq!(report["format"], 1);
    assert_eq!(report["bytes"], PAYLOAD.len());
    assert_eq!(report["sha256"].as_str().unwrap().len(), 64);
    assert!(!String::from_utf8_lossy(&out.stdout).contains("synthetic-private"));
    assert_eq!(fs::read(&path).unwrap(), original);
    let before = fs::read(&path).unwrap();
    refused(&run(command(
        &path,
        "03030303030303030303030303030303",
        OBJECT,
    )));
    refused(&run(command(
        &path,
        PROJECT,
        "03030303030303030303030303030303",
    )));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn actual_cli_refuses_identity_overrides_aliases_fifo_corruption_and_size_before_unbounded_io() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("synthetic.object");
    let original = fixture(&path);
    for (project, object) in [
        ("../private", OBJECT),
        (PROJECT, "../private"),
        ("", OBJECT),
        (PROJECT, "00"),
    ] {
        refused(&run(command(&path, project, object)));
    }
    let alias = d.path().join("alias");
    symlink(&path, &alias).unwrap();
    refused(&run(command(&alias, PROJECT, OBJECT)));
    fs::remove_file(&alias).unwrap();
    fs::hard_link(&path, &alias).unwrap();
    refused(&run(command(&path, PROJECT, OBJECT)));
    fs::remove_file(&alias).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    refused(&run(command(&path, PROJECT, OBJECT)));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let fifo = d.path().join("fifo");
    create_fifo(&fifo);
    refused(&run(command(&fifo, PROJECT, OBJECT)));
    refused(&run(command(&fifo, "../private", OBJECT)));
    let mut corrupt = original.clone();
    corrupt[96] ^= 1;
    fs::write(&path, corrupt).unwrap();
    refused(&run(command(&path, PROJECT, OBJECT)));
    let file = File::options().write(true).open(&path).unwrap();
    file.set_len((8 * 1024 * 1024 + 97) as u64).unwrap();
    refused(&run(command(&path, PROJECT, OBJECT)));
    file.set_len(0).unwrap();
    refused(&run(command(&path, PROJECT, OBJECT)));
    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
}
fn create_fifo(path: &Path) {
    // Use the existing system utility in this actual-binary fixture only.
    assert!(
        Command::new("mkfifo")
            .arg("-m")
            .arg("600")
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn stdout_failure_after_readonly_inspection_is_failure_without_mutating_the_source() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("synthetic.object");
    let bytes = fixture(&path);
    let full = File::options().write(true).open("/dev/full").unwrap();
    let out = command(&path, PROJECT, OBJECT)
        .stdout(Stdio::from(full))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let success = run(command(&path, PROJECT, OBJECT));
    assert!(success.status.success());
    let report: Value = serde_json::from_slice(&success.stdout).unwrap();
    assert_eq!(report["bytes"], json!(PAYLOAD.len()));
}
