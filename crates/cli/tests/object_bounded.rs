use emilybase_object_storage::{ObjectId, ProjectDirectory, ProjectId, WriteLimits};
use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const PROJECT: &str = "01010101010101010101010101010101";
const OBJECT: &str = "02020202020202020202020202020202";
fn command(path: &Path, object: &str, objects: &str, bytes: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    command
        .arg("object-directory")
        .arg(path)
        .arg(PROJECT)
        .arg("put-bounded")
        .arg(object)
        .arg("--max-objects")
        .arg(objects)
        .arg("--max-bytes")
        .arg(bytes);
    command
}
fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    drop(ProjectDirectory::initialize(&path, PROJECT.parse().unwrap()).unwrap());
    (temp, path)
}
fn run(mut command: Command, payload: &[u8]) -> Output {
    let mut input = tempfile::tempfile().unwrap();
    input.write_all(payload).unwrap();
    use std::io::{Seek, SeekFrom};
    input.seek(SeekFrom::Start(0)).unwrap();
    command.stdin(Stdio::from(input)).output().unwrap()
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
fn actual_cli_admits_exact_capacity_and_reports_complete_metadata_without_payload_output() {
    let (_temp, path) = fixture();
    let payload = b"synthetic-private";
    let report = success(run(command(&path, OBJECT, "1", "17"), payload));
    assert_eq!(report.as_object().unwrap().len(), 8);
    assert_eq!(report["format"], 1);
    assert_eq!(report["project"], PROJECT);
    assert_eq!(report["object"], OBJECT);
    assert_eq!(report["bytes"], 17);
    assert_eq!(report["objects"], 1);
    assert_eq!(report["total_bytes"], 17);
    let before = fs::read(path.join(format!("{OBJECT}.object"))).unwrap();
    refused(run(command(&path, OBJECT, "1", "17"), b"replacement"));
    refused(run(
        command(&path, "04040404040404040404040404040404", "1", "17"),
        &[],
    ));
    assert_eq!(
        fs::read(path.join(format!("{OBJECT}.object"))).unwrap(),
        before
    );
    assert_eq!(fs::read_dir(path).unwrap().count(), 2);
}

#[test]
fn actual_cli_stops_input_at_configured_byte_limit_and_zero_bytes_still_charge_one_name() {
    let (_temp, path) = fixture();
    refused(run(command(&path, OBJECT, "2", "3"), b"more"));
    assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
    let mut child = command(&path, OBJECT, "2", "3")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut unfinished = child.stdin.take().unwrap();
    unfinished.write_all(b"more").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("oversized input waited for EOF");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    refused(child.wait_with_output().unwrap());
    drop(unfinished);
    assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
    let empty = success(run(command(&path, OBJECT, "1", "0"), &[]));
    assert_eq!(empty["bytes"], 0);
    assert_eq!(empty["objects"], 1);
    assert_eq!(empty["total_bytes"], 0);
    refused(run(
        command(&path, "04040404040404040404040404040404", "1", "0"),
        &[],
    ));
    assert_eq!(fs::read_dir(&path).unwrap().count(), 2);
}

#[test]
fn invalid_limits_and_ids_refuse_before_waiting_for_unfinished_input() {
    let (_temp, path) = fixture();
    for (object, objects, bytes) in [
        (OBJECT, "129", "0"),
        (OBJECT, "1", "67108865"),
        (OBJECT, "0", "0"),
        ("../foreign", "1", "0"),
    ] {
        let mut child = command(&path, object, objects, bytes)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("limit parsing waited for stdin");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        refused(child.wait_with_output().unwrap());
        assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
    }
}

#[test]
fn stdout_failure_preserves_bounded_write_and_reopened_capacity_refuses_an_extra_name() {
    let (_temp, path) = fixture();
    let input = tempfile::tempfile().unwrap();
    let output = command(&path, OBJECT, "1", "0")
        .stdin(Stdio::from(input))
        .stdout(Stdio::from(
            File::options().write(true).open("/dev/full").unwrap(),
        ))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    refused(output);
    let mut owner = ProjectDirectory::open(&path, PROJECT.parse::<ProjectId>().unwrap()).unwrap();
    assert!(
        owner
            .get(OBJECT.parse::<ObjectId>().unwrap())
            .unwrap()
            .payload()
            .is_empty()
    );
    assert!(
        owner
            .put_bounded(
                ObjectId::from_bytes([4; 16]),
                &[],
                WriteLimits::new(1, 0).unwrap()
            )
            .is_err()
    );
    assert_eq!(owner.inventory().unwrap().entries().len(), 1);
}
