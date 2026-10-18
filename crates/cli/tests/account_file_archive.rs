use emilybase_auth::password::PasswordPool;
use emilybase_files::{FileId, FileQuota, NativeFileRoot, initialize_file_root};
use emilybase_object_storage::{ObjectId, ProjectDirectory};
use emilybase_server::{
    AccountRoot, ProjectStore, capture_account_file_components, initialize_account_root,
    publish_account_file_archive, restore_account_bundle_bytes,
};
use serde_json::Value;
use std::fs::{self, File};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
static CASES: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[path = "account_file_archive/backup.rs"]
mod backup;
#[path = "account_file_archive/restore.rs"]
mod restore;
fn fixture(parent: &Path, empty: bool, compact: bool) -> (PathBuf, Vec<u8>, Option<String>) {
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
    let root = parent.join("root");
    let pool = PasswordPool::new(1).unwrap();
    if empty {
        let mut registry = ProjectStore::open(parent.join("registry")).unwrap();
        let bytes = registry.capture_account_bundle(&mut []).unwrap();
        drop(registry);
        restore_account_bundle_bytes(&bytes, &root, pool.clone(), 50).unwrap();
    } else {
        initialize_account_root(&root, "private-project-display", pool.clone(), 50).unwrap();
    }
    let mut accounts = AccountRoot::open(&root, pool).unwrap();
    let projects = accounts.projects().unwrap();
    let mut files = Vec::new();
    let id = projects.first().map(|p| p.id.clone());
    if let Some(id) = &id {
        let project = id.parse().unwrap();
        let path = parent.join("files");
        initialize_file_root(&path, project, FileQuota::new(8, 1024).unwrap()).unwrap();
        let mut file = NativeFileRoot::open(&path, project).unwrap();
        file.publish(
            FileId::from_bytes([2; 16]),
            ObjectId::from_bytes([3; 16]),
            [4; 16],
            "private-display",
            b"private-payload",
        )
        .unwrap();
        drop(file);
        ProjectDirectory::open(path.join("objects"), project)
            .unwrap()
            .put(ObjectId::from_bytes([5; 16]), b"orphan")
            .unwrap();
        if compact {
            emilybase_transactions::Database::open(path.join("metadata"))
                .unwrap()
                .compact()
                .unwrap();
        }
        files.push(NativeFileRoot::open(&path, project).unwrap());
    }
    let snapshot = capture_account_file_components(&mut accounts, &mut files).unwrap();
    let target = parent.join("архив 界 с пробелами.bin");
    publish_account_file_archive(&snapshot, &target).unwrap();
    let bytes = fs::read(&target).unwrap();
    (target, bytes, id)
}
fn run(parent: &Path, path: &Path, stdout: Option<File>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    command
        .current_dir(parent)
        .arg("account-file-verify")
        .arg(path)
        .stdout(stdout.map(Stdio::from).unwrap_or_else(Stdio::piped))
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}
fn refused(output: Output) {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    for secret in [
        "panicked",
        "private-payload",
        "private-display",
        "private-project-display",
        "$argon2id$",
    ] {
        assert!(!error.contains(secret));
    }
}

#[test]
fn actual_cli_verifies_empty_nonempty_and_both_wals_with_only_bounded_counts_and_no_writes() {
    let _serial = CASES.lock().unwrap();
    for empty in [false, true] {
        for compact in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let (path, bytes, id) = fixture(temp.path(), empty, compact);
            let output = run(temp.path(), Path::new("./архив 界 с пробелами.bin"), None);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.stderr.is_empty());
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value.as_object().unwrap().len(), 9);
            assert_eq!(value["format"], 1);
            assert_eq!(value["projects"], usize::from(!empty));
            assert_eq!(value["private_projects"], usize::from(!empty));
            assert_eq!(value["file_stores"], usize::from(!empty));
            assert_eq!(value["references"], usize::from(!empty));
            assert_eq!(value["physical_objects"], usize::from(!empty) * 2);
            assert_eq!(value["archive_bytes"], bytes.len());
            assert_eq!(value["reset_at"], "50");
            let text = String::from_utf8(output.stdout).unwrap();
            for secret in [
                "private-payload",
                "private-display",
                "private-project-display",
                "$argon2id$",
            ] {
                assert!(!text.contains(secret));
            }
            if let Some(id) = id {
                assert!(!text.contains(&id));
            }
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
}

#[test]
fn actual_cli_refuses_aliases_broad_modes_invalid_oversized_and_missing_without_receipts_or_repairs()
 {
    let _serial = CASES.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (path, bytes, _) = fixture(temp.path(), false, false);
    let link = temp.path().join("link");
    symlink(&path, &link).unwrap();
    refused(run(temp.path(), &link, None));
    let alias = temp.path().join("alias");
    fs::hard_link(&path, &alias).unwrap();
    refused(run(temp.path(), &path, None));
    refused(run(temp.path(), &alias, None));
    fs::remove_file(alias).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    refused(run(temp.path(), &path, None));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let invalid = temp.path().join("invalid");
    fs::write(&invalid, b"private-payload").unwrap();
    fs::set_permissions(&invalid, fs::Permissions::from_mode(0o600)).unwrap();
    refused(run(temp.path(), &invalid, None));
    assert_eq!(fs::read(&invalid).unwrap(), b"private-payload");
    File::options()
        .write(true)
        .open(&invalid)
        .unwrap()
        .set_len(emilybase_server::MAX_ACCOUNT_FILE_ARCHIVE_BYTES as u64 + 1)
        .unwrap();
    refused(run(temp.path(), &invalid, None));
    let missing = temp.path().join("missing");
    refused(run(temp.path(), &missing, None));
    assert!(!missing.exists());
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn failed_count_receipt_delivery_is_a_process_error_and_preserves_the_selected_archive() {
    let _serial = CASES.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (path, bytes, _) = fixture(temp.path(), false, true);
    refused(run(
        temp.path(),
        &path,
        Some(File::options().write(true).open("/dev/full").unwrap()),
    ));
    assert_eq!(fs::read(path).unwrap(), bytes);
}
