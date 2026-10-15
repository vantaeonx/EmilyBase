use emilybase_files::{FileId, FileQuota, FileStore, initialize_file_root};
use emilybase_object_storage::{ObjectId, ProjectDirectory};
use emilybase_transactions::Database;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const PROJECT: &str = "01010101010101010101010101010101";
// Peer test forks can briefly inherit a fixture's native lock description before
// exec closes it. Keep owner creation/drop and CLI forks serial in this executable.
static TEST_IO: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn command(path: &Path, project: &str) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    c.arg("file-root-info").arg(path).arg(project);
    c
}
fn refused(out: Output) {
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(!error.contains("private-display"));
    assert!(!error.contains("panicked"));
}

#[test]
fn actual_cli_observes_exact_scope_quota_and_charge_without_names_owners_or_payload() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("root");
        initialize_file_root(
            &path,
            PROJECT.parse().unwrap(),
            FileQuota::new(8, 32768).unwrap(),
        )
        .unwrap();
        let mut db = Database::open(path.join("metadata")).unwrap();
        if compacted {
            db.compact().unwrap();
        }
        let mut store = FileStore::open(
            db,
            ProjectDirectory::open(path.join("objects"), PROJECT.parse().unwrap()).unwrap(),
        )
        .unwrap();
        let identity = store
            .capture()
            .unwrap()
            .metadata_report()
            .database_id
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        store
            .publish(
                FileId::from_bytes([2; 16]),
                ObjectId::from_bytes([3; 16]),
                [4; 16],
                "private-display",
                b"private-payload",
            )
            .unwrap();
        let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
        drop(store);
        let out = command(Path::new("root"), PROJECT)
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
        assert!(!text.contains("private-payload"));
        let report: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(
            report,
            serde_json::json!({"format":1,"project":PROJECT,"database_id":identity,"revision":"3","quota":{"max_objects":8,"max_bytes":32768},"usage":{"objects":1,"bytes":15,"references":1,"orphans":0}})
        );
        assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
    }
}

#[test]
fn actual_cli_scope_shape_and_busy_refusal_do_not_modify_existing_root() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("root");
    initialize_file_root(
        &path,
        PROJECT.parse().unwrap(),
        FileQuota::new(0, 0).unwrap(),
    )
    .unwrap();
    let before = fs::read(path.join("metadata/redo.wal")).unwrap();
    for project in ["../foreign", "09090909090909090909090909090909"] {
        refused(command(&path, project).output().unwrap());
    }
    let db = Database::open(path.join("metadata")).unwrap();
    refused(command(&path, PROJECT).output().unwrap());
    drop(db);
    fs::write(path.join("unexpected"), b"protected").unwrap();
    refused(command(&path, PROJECT).output().unwrap());
    assert_eq!(fs::read(path.join("unexpected")).unwrap(), b"protected");
    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), before);
}
