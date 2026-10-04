use emilybase_server::ProjectStore;
use std::path::Path;
use std::process::{Command, Output};

// Keep ownership checks outside another case's fork/exec inheritance window.
static CASES: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn command(action: &str, paths: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg(action)
        .args(paths)
        .output()
        .unwrap()
}

#[test]
fn actual_registry_backup_cli_preserves_keys_rows_epochs_and_existing_destinations() {
    let _serial = CASES.lock().unwrap();
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

#[test]
fn actual_relative_registry_cli_preserves_empty_and_mixed_wal_registries() {
    let _serial = CASES.lock().unwrap();
    for count in [0, 3] {
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("каталог 界 с пробелами");
        std::fs::create_dir(&parent).unwrap();
        let source = parent.join("source");
        let mut store = ProjectStore::open(&source).unwrap();
        let mut model = Vec::new();
        for number in 0..count {
            let created = store.create("synthetic relative CLI model").unwrap();
            let id = created.project.id;
            let key = created.api_key;
            store
                .authorize(&id, &key)
                .unwrap()
                .execute(
                    "CREATE TABLE t(id INT PRIMARY KEY,n INT);INSERT INTO t VALUES(1,$1)",
                    &[emilybase_catalog::Value::Integer(number)],
                )
                .unwrap();
            let rotated = store.rotate(&id).unwrap().api_key;
            if number % 2 == 0 {
                emilybase_transactions::Database::open(source.join(&id).join("data"))
                    .unwrap()
                    .compact()
                    .unwrap();
            }
            model.push((id, key, rotated, number));
        }
        let expected = store.backup_image().unwrap();
        drop(store);
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
            for (id, key, rotated, _) in &model {
                for private in [id, key, rotated] {
                    assert!(!String::from_utf8_lossy(&output.stdout).contains(private));
                    assert!(!String::from_utf8_lossy(&output.stderr).contains(private));
                }
            }
            output.stdout
        };
        let report = relative(&["projects-backup", "./source", "копия.backup"]);
        assert_eq!(
            relative(&["projects-backup-verify", "./копия.backup"]),
            report
        );
        assert_eq!(
            relative(&["projects-restore", "копия.backup", "./restored"]),
            report
        );
        assert_eq!(
            std::fs::read(parent.join("копия.backup")).unwrap(),
            expected
        );
        let mut restored = ProjectStore::open_existing(parent.join("restored")).unwrap();
        assert_eq!(restored.backup_image().unwrap(), expected);
        for (id, old, current, value) in &model {
            assert!(restored.authorize(id, old).is_err());
            assert_eq!(
                restored
                    .list()
                    .unwrap()
                    .iter()
                    .find(|project| &project.id == id)
                    .unwrap()
                    .key_epoch,
                2
            );
            let report = restored
                .authorize(id, current)
                .unwrap()
                .execute("SELECT id,n FROM t", &[])
                .unwrap();
            assert_eq!(
                report.results[0].rows,
                vec![vec![
                    emilybase_catalog::Value::Integer(1),
                    emilybase_catalog::Value::Integer(*value)
                ]]
            );
            restored
                .authorize(id, current)
                .unwrap()
                .execute("INSERT INTO t VALUES(2,99)", &[])
                .unwrap();
        }
        drop(restored);
        assert_eq!(
            ProjectStore::open_existing(&source)
                .unwrap()
                .backup_image()
                .unwrap(),
            expected
        );
        assert_eq!(
            std::fs::read(parent.join("копия.backup")).unwrap(),
            expected
        );
        assert!(!std::fs::read_dir(parent).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".emilybase-registry-")
        }));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn native_registry_cli_refuses_parent_aliases_without_disclosing_project_credentials() {
    let _serial = CASES.lock().unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    let mut store = ProjectStore::open(&source).unwrap();
    let project = store.create("synthetic-private-project").unwrap();
    let expected = store.backup_image().unwrap();
    drop(store);
    let archive = temporary.path().join("source.backup");
    assert!(
        command("projects-backup", &[&source, &archive])
            .status
            .success()
    );
    let parent = temporary.path().join("outputs");
    std::fs::create_dir(&parent).unwrap();
    let alias = temporary.path().join("alias");
    std::os::unix::fs::symlink(&parent, &alias).unwrap();
    let target = alias.join("selected");
    for output in [
        command("projects-backup", &[&source, &target]),
        command("projects-restore", &[&archive, &target]),
    ] {
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        for private in [
            &project.api_key,
            &project.project.id,
            "synthetic-private-project",
            temporary.path().to_str().unwrap(),
        ] {
            assert!(!String::from_utf8_lossy(&output.stderr).contains(private));
        }
    }
    assert_eq!(std::fs::read(&archive).unwrap(), expected);
    assert_eq!(
        ProjectStore::open_existing(&source)
            .unwrap()
            .backup_image()
            .unwrap(),
        expected
    );
    assert_eq!(std::fs::read_dir(parent).unwrap().count(), 0);
    assert!(
        std::fs::symlink_metadata(alias)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
