use emilybase_server::ProjectStore;
use std::path::Path;
use std::process::{Command, Output};

fn command(action: &str, paths: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg(action)
        .args(paths)
        .output()
        .unwrap()
}

#[test]
fn actual_registry_backup_cli_preserves_keys_rows_epochs_and_existing_destinations() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    let mut store = ProjectStore::open(&root).unwrap();
    let created = store.create("synthetic_private_name").unwrap();
    store.authorize(&created.project.id, &created.api_key).unwrap()
        .execute("CREATE TABLE t(id INT PRIMARY KEY,v TEXT); INSERT INTO t VALUES(1,'synthetic_private_row')", &[]).unwrap();
    let rotated = store.rotate(&created.project.id).unwrap();
    drop(store);
    let archive = temp.path().join("projects.backup");
    let copy = temp.path().join("independent");
    for (action, paths) in [
        ("projects-backup", vec![root.as_path(), archive.as_path()]),
        ("projects-backup-verify", vec![archive.as_path()]),
        ("projects-restore", vec![archive.as_path(), copy.as_path()]),
    ] {
        let output = command(action, &paths);
        assert!(output.status.success());
        let printed = String::from_utf8(output.stdout).unwrap();
        assert!(printed.starts_with("verified registry projects=1 tables=1 rows=1 archive_bytes="));
        for private in [
            &created.api_key,
            &rotated.api_key,
            &created.project.id,
            "synthetic_private_name",
            "synthetic_private_row",
        ] {
            assert!(!printed.contains(private));
            assert!(!String::from_utf8_lossy(&output.stderr).contains(private));
        }
    }
    let restored = ProjectStore::open_existing(&copy).unwrap();
    assert!(
        restored
            .authorize(&created.project.id, &created.api_key)
            .is_err()
    );
    let status = restored
        .authorize(&created.project.id, &rotated.api_key)
        .unwrap()
        .status()
        .unwrap();
    assert_eq!(status.transaction, 2);
    assert_eq!(status.rows, 1);
    assert_eq!(restored.list().unwrap()[0].key_epoch, 2);
    drop(restored);
    let before = std::fs::read(&archive).unwrap();
    assert!(
        !command("projects-backup", &[&root, &archive])
            .status
            .success()
    );
    assert!(
        !command("projects-restore", &[&archive, &copy])
            .status
            .success()
    );
    assert_eq!(std::fs::read(&archive).unwrap(), before);
    let missing = temp.path().join("missing");
    let unpublished = temp.path().join("unpublished.backup");
    assert!(
        !command("projects-backup", &[&missing, &unpublished])
            .status
            .success()
    );
    assert!(!missing.exists() && !unpublished.exists());
    let source = ProjectStore::open_existing(&root).unwrap();
    assert_eq!(
        source
            .authorize(&created.project.id, &rotated.api_key)
            .unwrap()
            .status()
            .unwrap()
            .transaction,
        2
    );
}
