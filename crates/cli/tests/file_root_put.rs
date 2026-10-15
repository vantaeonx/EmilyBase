use emilybase_files::{FileId, FileQuota, NativeFileRoot, initialize_file_root};
use emilybase_object_storage::MAX_PAYLOAD_BYTES;
use emilybase_transactions::Database;
use serde_json::Value;
use std::fs::{self, File};
use std::path::Path;
use std::process::{Command, Output, Stdio};

const PROJECT: &str = "01010101010101010101010101010101";
const FILE: &str = "02020202020202020202020202020202";
const OBJECT: &str = "03030303030303030303030303030303";
const OWNER: &str = "04040404040404040404040404040404";
// Fixture owners and peer CLI forks must not overlap their inherited locks.
static TEST_IO: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn command(
    path: &Path,
    project: &str,
    file: &str,
    object: &str,
    owner: &str,
    name: &str,
    input: &Path,
) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    c.arg("file-root-put")
        .arg(path)
        .arg(project)
        .arg(file)
        .arg(object)
        .args(["--owner", owner, "--name", name])
        .stdin(Stdio::from(File::open(input).unwrap()));
    c
}
fn refused(out: Output) {
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let s = String::from_utf8_lossy(&out.stderr);
    assert!(!s.contains("private-display"));
    assert!(!s.contains("private-payload"));
    assert!(!s.contains("panicked"));
}

#[test]
fn actual_cli_publishes_exact_empty_binary_payload_and_count_only_receipt_on_both_wals() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for empty in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("root");
            initialize_file_root(
                &path,
                PROJECT.parse().unwrap(),
                FileQuota::new(2, 16384).unwrap(),
            )
            .unwrap();
            if compacted {
                let mut db = Database::open(path.join("metadata")).unwrap();
                db.compact().unwrap();
            }
            let payload = if empty { vec![] } else { vec![0x53; 8193] };
            let input = temp.path().join("input");
            fs::write(&input, &payload).unwrap();
            let out = command(
                Path::new("root"),
                PROJECT,
                FILE,
                OBJECT,
                OWNER,
                "private-display",
                &input,
            )
            .current_dir(temp.path())
            .output()
            .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(out.stderr.is_empty());
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(!text.contains("private-display"));
            assert!(!text.contains(OWNER));
            let report: Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(report.as_object().unwrap().len(), 7);
            assert_eq!(report["format"], 1);
            assert_eq!(report["project"], PROJECT);
            assert_eq!(report["file"], FILE);
            assert_eq!(report["object"], OBJECT);
            assert_eq!(report["revision"], "3");
            assert_eq!(report["bytes"], payload.len());
            assert_eq!(report["sha256"].as_str().unwrap().len(), 64);
            let mut root = NativeFileRoot::open(&path, PROJECT.parse().unwrap()).unwrap();
            let files = root.list().unwrap();
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].owner(), &[4; 16]);
            assert_eq!(files[0].name(), "private-display");
            let snapshot = root.capture().unwrap();
            assert_eq!(
                snapshot.metadata_report().wal_version,
                if compacted { 2 } else { 1 }
            );
            assert_eq!(snapshot.objects().objects()[0].payload(), payload);
            drop(root);
            let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
            refused(
                command(
                    &path,
                    PROJECT,
                    FILE,
                    "05050505050505050505050505050505",
                    OWNER,
                    "overwrite",
                    &input,
                )
                .output()
                .unwrap(),
            );
            assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
            assert_eq!(
                NativeFileRoot::open(&path, PROJECT.parse().unwrap())
                    .unwrap()
                    .inspect()
                    .unwrap()
                    .usage()
                    .physical_objects,
                1
            );
        }
    }
}

#[test]
fn actual_cli_rejects_identity_name_scope_capacity_and_oversize_without_metadata_or_blob_changes() {
    let _serial = TEST_IO.lock().unwrap();
    for kind in 0..8 {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("root");
        initialize_file_root(
            &path,
            PROJECT.parse().unwrap(),
            FileQuota::new(if kind == 6 { 0 } else { 2 }, 16384).unwrap(),
        )
        .unwrap();
        let input = temp.path().join("input");
        fs::write(&input, b"private-payload").unwrap();
        let mut project = PROJECT;
        let mut file = FILE;
        let mut object = OBJECT;
        let mut owner = OWNER;
        let long = "x".repeat(257);
        let mut name = "synthetic";
        match kind {
            0 => project = "../foreign",
            1 => file = "not-file",
            2 => object = "../object",
            3 => owner = "not-owner",
            4 => name = &long,
            5 => project = "09090909090909090909090909090909",
            6 => {}
            _ => File::options()
                .write(true)
                .open(&input)
                .unwrap()
                .set_len(MAX_PAYLOAD_BYTES as u64 + 1)
                .unwrap(),
        }
        let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
        let output = command(&path, project, file, object, owner, name, &input)
            .output()
            .unwrap();
        refused(output);
        assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
        assert_eq!(
            NativeFileRoot::open(&path, PROJECT.parse().unwrap())
                .unwrap()
                .inspect()
                .unwrap()
                .usage()
                .physical_objects,
            0
        );
    }
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("root");
    initialize_file_root(
        &path,
        PROJECT.parse().unwrap(),
        FileQuota::new(2, 16384).unwrap(),
    )
    .unwrap();
    let input = temp.path().join("oversize");
    File::create(&input)
        .unwrap()
        .set_len(MAX_PAYLOAD_BYTES as u64 + 1)
        .unwrap();
    let owner = Database::open(path.join("metadata")).unwrap();
    let output = command(&path, PROJECT, FILE, OBJECT, OWNER, "synthetic", &input)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("input exceeds"));
    refused(output);
    drop(owner);
}

#[test]
fn failed_receipt_delivery_keeps_acknowledged_reference_and_retry_cannot_overwrite() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("root");
    initialize_file_root(
        &path,
        PROJECT.parse().unwrap(),
        FileQuota::new(2, 16384).unwrap(),
    )
    .unwrap();
    let input = temp.path().join("input");
    fs::write(&input, b"private-payload\0\xff").unwrap();
    let out = command(
        &path,
        PROJECT,
        FILE,
        OBJECT,
        OWNER,
        "private-display",
        &input,
    )
    .stdout(Stdio::from(
        File::options().write(true).open("/dev/full").unwrap(),
    ))
    .stderr(Stdio::piped())
    .output()
    .unwrap();
    refused(out);
    let mut root = NativeFileRoot::open(&path, PROJECT.parse().unwrap()).unwrap();
    assert_eq!(root.inspect().unwrap().revision(), 3);
    assert_eq!(
        root.list().unwrap()[0].id(),
        FILE.parse::<FileId>().unwrap()
    );
    assert_eq!(
        root.capture().unwrap().objects().objects()[0].payload(),
        b"private-payload\0\xff"
    );
    drop(root);
    let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
    refused(
        command(&path, PROJECT, FILE, OBJECT, OWNER, "overwrite", &input)
            .output()
            .unwrap(),
    );
    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
}
