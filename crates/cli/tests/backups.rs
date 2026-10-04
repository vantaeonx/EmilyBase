use std::path::Path;
use std::process::{Command, Output};

use serde_json::json;

fn run(command: &str, paths: &[&Path], args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg(command)
        .args(paths)
        .args(args)
        .output()
        .unwrap()
}

fn ok(command: &str, paths: &[&Path], args: &[&str]) -> String {
    let output = run(command, paths, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn seed(path: &Path) {
    ok("db-init", &[path], &["--durable"]);
    let schema = json!({"name":"items","columns":[
        {"name":"id","data_type":"integer","nullable":false},
        {"name":"text","data_type":"text","nullable":false}
    ],"primary_key":0})
    .to_string();
    ok("table-create", &[path], &[&schema]);
    ok(
        "row-insert",
        &[path],
        &[
            "items",
            &json!([
                {"type":"integer","value":7},
                {"type":"text","value":"synthetic-private-value"}
            ])
            .to_string(),
        ],
    );
}

#[test]
fn actual_cli_verifies_restores_and_continues_writing_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("исходная база");
    let backup = dir.path().join("копия с пробелами.backup");
    let target = dir.path().join("восстановленная база");
    seed(&source);
    let created = ok("backup", &[&source, &backup], &[]);
    assert!(created.contains("tables=1 rows=1 transaction=3"));
    assert!(!created.contains("synthetic-private-value"));
    assert_eq!(ok("backup-verify", &[&backup], &[]), created);
    assert_eq!(ok("restore", &[&backup, &target], &[]), created);
    assert_eq!(
        ok("row-scan", &[&source], &["items"]),
        ok("row-scan", &[&target], &["items"])
    );
    ok(
        "row-insert",
        &[&target],
        &[
            "items",
            &json!([
                {"type":"integer","value":8},{"type":"text","value":"new restored commit"}
            ])
            .to_string(),
        ],
    );
    let rows: serde_json::Value =
        serde_json::from_str(&ok("row-scan", &[&target], &["items"])).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert_eq!(ok("backup-verify", &[&backup], &[]), created);
}

#[test]
fn cli_refuses_overwrite_and_bad_archives_without_partial_destinations() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let backup = dir.path().join("snapshot.backup");
    seed(&source);
    ok("backup", &[&source, &backup], &[]);
    let before = std::fs::read(&backup).unwrap();
    assert!(!run("backup", &[&source, &backup], &[]).status.success());
    assert_eq!(std::fs::read(&backup).unwrap(), before);
    let existing = dir.path().join("existing");
    std::fs::create_dir(&existing).unwrap();
    assert!(!run("restore", &[&backup, &existing], &[]).status.success());
    assert_eq!(std::fs::read_dir(existing).unwrap().count(), 0);
    let mut damaged = before;
    damaged[emilybase_backup::HEADER_SIZE + 100] ^= 1;
    std::fs::write(&backup, damaged).unwrap();
    let target = dir.path().join("must-not-exist");
    let output = run("restore", &[&backup, &target], &[]);
    assert!(!output.status.success());
    assert!(!target.exists());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-private-value"));
    assert!(!run("backup-verify", &[&backup], &[]).status.success());
}

#[test]
fn backup_cli_requires_managed_ownership_and_rejects_legacy_files() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    seed(&source);
    let db = emilybase_transactions::Database::open(&source).unwrap();
    let backup = dir.path().join("blocked.backup");
    assert!(!run("backup", &[&source, &backup], &[]).status.success());
    assert!(!backup.exists());
    drop(db);
    let legacy = dir.path().join("legacy.emily");
    ok("db-init", &[&legacy], &[]);
    assert!(!run("backup", &[&legacy, &backup], &[]).status.success());
    assert!(!backup.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn native_cli_relative_publication_preserves_both_original_wal_versions() {
    for compacted in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("каталог с пробелами 界");
        std::fs::create_dir(&parent).unwrap();
        let source = parent.join("source");
        seed(&source);
        if compacted {
            ok("compact", &[&source], &[]);
        }
        let mut database = emilybase_transactions::Database::open(&source).unwrap();
        let wal = database.committed_wal().unwrap();
        let expected = emilybase_backup::encode(&wal).unwrap();
        let rows = database.view().unwrap().scan("items", 10).unwrap();
        let id = database.database_id();
        drop(database);
        let relative = |args: &[&str]| {
            let output = Command::new(env!("CARGO_BIN_EXE_emilybase"))
                .current_dir(&parent)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        };
        let report = relative(&["backup", "./source", "копия.backup"]);
        assert_eq!(relative(&["backup-verify", "./копия.backup"]), report);
        assert_eq!(relative(&["restore", "копия.backup", "./restored"]), report);
        assert_eq!(
            std::fs::read(parent.join("копия.backup")).unwrap(),
            expected
        );
        assert_eq!(std::fs::read(source.join("redo.wal")).unwrap(), wal);
        let restored = emilybase_transactions::Database::open(parent.join("restored")).unwrap();
        assert_eq!(restored.database_id(), id);
        assert_eq!(restored.view().unwrap().scan("items", 10).unwrap(), rows);
        drop(restored);
        let duplicate = run(
            "restore",
            &[&parent.join("копия.backup"), &parent.join("restored")],
            &[],
        );
        assert!(!duplicate.status.success());
        assert!(duplicate.stdout.is_empty());
        assert_eq!(
            std::fs::read(parent.join("restored").join("redo.wal")).unwrap(),
            wal
        );
        assert!(!std::fs::read_dir(&parent).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".emilybase-backup-")
        }));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn native_cli_refuses_archive_and_parent_aliases_without_path_or_data_disclosure() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("synthetic-private-source");
    seed(&source);
    let archive = temporary.path().join("synthetic-private.backup");
    ok("backup", &[&source, &archive], &[]);
    let bytes = std::fs::read(&archive).unwrap();
    let wal = std::fs::read(source.join("redo.wal")).unwrap();
    let alias = temporary.path().join("synthetic-private-alias");
    std::os::unix::fs::symlink(&archive, &alias).unwrap();
    let parent = temporary.path().join("outputs");
    std::fs::create_dir(&parent).unwrap();
    let linked_parent = temporary.path().join("parent-alias");
    std::os::unix::fs::symlink(&parent, &linked_parent).unwrap();
    let target = linked_parent.join("selected");
    let failures = [
        run("backup-verify", &[&alias], &[]),
        run("restore", &[&alias, &parent.join("restored")], &[]),
        run("backup", &[&source, &target], &[]),
        run("restore", &[&archive, &target], &[]),
    ];
    for output in failures {
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        for private in ["synthetic-private", temporary.path().to_str().unwrap()] {
            assert!(!error.contains(private));
        }
    }
    assert_eq!(std::fs::read(&archive).unwrap(), bytes);
    assert_eq!(std::fs::read(source.join("redo.wal")).unwrap(), wal);
    assert_eq!(std::fs::read_dir(parent).unwrap().count(), 0);
    assert!(
        std::fs::symlink_metadata(&alias)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
