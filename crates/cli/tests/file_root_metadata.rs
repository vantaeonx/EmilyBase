use emilybase_files::{FileQuota, NativeFileRoot, initialize_file_root};
use emilybase_transactions::Database;
use serde_json::Value;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const PROJECT: &str = "01010101010101010101010101010101";
const FILE: &str = "02020202020202020202020202020202";
// A peer fork must not inherit another fixture's temporarily owned WAL lock.
static TEST_IO: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn fixture(parent: &Path, compacted: bool) -> (PathBuf, String) {
    let path = parent.join("root");
    initialize_file_root(
        &path,
        PROJECT.parse().unwrap(),
        FileQuota::new(4, 4096).unwrap(),
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
        "03030303030303030303030303030303".parse().unwrap(),
        [4; 16],
        "private-display",
        b"private-payload",
    )
    .unwrap();
    let database = root
        .inspect()
        .unwrap()
        .database_id()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    (path, database)
}
fn command(operation: &str, path: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    c.arg(operation)
        .arg(path)
        .arg(PROJECT)
        .args(args)
        .stdin(Stdio::null());
    c
}
fn accepted(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let text = String::from_utf8_lossy(&output.stdout);
    for private in [
        "private-display",
        "private-payload",
        "renamed-private",
        "04040404040404040404040404040404",
    ] {
        assert!(!text.contains(private));
    }
    serde_json::from_slice(&output.stdout).unwrap()
}
fn refused(output: Output) {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let text = String::from_utf8_lossy(&output.stderr);
    for private in [
        "private-display",
        "private-payload",
        "renamed-private",
        "panicked",
    ] {
        assert!(!text.contains(private));
    }
}
fn quota(path: &Path, database: &str, revision: &str, objects: &str, bytes: &str) -> Command {
    command(
        "file-root-quota",
        path,
        &[
            "--database",
            database,
            "--revision",
            revision,
            "--max-objects",
            objects,
            "--max-bytes",
            bytes,
        ],
    )
}

#[test]
fn actual_cli_metadata_receipts_cas_noops_and_physical_quota_on_both_wals() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (path, database) = fixture(temp.path(), compacted);
        let info = accepted(command("file-root-stat", &path, &[FILE]).output().unwrap());
        assert_eq!(info.as_object().unwrap().len(), 7);
        assert_eq!(info["file"], FILE);
        assert_eq!(info["revision"], "3");
        assert_eq!(info["bytes"], 15);
        let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
        assert_eq!(
            accepted(
                command(
                    "file-root-rename",
                    &path,
                    &[FILE, "--revision", "3", "--name", "private-display"]
                )
                .output()
                .unwrap()
            ),
            info
        );
        assert_eq!(
            accepted(quota(&path, &database, "3", "4", "4096").output().unwrap())["revision"],
            "3"
        );
        assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
        let renamed = accepted(
            command(
                "file-root-rename",
                &path,
                &[FILE, "--revision", "3", "--name", "renamed-private"],
            )
            .output()
            .unwrap(),
        );
        assert_eq!(renamed["revision"], "4");
        let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
        refused(
            command(
                "file-root-rename",
                &path,
                &[FILE, "--revision", "3", "--name", "renamed-private"],
            )
            .output()
            .unwrap(),
        );
        refused(
            command("file-root-remove", &path, &[FILE, "--revision", "3"])
                .output()
                .unwrap(),
        );
        refused(quota(&path, &database, "3", "4", "4096").output().unwrap());
        assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
        let removed = accepted(
            command("file-root-remove", &path, &[FILE, "--revision", "4"])
                .output()
                .unwrap(),
        );
        assert_eq!(removed.as_object().unwrap().len(), 6);
        assert_eq!(removed["removed_revision"], "4");
        assert_eq!(removed["revision"], "5");
        refused(command("file-root-stat", &path, &[FILE]).output().unwrap());
        refused(
            command("file-root-remove", &path, &[FILE, "--revision", "4"])
                .output()
                .unwrap(),
        );
        refused(quota(&path, &database, "5", "0", "15").output().unwrap());
        refused(quota(&path, &database, "5", "1", "14").output().unwrap());
        let lowered = accepted(quota(&path, &database, "5", "1", "15").output().unwrap());
        assert_eq!(lowered["revision"], "6");
        assert_eq!(lowered["usage"]["orphans"], 1);
        assert_eq!(lowered["usage"]["objects"], 1);
        assert_eq!(lowered["usage"]["bytes"], 15);
        let mut root = NativeFileRoot::open(&path, PROJECT.parse().unwrap()).unwrap();
        assert_eq!(
            root.capture().unwrap().objects().objects()[0].payload(),
            b"private-payload"
        );
        assert_eq!(
            root.inspect().unwrap().quota(),
            FileQuota::new(1, 15).unwrap()
        );
    }
}

#[test]
fn actual_cli_refuses_foreign_scope_and_malformed_inputs_without_touching_source() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (path, database) = fixture(temp.path(), false);
    let other = tempfile::tempdir().unwrap();
    let (_, foreign) = fixture(other.path(), false);
    let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
    for bad in [
        "../database",
        "ABCDEFABCDEFABCDEFABCDEFABCDEFABCD",
        foreign.as_str(),
    ] {
        refused(quota(&path, bad, "3", "2", "32").output().unwrap());
    }
    for bad in ["-1", "18446744073709551616", "not-a-number"] {
        refused(quota(&path, &database, bad, "2", "32").output().unwrap());
    }
    refused(quota(&path, &database, "3", "129", "32").output().unwrap());
    let long = "x".repeat(257);
    for bad in ["", "\n", long.as_str()] {
        refused(
            command(
                "file-root-rename",
                &path,
                &[FILE, "--revision", "3", "--name", bad],
            )
            .output()
            .unwrap(),
        );
    }
    for bad in ["../file", "0202020202020202020202020202020A"] {
        refused(command("file-root-stat", &path, &[bad]).output().unwrap());
    }
    let out = Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg("file-root-remove")
        .arg(&path)
        .args(["09090909090909090909090909090909", FILE, "--revision", "3"])
        .output()
        .unwrap();
    refused(out);
    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
    let mut root = NativeFileRoot::open(&path, PROJECT.parse().unwrap()).unwrap();
    assert_eq!(root.inspect().unwrap().revision(), 3);
    assert_eq!(root.list().unwrap()[0].name(), "private-display");
}

#[test]
fn undelivered_cli_metadata_receipt_keeps_commit_and_does_not_allow_stale_retry() {
    let _serial = TEST_IO.lock().unwrap();
    for operation in ["rename", "remove", "quota"] {
        let temp = tempfile::tempdir().unwrap();
        let (path, database) = fixture(temp.path(), false);
        let mut c = match operation {
            "rename" => command(
                "file-root-rename",
                &path,
                &[FILE, "--revision", "3", "--name", "renamed-private"],
            ),
            "remove" => command("file-root-remove", &path, &[FILE, "--revision", "3"]),
            _ => quota(&path, &database, "3", "1", "15"),
        };
        refused(
            c.stdout(Stdio::from(
                File::options().write(true).open("/dev/full").unwrap(),
            ))
            .stderr(Stdio::piped())
            .output()
            .unwrap(),
        );
        let mut root = NativeFileRoot::open(&path, PROJECT.parse().unwrap()).unwrap();
        assert_eq!(root.inspect().unwrap().revision(), 4);
        assert_eq!(root.inspect().unwrap().usage().physical_objects, 1);
        match operation {
            "rename" => assert_eq!(root.list().unwrap()[0].name(), "renamed-private"),
            "remove" => assert!(root.list().unwrap().is_empty()),
            _ => assert_eq!(
                root.inspect().unwrap().quota(),
                FileQuota::new(1, 15).unwrap()
            ),
        }
        drop(root);
        let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
        refused(c.stdout(Stdio::piped()).output().unwrap());
        assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
    }
}
