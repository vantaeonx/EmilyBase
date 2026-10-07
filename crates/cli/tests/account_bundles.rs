use emilybase_auth::{accounts::AccountStore, password::PasswordPool};
use emilybase_server::ProjectStore;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::{Command, Output};

// Serialize file ownership checks around this binary's fork/exec windows.
static CASES: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn run(parent: &Path, path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .current_dir(parent)
        .arg("account-bundle-verify")
        .arg(path)
        .output()
        .unwrap()
}
fn text(output: &Output) -> String {
    [output.stdout.as_slice(), output.stderr.as_slice()]
        .iter()
        .map(|bytes| String::from_utf8_lossy(bytes))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn actual_cli_checks_both_wals_all_private_versions_and_preserves_live_credentials() {
    let _serial = CASES.lock().unwrap();
    for version in [1, 2, 3] {
        for compact in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("registry");
            let mut registry = ProjectStore::open(&root).unwrap();
            let created = registry.create("synthetic_private_project").unwrap();
            registry.authorize(&created.project.id,&created.api_key).unwrap()
                .execute("CREATE TABLE t(id INT PRIMARY KEY,v TEXT); INSERT INTO t VALUES(1,'synthetic_private_row')",&[]).unwrap();
            let private = dir.path().join("private");
            let mut accounts = [AccountStore::create(
                &private,
                &created.project.id,
                PasswordPool::new(1).unwrap(),
            )
            .unwrap()];
            let user = accounts[0]
                .create_user("synthetic_login", b"synthetic-password")
                .unwrap();
            let token = if version == 3 {
                accounts[0].enable_session_clock(100).unwrap();
                Some(
                    accounts[0]
                        .sign_in("synthetic_login", b"synthetic-password", 100)
                        .unwrap(),
                )
            } else {
                if version == 2 {
                    accounts[0].enable_session_storage().unwrap();
                }
                None
            };
            let data = root.join(&created.project.id).join("data");
            if compact {
                accounts[0].compact().unwrap();
                emilybase_transactions::Database::open(&data)
                    .unwrap()
                    .compact()
                    .unwrap();
            }
            let archive = dir.path().join("копия 界 с пробелами.account-bundle");
            registry
                .backup_account_bundle(&mut accounts, &archive)
                .unwrap();
            let source = fs::read(data.join("redo.wal")).unwrap();
            let private_before = fs::read(private.join("redo.wal")).unwrap();
            let archive_before = fs::read(&archive).unwrap();
            let output = run(
                dir.path(),
                Path::new("./копия 界 с пробелами.account-bundle"),
            );
            assert!(output.status.success());
            assert!(output.stderr.is_empty());
            let printed = String::from_utf8(output.stdout.clone()).unwrap();
            assert!(printed.starts_with(&format!("verified bundle projects=1 private_stores=1 tables=1 rows=1 accounts=1 session_families={} archive_bytes=",usize::from(version==3))));
            for secret in [
                &created.project.id,
                &created.api_key,
                "synthetic_login",
                "synthetic-password",
                "synthetic_private_project",
                "synthetic_private_row",
            ] {
                assert!(!text(&output).contains(secret));
            }
            assert_eq!(fs::read(&archive).unwrap(), archive_before);
            assert_eq!(fs::read(data.join("redo.wal")).unwrap(), source);
            assert_eq!(fs::read(private.join("redo.wal")).unwrap(), private_before);
            assert_eq!(
                accounts[0]
                    .check_password("synthetic_login", b"synthetic-password")
                    .unwrap(),
                Some(user)
            );
            if let Some(token) = token {
                assert!(
                    accounts[0]
                        .verify_access(token.access.expose(), 100)
                        .is_ok()
                );
            }
            assert_eq!(
                registry
                    .authorize(&created.project.id, &created.api_key)
                    .unwrap()
                    .status()
                    .unwrap()
                    .rows,
                1
            );
        }
    }
}

#[test]
fn actual_cli_distinguishes_empty_and_explicit_subset_inventories_without_discovery() {
    let _serial = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut registry = ProjectStore::open(dir.path().join("registry")).unwrap();
    let archive = dir.path().join("empty.account-bundle");
    registry.backup_account_bundle(&mut [], &archive).unwrap();
    let empty = run(dir.path(), &archive);
    assert!(empty.status.success());
    assert!(text(&empty).starts_with("verified bundle projects=0 private_stores=0 tables=0 rows=0 accounts=0 session_families=0 archive_bytes=256"));
    let created = registry.create("synthetic_private_project").unwrap();
    let mut private = [AccountStore::create(
        dir.path().join("private"),
        &created.project.id,
        PasswordPool::new(1).unwrap(),
    )
    .unwrap()];
    private[0]
        .create_user("synthetic_login", b"synthetic-password")
        .unwrap();
    let subset = dir.path().join("subset.account-bundle");
    registry.backup_account_bundle(&mut [], &subset).unwrap();
    let output = run(dir.path(), &subset);
    assert!(output.status.success());
    assert!(text(&output).starts_with(
        "verified bundle projects=1 private_stores=0 tables=0 rows=0 accounts=0 session_families=0"
    ));
    assert_eq!(private[0].count().unwrap(), 1);
}

#[test]
fn actual_cli_rejects_unsafe_or_corrupt_files_with_no_private_output_or_new_paths() {
    let _serial = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut registry = ProjectStore::open(dir.path().join("registry")).unwrap();
    let created = registry.create("synthetic_private_project").unwrap();
    let mut private = [AccountStore::create(
        dir.path().join("private"),
        &created.project.id,
        PasswordPool::new(1).unwrap(),
    )
    .unwrap()];
    let archive = dir.path().join("valid.account-bundle");
    registry
        .backup_account_bundle(&mut private, &archive)
        .unwrap();
    let before = fs::read(&archive).unwrap();
    let damaged = dir.path().join("damaged.account-bundle");
    let mut broken = before.clone();
    let last = broken.len() - 1;
    broken[last] ^= 1;
    fs::write(&damaged, &broken).unwrap();
    fs::set_permissions(&damaged, fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.path().join("link");
    symlink(&archive, &link).unwrap();
    let missing = dir.path().join("missing");
    for path in [&damaged, &link, &missing, dir.path()] {
        let output = run(dir.path(), path);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let printed = text(&output);
        assert!(printed.contains("error:"));
        for secret in [
            &created.project.id,
            &created.api_key,
            "synthetic_private_project",
            "synthetic-password",
        ] {
            assert!(!printed.contains(secret));
        }
    }
    assert!(!missing.exists());
    fs::set_permissions(&archive, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!run(dir.path(), &archive).status.success());
    fs::set_permissions(&archive, fs::Permissions::from_mode(0o600)).unwrap();
    let alias = dir.path().join("hardlink");
    fs::hard_link(&archive, &alias).unwrap();
    assert!(!run(dir.path(), &archive).status.success());
    fs::remove_file(alias).unwrap();
    assert_eq!(fs::read(archive).unwrap(), before);
    assert_eq!(fs::read(damaged).unwrap(), broken);
    assert!(
        registry
            .authorize(&created.project.id, &created.api_key)
            .is_ok()
    );
}
