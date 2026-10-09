use emilybase_object_storage::{ObjectId, ProjectDirectory, ProjectId};
use serde_json::Value;
use std::fs::{self, File};
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const PROJECT: &str = "01010101010101010101010101010101";
fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::DirBuilder::new().mode(0o700).create(&source).unwrap();
    let mut owner = ProjectDirectory::initialize(source, PROJECT.parse().unwrap()).unwrap();
    owner
        .put(ObjectId::from_bytes([2; 16]), b"synthetic-private\0\xff")
        .unwrap();
    let archive = temp.path().join("copy.object-archive");
    owner.backup_to(&archive).unwrap();
    (temp, archive)
}
fn command(archive: &Path, project: &str, target: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    command
        .arg("object-archive-restore")
        .arg(archive)
        .arg(project)
        .arg(target);
    command
}
fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("synthetic-private"));
    serde_json::from_slice(&output.stdout).unwrap()
}
fn refused(output: Output) {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-private"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
}

#[test]
fn actual_cli_restores_relative_complete_directory_and_reports_exact_verified_metadata() {
    let (temp, archive) = fixture();
    let target = temp.path().join("restored");
    let bytes = fs::read(&archive).unwrap();
    let report = success(
        command(
            Path::new("copy.object-archive"),
            PROJECT,
            Path::new("restored"),
        )
        .current_dir(temp.path())
        .output()
        .unwrap(),
    );
    let verified = success(
        Command::new(env!("CARGO_BIN_EXE_emilybase"))
            .arg("object-archive-verify")
            .arg(&archive)
            .arg(PROJECT)
            .output()
            .unwrap(),
    );
    assert_eq!(report, verified);
    let owner = ProjectDirectory::open(&target, PROJECT.parse().unwrap()).unwrap();
    assert_eq!(
        emilybase_object_storage::encode_archive(&owner.capture().unwrap()).unwrap(),
        bytes
    );
    drop(owner);
    refused(command(&archive, PROJECT, &target).output().unwrap());
    assert_eq!(fs::read(&archive).unwrap(), bytes);
}

#[test]
fn actual_cli_refuses_corrupt_foreign_or_malformed_identity_before_any_stage_or_output() {
    for mutation in 0..3 {
        let (temp, archive) = fixture();
        let target = temp.path().join("restored");
        let project = match mutation {
            0 => {
                fs::write(&archive, b"damaged").unwrap();
                PROJECT
            }
            1 => "03030303030303030303030303030303",
            _ => "../foreign",
        };
        refused(command(&archive, project, &target).output().unwrap());
        assert!(!target.exists());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
    }
}

#[test]
fn actual_cli_stdout_failure_preserves_selected_directory_and_retry_never_overwrites_it() {
    let (temp, archive) = fixture();
    let target = temp.path().join("restored");
    let before = fs::read(&archive).unwrap();
    let output = command(&archive, PROJECT, &target)
        .stdout(Stdio::from(
            File::options().write(true).open("/dev/full").unwrap(),
        ))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    refused(output);
    let owner = ProjectDirectory::open(&target, PROJECT.parse::<ProjectId>().unwrap()).unwrap();
    assert_eq!(
        emilybase_object_storage::encode_archive(&owner.capture().unwrap()).unwrap(),
        before
    );
    drop(owner);
    refused(command(&archive, PROJECT, &target).output().unwrap());
    assert_eq!(fs::read(&archive).unwrap(), before);
    let inspected = success(
        Command::new(env!("CARGO_BIN_EXE_emilybase"))
            .arg("object-directory")
            .arg(&target)
            .arg(PROJECT)
            .arg("list")
            .output()
            .unwrap(),
    );
    assert_eq!(inspected["objects"].as_array().unwrap().len(), 1);
    assert_eq!(inspected["bytes"], 19);
}
