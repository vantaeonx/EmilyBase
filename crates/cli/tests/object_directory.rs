use emilybase_object_storage::{ObjectId, ProjectDirectory, ProjectId};
use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const PROJECT: &str = "01010101010101010101010101010101";
const FOREIGN: &str = "03030303030303030303030303030303";
const OBJECT: &str = "02020202020202020202020202020202";
fn command(path: &Path, project: &str, operation: &str, object: Option<&str>) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    c.arg("object-directory")
        .arg(path)
        .arg(project)
        .arg(operation);
    if let Some(object) = object {
        c.arg(object);
    }
    c
}
fn run(mut command: Command, input: Vec<u8>) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("object directory CLI deadline");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    writer.join().unwrap();
    child.wait_with_output().unwrap()
}
fn success(out: Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    serde_json::from_slice(&out.stdout).unwrap()
}
fn refused(out: Output) {
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("synthetic-private"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
}
fn directory(path: &Path) {
    fs::DirBuilder::new().mode(0o700).create(path).unwrap();
}

#[test]
fn actual_cli_initializes_imports_exact_binary_and_inspects_without_payload_output() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    directory(&path);
    let initialized = success(run(command(&path, PROJECT, "init", None), Vec::new()));
    assert_eq!(initialized["project"], PROJECT);
    assert_eq!(initialized["format"], 1);
    let payload = b"synthetic-private\0\xff\xfe\n".to_vec();
    let put = success(run(
        command(&path, PROJECT, "put", Some(OBJECT)),
        payload.clone(),
    ));
    assert_eq!(put.as_object().unwrap().len(), 3);
    assert_eq!(put["bytes"], payload.len());
    assert_eq!(put["sha256"].as_str().unwrap().len(), 64);
    let before = fs::read(path.join(format!("{OBJECT}.object"))).unwrap();
    let inspected = success(run(
        command(&path, PROJECT, "inspect", Some(OBJECT)),
        Vec::new(),
    ));
    assert_eq!(put, inspected);
    assert_eq!(
        fs::read(path.join(format!("{OBJECT}.object"))).unwrap(),
        before
    );
    let owner = ProjectDirectory::open(&path, PROJECT.parse::<ProjectId>().unwrap()).unwrap();
    assert_eq!(
        owner
            .get(OBJECT.parse::<ObjectId>().unwrap())
            .unwrap()
            .payload(),
        payload
    );
}

#[test]
fn actual_cli_refuses_foreign_scope_overwrite_missing_initialization_and_oversized_stdin() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    directory(&path);
    refused(run(
        command(&path, PROJECT, "put", Some(OBJECT)),
        b"synthetic-private".to_vec(),
    ));
    assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
    success(run(command(&path, PROJECT, "init", None), Vec::new()));
    refused(run(
        command(&path, FOREIGN, "inspect", Some(OBJECT)),
        Vec::new(),
    ));
    refused(run(
        command(&path, FOREIGN, "put", Some(OBJECT)),
        b"synthetic-private".to_vec(),
    ));
    refused(run(
        command(&path, PROJECT, "put", Some(OBJECT)),
        vec![0x59; 8 * 1024 * 1024 + 1],
    ));
    assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
    success(run(
        command(&path, PROJECT, "put", Some(OBJECT)),
        Vec::new(),
    ));
    let before = fs::read(path.join(format!("{OBJECT}.object"))).unwrap();
    refused(run(
        command(&path, PROJECT, "put", Some(OBJECT)),
        b"replacement".to_vec(),
    ));
    refused(run(command(&path, PROJECT, "init", None), Vec::new()));
    assert_eq!(
        fs::read(path.join(format!("{OBJECT}.object"))).unwrap(),
        before
    );
    assert_eq!(fs::read_dir(&path).unwrap().count(), 2);
}

#[test]
fn actual_cli_stdout_failure_preserves_initialized_scope_and_complete_published_object() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    directory(&path);
    let full = || Stdio::from(File::options().write(true).open("/dev/full").unwrap());
    let result = command(&path, PROJECT, "init", None)
        .stdout(full())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!result.status.success());
    let project = PROJECT.parse::<ProjectId>().unwrap();
    let object = OBJECT.parse::<ObjectId>().unwrap();
    drop(ProjectDirectory::open(&path, project).unwrap());
    let input = temp.path().join("input");
    fs::write(&input, b"synthetic-private-after-output-failure").unwrap();
    let result = command(&path, PROJECT, "put", Some(OBJECT))
        .stdin(Stdio::from(File::open(input).unwrap()))
        .stdout(full())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!result.status.success());
    let owner = ProjectDirectory::open(&path, project).unwrap();
    assert_eq!(
        owner.get(object).unwrap().payload(),
        b"synthetic-private-after-output-failure"
    );
    drop(owner);
    let report = success(run(
        command(&path, PROJECT, "inspect", Some(OBJECT)),
        Vec::new(),
    ));
    assert_eq!(report["bytes"], 38);
    refused(run(
        command(&path, PROJECT, "put", Some(OBJECT)),
        b"retry".to_vec(),
    ));
}

#[test]
fn malformed_typed_ids_refuse_before_waiting_for_an_unfinished_input_stream() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    directory(&path);
    for (project, object) in [
        ("../foreign", OBJECT),
        (PROJECT, "../foreign"),
        (PROJECT, "00"),
    ] {
        let mut child = command(&path, project, "put", Some(object))
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
                panic!("identity parsing waited for stdin");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        refused(child.wait_with_output().unwrap());
        assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
    }
}

#[test]
fn actual_cli_list_reports_complete_canonical_metadata_without_any_payload_output() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    directory(&path);
    let project = PROJECT.parse::<ProjectId>().unwrap();
    let mut owner = ProjectDirectory::initialize(&path, project).unwrap();
    for key in (0..128).rev() {
        owner
            .put(ObjectId::from_bytes([key; 16]), b"synthetic-private")
            .unwrap();
    }
    let expected = owner.inventory().unwrap();
    drop(owner);
    let out = run(command(&path, PROJECT, "list", None), Vec::new());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("synthetic-private"));
    let report = success(out);
    assert_eq!(report.as_object().unwrap().len(), 5);
    assert_eq!(report["project"], PROJECT);
    assert_eq!(report["format"], 1);
    assert_eq!(report["bytes"], 128 * 17);
    assert_eq!(report["objects"].as_array().unwrap().len(), 128);
    for (index, entry) in report["objects"].as_array().unwrap().iter().enumerate() {
        assert_eq!(entry.as_object().unwrap().len(), 3);
        assert_eq!(
            entry["object"],
            ObjectId::from_bytes([index as u8; 16]).to_string()
        );
        assert_eq!(entry["bytes"], 17);
        assert_eq!(entry["sha256"].as_str().unwrap().len(), 64);
    }
    assert_eq!(
        report["digest"],
        expected
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    assert_eq!(
        success(run(command(&path, PROJECT, "list", None), Vec::new())),
        report
    );
    assert_eq!(fs::read_dir(&path).unwrap().count(), 129);
}

#[test]
fn actual_cli_list_refuses_unknown_corrupt_foreign_and_count_overflow_without_partial_output() {
    for mutation in 0..4 {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("objects");
        directory(&path);
        let project = PROJECT.parse::<ProjectId>().unwrap();
        let object = OBJECT.parse::<ObjectId>().unwrap();
        let mut owner = ProjectDirectory::initialize(&path, project).unwrap();
        owner.put(object, b"synthetic-private").unwrap();
        match mutation {
            0 => fs::write(path.join("unmanaged"), b"synthetic-private-unmanaged").unwrap(),
            1 => fs::write(path.join(format!("{OBJECT}.object")), b"damaged").unwrap(),
            2 => fs::write(
                path.join(format!("{OBJECT}.object")),
                emilybase_object_storage::encode(
                    FOREIGN.parse().unwrap(),
                    object,
                    b"synthetic-private-foreign",
                )
                .unwrap(),
            )
            .unwrap(),
            _ => {
                for key in 0..130u8 {
                    let id = ObjectId::from_bytes([key; 16]);
                    if id != object {
                        owner.put(id, &[]).unwrap();
                    }
                }
            }
        }
        let before = fs::read(path.join(format!("{OBJECT}.object"))).unwrap();
        let count = fs::read_dir(&path).unwrap().count();
        drop(owner);
        refused(run(command(&path, PROJECT, "list", None), Vec::new()));
        assert_eq!(
            fs::read(path.join(format!("{OBJECT}.object"))).unwrap(),
            before
        );
        assert_eq!(fs::read_dir(&path).unwrap().count(), count);
    }
}

#[test]
fn actual_cli_list_stdout_failure_leaves_complete_source_objects_and_marker_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    directory(&path);
    success(run(command(&path, PROJECT, "init", None), Vec::new()));
    success(run(
        command(&path, PROJECT, "put", Some(OBJECT)),
        b"synthetic-private".to_vec(),
    ));
    let marker = fs::read(path.join(".emilybase-objects")).unwrap();
    let image = fs::read(path.join(format!("{OBJECT}.object"))).unwrap();
    let full = File::options().write(true).open("/dev/full").unwrap();
    let result = command(&path, PROJECT, "list", None)
        .stdout(Stdio::from(full))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("synthetic-private"));
    assert_eq!(fs::read(path.join(".emilybase-objects")).unwrap(), marker);
    assert_eq!(
        fs::read(path.join(format!("{OBJECT}.object"))).unwrap(),
        image
    );
    success(run(command(&path, PROJECT, "list", None), Vec::new()));
}
