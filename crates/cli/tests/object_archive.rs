use emilybase_object_storage::{
    MAX_ARCHIVE_BYTES, ObjectId, ProjectDirectory, ProjectId, encode_archive,
};
use serde_json::Value;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
const PROJECT: &str = "01010101010101010101010101010101";
fn command(path: &Path, project: &str) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    c.arg("object-archive-verify").arg(path).arg(project);
    c
}
fn run(c: Command) -> Output {
    run_with_timeout(c, Duration::from_secs(5))
}
fn run_with_timeout(mut c: Command, timeout: Duration) -> Output {
    let mut child = c
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + timeout;
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("archive inspector deadline");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.wait_with_output().unwrap()
}
fn fixture(parent: &Path) -> (std::path::PathBuf, Vec<u8>) {
    let source = parent.join("objects");
    fs::DirBuilder::new().mode(0o700).create(&source).unwrap();
    let mut owner =
        ProjectDirectory::initialize(&source, PROJECT.parse::<ProjectId>().unwrap()).unwrap();
    owner
        .put(
            ObjectId::from_bytes([2; 16]),
            b"synthetic-private-archive\0\xff",
        )
        .unwrap();
    let bytes = encode_archive(&owner.capture().unwrap()).unwrap();
    let path = parent.join("synthetic.object-archive");
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
    (path, bytes)
}
fn refused(out: Output) {
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(!error.contains("synthetic-private-archive"));
    assert!(!error.contains("panicked"));
}

#[test]
fn actual_cli_verifies_complete_archive_and_prints_only_checked_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let (path, bytes) = fixture(temp.path());
    let out = run(command(&path, PROJECT));
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("synthetic-private-archive"));
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report.as_object().unwrap().len(), 5);
    assert_eq!(report["format"], 1);
    assert_eq!(report["project"], PROJECT);
    assert_eq!(report["objects"], 1);
    assert_eq!(report["bytes"], b"synthetic-private-archive\0\xff".len());
    assert_eq!(report["digest"].as_str().unwrap().len(), 64);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    refused(run(command(&path, "09090909090909090909090909090909")));
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn actual_cli_checks_readonly_maximum_archive_and_refuses_final_byte_corruption() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("objects");
    fs::DirBuilder::new().mode(0o700).create(&source).unwrap();
    let mut owner = ProjectDirectory::initialize(&source, PROJECT.parse().unwrap()).unwrap();
    let payload = vec![0xa5; 8 * 1024 * 1024];
    for key in 0..128 {
        owner
            .put(
                ObjectId::from_bytes([key; 16]),
                if key < 8 { &payload } else { &[] },
            )
            .unwrap();
    }
    let bytes = encode_archive(&owner.capture().unwrap()).unwrap();
    assert_eq!(bytes.len(), MAX_ARCHIVE_BYTES);
    drop(owner);
    let path = temp.path().join("maximum.object-archive");
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
    drop(file);
    drop(bytes);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    let before = fs::metadata(&path).unwrap();
    let output = run_with_timeout(command(&path, PROJECT), Duration::from_secs(20));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(output.stdout.len() < 512);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["objects"], 128);
    assert_eq!(report["bytes"], 64 * 1024 * 1024);
    let after = fs::metadata(&path).unwrap();
    assert_eq!(
        (
            before.ino(),
            before.len(),
            before.mtime(),
            before.mtime_nsec()
        ),
        (after.ino(), after.len(), after.mtime(), after.mtime_nsec())
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let mut file = File::options().read(true).write(true).open(&path).unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    let mut last = [0; 1];
    file.read_exact(&mut last).unwrap();
    last[0] ^= 1;
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&last).unwrap();
    file.sync_all().unwrap();
    drop(file);
    refused(run_with_timeout(
        command(&path, PROJECT),
        Duration::from_secs(20),
    ));
    assert_eq!(fs::metadata(&path).unwrap().len(), MAX_ARCHIVE_BYTES as u64);
}

#[test]
fn actual_cli_refuses_wrong_scope_aliases_fifo_corruption_and_oversize_without_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let (path, original) = fixture(temp.path());
    refused(run(command(&path, "../foreign")));
    let alias = temp.path().join("alias");
    symlink(&path, &alias).unwrap();
    refused(run(command(&alias, PROJECT)));
    fs::remove_file(&alias).unwrap();
    fs::hard_link(&path, &alias).unwrap();
    refused(run(command(&path, PROJECT)));
    fs::remove_file(&alias).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    refused(run(command(&path, PROJECT)));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let fifo = temp.path().join("fifo");
    assert!(
        Command::new("mkfifo")
            .arg("-m")
            .arg("600")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    refused(run(command(&fifo, PROJECT)));
    refused(run(command(&fifo, "../foreign")));
    let mut bytes = original;
    bytes[128] ^= 1;
    fs::write(&path, &bytes).unwrap();
    refused(run(command(&path, PROJECT)));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let file = File::options().write(true).open(&path).unwrap();
    file.set_len((MAX_ARCHIVE_BYTES + 1) as u64).unwrap();
    refused(run(command(&path, PROJECT)));
    file.set_len(0).unwrap();
    refused(run(command(&path, PROJECT)));
    assert_eq!(fs::metadata(path).unwrap().len(), 0);
}

#[test]
fn readonly_archive_stdout_failure_preserves_original_image_and_subsequent_inspection() {
    let temp = tempfile::tempdir().unwrap();
    let (path, bytes) = fixture(temp.path());
    let full = File::options().write(true).open("/dev/full").unwrap();
    let result = command(&path, PROJECT)
        .stdout(Stdio::from(full))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("synthetic-private-archive"));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(run(command(&path, PROJECT)).status.success());
}
