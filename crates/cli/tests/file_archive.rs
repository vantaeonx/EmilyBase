use emilybase_files::{FileId, FileQuota, FileStore, publish_file_archive};
use emilybase_object_storage::{ObjectId, ProjectDirectory, ProjectId};
use emilybase_transactions::Database;
use serde_json::Value;
use std::fs::{self, File};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const PROJECT: &str = "01010101010101010101010101010101";
const FILE: FileId = FileId::from_bytes([2; 16]);
const PRIVATE: &[u8] = b"synthetic-private-file\0\xff";
fn fixture(parent: &Path, compacted: bool, empty: bool) -> (PathBuf, Vec<u8>, Value) {
    let source = parent.join("source");
    fs::create_dir(&source).unwrap();
    let mut database = Database::create(source.join("metadata")).unwrap();
    if compacted {
        database.compact().unwrap();
    }
    fs::DirBuilder::new()
        .mode(0o700)
        .create(source.join("objects"))
        .unwrap();
    let project = PROJECT.parse::<ProjectId>().unwrap();
    let objects = ProjectDirectory::initialize(source.join("objects"), project).unwrap();
    let mut store =
        FileStore::initialize(database, objects, FileQuota::new(8, 32768).unwrap()).unwrap();
    let payload = if empty { &[][..] } else { PRIVATE };
    store
        .publish(
            FILE,
            ObjectId::from_bytes([3; 16]),
            [4; 16],
            "private-display",
            payload,
        )
        .unwrap();
    let orphan_file = FileId::from_bytes([5; 16]);
    let orphan = store
        .publish(
            orphan_file,
            ObjectId::from_bytes([6; 16]),
            [7; 16],
            "orphan-display",
            b"orphan",
        )
        .unwrap();
    store.remove(orphan_file, orphan.revision()).unwrap();
    let quota = FileQuota::new(2, payload.len() as u64 + 6).unwrap();
    let state = store.quota_state().unwrap();
    store.set_quota(state, quota).unwrap();
    let path = parent.join("copy.file-archive");
    let report = publish_file_archive(&store.capture().unwrap(), &path).unwrap();
    let expected = serde_json::json!({
        "format":1,"project":PROJECT,
        "metadata":{
            "database_id":report.metadata().database_id.iter().map(|b|format!("{b:02x}")).collect::<String>(),
            "last_transaction":report.metadata().last_transaction.to_string(),
            "wal_version":if compacted {2} else {1},"wal_bytes":report.metadata().wal_bytes,
            "tables":report.metadata().tables,"rows":report.metadata().rows,"pages":report.metadata().pages
        },
        "quota":{"max_objects":2,"max_bytes":payload.len() as u64+6},
        "references":1,
        "objects":{
            "count":2,"bytes":payload.len() as u64+6,
            "digest":report.objects().digest.iter().map(|b|format!("{b:02x}")).collect::<String>()
        }
    });
    let bytes = fs::read(&path).unwrap();
    (path, bytes, expected)
}
fn command(operation: &str, path: &Path, project: &str, target: Option<&Path>) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    command
        .arg(operation)
        .arg(path)
        .arg(project)
        .stdin(Stdio::null());
    if let Some(target) = target {
        command.arg(target);
    }
    command
}
fn run(mut command: Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("file archive CLI deadline");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.wait_with_output().unwrap()
}
fn success(out: Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    let text = String::from_utf8_lossy(&out.stdout);
    for secret in [
        "synthetic-private-file",
        "private-display",
        "orphan-display",
    ] {
        assert!(!text.contains(secret));
    }
    serde_json::from_slice(&out.stdout).unwrap()
}
fn refused(out: Output) {
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let text = String::from_utf8_lossy(&out.stderr);
    for secret in [
        "synthetic-private-file",
        "private-display",
        "orphan-display",
        "panicked",
    ] {
        assert!(!text.contains(secret));
    }
}
fn open(path: &Path) -> FileStore {
    FileStore::open(
        Database::open(path.join("metadata")).unwrap(),
        ProjectDirectory::open(path.join("objects"), PROJECT.parse().unwrap()).unwrap(),
    )
    .unwrap()
}

#[test]
fn actual_cli_checks_restores_and_reports_complete_pair_without_private_contents() {
    for compacted in [false, true] {
        for empty in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let (archive, bytes, expected) = fixture(temp.path(), compacted, empty);
            fs::set_permissions(&archive, fs::Permissions::from_mode(0o400)).unwrap();
            let checked = success(run(command("file-archive-verify", &archive, PROJECT, None)));
            assert_eq!(checked, expected);
            let mut restore = command(
                "file-archive-restore",
                Path::new("copy.file-archive"),
                PROJECT,
                Some(Path::new("restored")),
            );
            restore.current_dir(temp.path());
            let restored = success(run(restore));
            assert_eq!(restored, expected);
            let target = temp.path().join("restored");
            assert_eq!(fs::metadata(&target).unwrap().mode() & 0o777, 0o700);
            assert_eq!(fs::read_dir(&target).unwrap().count(), 2);
            let mut store = open(&target);
            assert_eq!(store.usage().unwrap().orphans, 1);
            assert_eq!(store.usage().unwrap().references, 1);
            let mut reader = store.reader(FILE).unwrap();
            let mut payload = Vec::new();
            let mut scratch = [0; 8192];
            loop {
                let n = reader.read_payload(&mut scratch).unwrap();
                if n == 0 {
                    break;
                }
                payload.extend_from_slice(&scratch[..n]);
            }
            reader.finish().unwrap();
            assert_eq!(payload, if empty { &[][..] } else { PRIVATE });
            let info = store.info(FILE).unwrap().unwrap();
            store
                .rename(FILE, info.revision(), "independent-write")
                .unwrap();
            drop(store);
            let later = fs::read(target.join("metadata/redo.wal")).unwrap();
            refused(run(command(
                "file-archive-restore",
                &archive,
                PROJECT,
                Some(&target),
            )));
            assert_eq!(fs::read(target.join("metadata/redo.wal")).unwrap(), later);
            assert_eq!(
                open(&target).info(FILE).unwrap().unwrap().name(),
                "independent-write"
            );
            assert_eq!(fs::read(&archive).unwrap(), bytes);
        }
    }
}

#[test]
fn actual_cli_rejects_input_scope_selection_and_size_before_any_stage_or_report() {
    for mutation in 0..9 {
        let temp = tempfile::tempdir().unwrap();
        let (archive, _, _) = fixture(temp.path(), false, false);
        let mut project = PROJECT;
        match mutation {
            0 => fs::write(&archive, b"damaged").unwrap(),
            1 => project = "03030303030303030303030303030303",
            2 => project = "../foreign",
            3 => {
                fs::rename(&archive, temp.path().join("saved")).unwrap();
                symlink(temp.path().join("saved"), &archive).unwrap();
            }
            4 => fs::hard_link(&archive, temp.path().join("alias")).unwrap(),
            5 => fs::set_permissions(&archive, fs::Permissions::from_mode(0o644)).unwrap(),
            6 => {
                fs::remove_file(&archive).unwrap();
                fs::create_dir(&archive).unwrap();
            }
            7 => {
                File::options()
                    .write(true)
                    .open(&archive)
                    .unwrap()
                    .set_len(emilybase_files::MAX_FILE_ARCHIVE_BYTES as u64 + 1)
                    .unwrap();
            }
            _ => {
                fs::remove_file(&archive).unwrap();
                rustix::fs::mknodat(
                    rustix::fs::CWD,
                    &archive,
                    rustix::fs::FileType::Fifo,
                    rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
                    0,
                )
                .unwrap();
            }
        }
        let target = temp.path().join("restored");
        let before = fs::read_dir(temp.path()).unwrap().count();
        refused(run(command("file-archive-verify", &archive, project, None)));
        refused(run(command(
            "file-archive-restore",
            &archive,
            project,
            Some(&target),
        )));
        assert!(!target.exists());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), before);
    }
}

#[test]
fn failed_stdout_preserves_selected_pair_and_does_not_make_retry_safe() {
    let temp = tempfile::tempdir().unwrap();
    let (archive, bytes, expected) = fixture(temp.path(), true, false);
    let target = temp.path().join("restored");
    let out = command("file-archive-restore", &archive, PROJECT, Some(&target))
        .stdout(Stdio::from(
            File::options().write(true).open("/dev/full").unwrap(),
        ))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    refused(out);
    assert_eq!(open(&target).usage().unwrap().physical_objects, 2);
    let wal = fs::read(target.join("metadata/redo.wal")).unwrap();
    refused(run(command(
        "file-archive-restore",
        &archive,
        PROJECT,
        Some(&target),
    )));
    assert_eq!(fs::read(target.join("metadata/redo.wal")).unwrap(), wal);
    assert_eq!(
        success(run(command("file-archive-verify", &archive, PROJECT, None))),
        expected
    );
    assert_eq!(fs::read(&archive).unwrap(), bytes);
}
