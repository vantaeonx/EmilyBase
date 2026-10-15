use emilybase_files::{FileQuota, NativeFileRoot, initialize_file_root};
use emilybase_object_storage::{MAX_PAYLOAD_BYTES, ObjectId, ProjectDirectory};
use emilybase_transactions::Database;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const PROJECT: &str = "01010101010101010101010101010101";
const FILE: &str = "02020202020202020202020202020202";
const OBJECT: &str = "03030303030303030303030303030303";
static TEST_IO: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn fixture(parent: &Path, compacted: bool, payload: &[u8]) -> PathBuf {
    let path = parent.join("root");
    initialize_file_root(
        &path,
        PROJECT.parse().unwrap(),
        FileQuota::new(2, MAX_PAYLOAD_BYTES as u64 + 6).unwrap(),
    )
    .unwrap();
    if compacted {
        Database::open(path.join("metadata"))
            .unwrap()
            .compact()
            .unwrap();
    }
    let mut root = NativeFileRoot::open(&path, PROJECT.parse().unwrap()).unwrap();
    root.publish(
        FILE.parse().unwrap(),
        OBJECT.parse().unwrap(),
        [4; 16],
        "private-display",
        payload,
    )
    .unwrap();
    drop(root);
    ProjectDirectory::open(path.join("objects"), PROJECT.parse().unwrap())
        .unwrap()
        .put(ObjectId::from_bytes([5; 16]), b"orphan")
        .unwrap();
    path
}
fn command(path: &Path, project: &str, file: &str) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    c.arg("file-root-cat")
        .arg(path)
        .arg(project)
        .arg(file)
        .stdin(Stdio::null());
    c
}
fn refused(output: Output) {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let s = String::from_utf8_lossy(&output.stderr);
    for private in ["private-display", "private-payload", "panicked"] {
        assert!(!s.contains(private));
    }
}

#[test]
fn actual_cli_streams_exact_empty_binary_and_maximum_payload_without_metadata_output() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for length in [0, 1, 8193, MAX_PAYLOAD_BYTES] {
            let temp = tempfile::tempdir().unwrap();
            let payload = (0..length).map(|i| (i % 251) as u8).collect::<Vec<_>>();
            let path = fixture(temp.path(), compacted, &payload);
            let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
            let out = command(Path::new("root"), PROJECT, FILE)
                .current_dir(temp.path())
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(out.stderr.is_empty());
            assert_eq!(out.stdout, payload);
            assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
            let mut root = NativeFileRoot::open(&path, PROJECT.parse().unwrap()).unwrap();
            assert_eq!(root.inspect().unwrap().revision(), 3);
            assert_eq!(root.inspect().unwrap().usage().orphans, 1);
            root.rename(FILE.parse().unwrap(), 3, "next").unwrap();
        }
    }
}

#[test]
fn actual_cli_refuses_orphan_missing_malformed_foreign_busy_and_corrupt_source_before_output() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = fixture(temp.path(), false, b"private-payload");
    let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
    for (project, file) in [
        (PROJECT, "../file"),
        (PROJECT, "0202020202020202020202020202020A"),
        ("../project", FILE),
        ("09090909090909090909090909090909", FILE),
        (PROJECT, "05050505050505050505050505050505"),
    ] {
        refused(command(&path, project, file).output().unwrap());
    }
    let owner = Database::open(path.join("metadata")).unwrap();
    refused(command(&path, PROJECT, FILE).output().unwrap());
    drop(owner);
    let owner = ProjectDirectory::open(path.join("objects"), PROJECT.parse().unwrap()).unwrap();
    refused(command(&path, PROJECT, FILE).output().unwrap());
    drop(owner);
    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
    let mut root = NativeFileRoot::open(&path, PROJECT.parse().unwrap()).unwrap();
    root.remove(FILE.parse().unwrap(), 3).unwrap();
    drop(root);
    refused(command(&path, PROJECT, FILE).output().unwrap());
    let second = tempfile::tempdir().unwrap();
    let path = fixture(second.path(), true, b"private-payload");
    let object = path.join(format!("objects/{OBJECT}.object"));
    let mut bytes = fs::read(&object).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&object, &bytes).unwrap();
    refused(command(&path, PROJECT, FILE).output().unwrap());
    assert_eq!(fs::read(object).unwrap(), bytes);
}

#[test]
fn failed_binary_delivery_and_narrow_stdout_pipe_preserve_original_data_and_release_owners() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let payload = vec![0x53; 8193];
    let path = fixture(temp.path(), false, &payload);
    let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
    refused(
        command(&path, PROJECT, FILE)
            .stdout(Stdio::from(
                File::options().write(true).open("/dev/full").unwrap(),
            ))
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
    );
    let (read, write) = rustix::pipe::pipe().unwrap();
    rustix::pipe::fcntl_setpipe_size(&write, 4096).unwrap();
    let mut child = command(&path, PROJECT, FILE)
        .stdout(Stdio::from(File::from(write)))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let thread = std::thread::spawn(move || {
        let mut input = File::from(read);
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes).unwrap();
        bytes
    });
    assert!(child.wait().unwrap().success());
    assert_eq!(thread.join().unwrap(), payload);
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(stderr.is_empty());
    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
    let mut root = NativeFileRoot::open(&path, PROJECT.parse().unwrap()).unwrap();
    assert_eq!(root.inspect().unwrap().revision(), 3);
    assert_eq!(
        root.capture()
            .unwrap()
            .objects()
            .objects()
            .iter()
            .find(|o| o.object().to_string() == OBJECT)
            .unwrap()
            .payload(),
        payload
    );
    root.rename(FILE.parse().unwrap(), 3, "next").unwrap();
}

#[test]
fn source_replacement_during_blocked_stdout_refuses_next_chunk_and_preserves_prior_delivery() {
    let _serial = TEST_IO.lock().unwrap();
    for kind in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let payload = vec![0x53; 3 * 8192];
        let path = fixture(temp.path(), false, &payload);
        let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
        let (read, write) = rustix::pipe::pipe().unwrap();
        rustix::pipe::fcntl_setpipe_size(&write, 4096).unwrap();
        let mut child = command(&path, PROJECT, FILE)
            .stdout(Stdio::from(File::from(write)))
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (ready_send, ready_receive) = std::sync::mpsc::channel();
        let (resume_send, resume_receive) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut input = File::from(read);
            let mut first = [0];
            let result = input.read_exact(&mut first);
            ready_send.send(result.is_ok()).unwrap();
            resume_receive.recv().unwrap();
            let mut bytes = if result.is_ok() {
                first.to_vec()
            } else {
                vec![]
            };
            input.read_to_end(&mut bytes).unwrap();
            bytes
        });
        let ready = ready_receive.recv_timeout(std::time::Duration::from_secs(15));
        if !matches!(ready, Ok(true)) {
            let _ = child.kill();
            resume_send.send(()).unwrap();
            let _ = child.wait();
            thread.join().unwrap();
            panic!("native stream did not reach the blocked-output boundary");
        }
        assert!(Database::open(path.join("metadata")).is_err());
        assert!(ProjectDirectory::open(path.join("objects"), PROJECT.parse().unwrap()).is_err());
        let saved = temp.path().join("saved");
        match kind {
            0 => {
                fs::rename(&path, &saved).unwrap();
                fs::DirBuilder::new().create(&path).unwrap();
                // Root selection fails before any replacement can be opened.
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            }
            1 => {
                let object = path.join(format!("objects/{OBJECT}.object"));
                fs::rename(&object, &saved).unwrap();
                fs::copy(&saved, object).unwrap();
            }
            _ => {
                let original = path.join("metadata/redo.wal");
                fs::rename(&original, &saved).unwrap();
                fs::copy(&saved, original).unwrap();
            }
        }
        resume_send.send(()).unwrap();
        let status = child.wait().unwrap();
        let delivered = thread.join().unwrap();
        assert!(!status.success());
        assert_eq!(delivered, payload[..8192]);
        let mut stderr = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        assert!(!stderr.contains("panicked"));
        assert!(!stderr.contains("private-display"));
        let original = if kind == 0 { &saved } else { &path };
        assert_eq!(fs::read(original.join("metadata/redo.wal")).unwrap(), wal);
        assert_eq!(
            NativeFileRoot::open(original, PROJECT.parse().unwrap())
                .unwrap()
                .inspect()
                .unwrap()
                .revision(),
            3
        );
    }
}
