use emilybase_object_storage::{ObjectId, ProjectDirectory, ProjectId};
use serde_json::Value;
use std::fs::{self, File};
use std::os::unix::fs::{DirBuilderExt, symlink};
use std::path::Path;
use std::process::{Command, Output, Stdio};

const PROJECT: &str = "01010101010101010101010101010101";
fn command(source: &Path, destination: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    command
        .args(["object-directory"])
        .arg(source)
        .arg(PROJECT)
        .arg("backup")
        .arg(destination);
    command
}
fn success(out: Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("synthetic-private"));
    serde_json::from_slice(&out.stdout).unwrap()
}
fn refused(out: Output) {
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("synthetic-private"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
}
fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::DirBuilder::new().mode(0o700).create(&source).unwrap();
    let mut owner = ProjectDirectory::initialize(&source, PROJECT.parse().unwrap()).unwrap();
    owner
        .put(ObjectId::from_bytes([2; 16]), b"synthetic-private\0\xff")
        .unwrap();
    drop(owner);
    (temp, source)
}

#[test]
fn actual_cli_creates_complete_binary_archive_and_independent_verifier_reports_exact_metadata() {
    let (temp, source) = fixture();
    let path = temp.path().join("copy.object-archive");
    let report = success(command(&source, &path).output().unwrap());
    assert_eq!(report.as_object().unwrap().len(), 5);
    assert_eq!(report["format"], 1);
    assert_eq!(report["project"], PROJECT);
    assert_eq!(report["objects"], 1);
    assert_eq!(report["bytes"], 19);
    assert_eq!(report["digest"].as_str().unwrap().len(), 64);
    let verified = success(
        Command::new(env!("CARGO_BIN_EXE_emilybase"))
            .arg("object-archive-verify")
            .arg(&path)
            .arg(PROJECT)
            .output()
            .unwrap(),
    );
    assert_eq!(verified, report);
    let bytes = fs::read(&path).unwrap();
    let view = emilybase_object_storage::verify_archive(&bytes, PROJECT.parse().unwrap()).unwrap();
    assert_eq!(view.objects()[0].payload(), b"synthetic-private\0\xff");
    refused(command(&source, &path).output().unwrap());
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn actual_cli_publishes_maximum_archive_and_retains_source_for_independent_restore() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::DirBuilder::new().mode(0o700).create(&source).unwrap();
    let project = PROJECT.parse().unwrap();
    let mut owner = ProjectDirectory::initialize(&source, project).unwrap();
    let payload = vec![0xa7; 8 * 1024 * 1024];
    for key in 0..128 {
        owner
            .put(
                ObjectId::from_bytes([key; 16]),
                if key < 8 { &payload } else { &[] },
            )
            .unwrap();
    }
    let inventory = owner.inventory().unwrap();
    drop(owner);
    let archive = temp.path().join("copy.object-archive");
    let mut child = command(&source, &archive)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("maximum backup deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let report = success(child.wait_with_output().unwrap());
    assert_eq!(report["objects"], 128);
    assert_eq!(report["bytes"], 64 * 1024 * 1024);
    assert_eq!(
        fs::metadata(&archive).unwrap().len(),
        emilybase_object_storage::MAX_ARCHIVE_BYTES as u64
    );
    let verified = emilybase_object_storage::inspect_archive_file(&archive, project).unwrap();
    assert_eq!(verified.digest, *inventory.digest());
    let restored = temp.path().join("restored");
    emilybase_object_storage::restore_archive_file(&archive, project, &restored).unwrap();
    let reopened = ProjectDirectory::open(&source, project).unwrap();
    let copy = ProjectDirectory::open(&restored, project).unwrap();
    assert_eq!(reopened.inventory().unwrap(), inventory);
    assert_eq!(copy.inventory().unwrap(), inventory);
}

#[test]
fn actual_cli_handles_relative_paths_and_refuses_source_alias_and_existing_destination() {
    let (temp, source) = fixture();
    let report = success(
        command(Path::new("source"), Path::new("copy"))
            .current_dir(temp.path())
            .output()
            .unwrap(),
    );
    assert_eq!(report["objects"], 1);
    let alias = temp.path().join("alias");
    symlink(&source, &alias).unwrap();
    refused(command(&source, &source.join("copy")).output().unwrap());
    refused(command(&source, &alias.join("copy")).output().unwrap());
    refused(
        command(&source, &alias.join("../source/copy"))
            .output()
            .unwrap(),
    );
    let existing = temp.path().join("existing");
    fs::write(&existing, b"synthetic-private-existing").unwrap();
    refused(command(&source, &existing).output().unwrap());
    assert_eq!(fs::read(existing).unwrap(), b"synthetic-private-existing");
    assert_eq!(fs::read_dir(&source).unwrap().count(), 2);
}

#[test]
fn actual_cli_refuses_unknown_corrupt_or_foreign_source_without_partial_archive_or_stdout() {
    for mutation in 0..3 {
        let (temp, source) = fixture();
        match mutation {
            0 => fs::write(source.join("unknown"), b"synthetic-private").unwrap(),
            1 => fs::write(
                source.join("02020202020202020202020202020202.object"),
                b"damaged",
            )
            .unwrap(),
            _ => fs::write(
                source.join(".emilybase-objects"),
                emilybase_object_storage::encode(
                    ProjectId::from_bytes([9; 16]),
                    ObjectId::from_bytes([0; 16]),
                    &[],
                )
                .unwrap(),
            )
            .unwrap(),
        }
        let path = temp.path().join("copy");
        refused(command(&source, &path).output().unwrap());
        assert!(!path.exists());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }
}

#[test]
fn actual_cli_stdout_failure_preserves_published_archive_and_explicit_verification_recovers_metadata()
 {
    let (temp, source) = fixture();
    let path = temp.path().join("copy");
    let result = command(&source, &path)
        .stdout(Stdio::from(
            File::options().write(true).open("/dev/full").unwrap(),
        ))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    refused(result);
    let report =
        emilybase_object_storage::inspect_archive_file(&path, PROJECT.parse().unwrap()).unwrap();
    assert_eq!(report.objects, 1);
    assert_eq!(report.payload_bytes, 19);
    let before = fs::read(&path).unwrap();
    refused(command(&source, &path).output().unwrap());
    assert_eq!(fs::read(&path).unwrap(), before);
    let metadata = success(
        Command::new(env!("CARGO_BIN_EXE_emilybase"))
            .arg("object-archive-verify")
            .arg(&path)
            .arg(PROJECT)
            .output()
            .unwrap(),
    );
    assert_eq!(metadata["bytes"], 19);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
}
