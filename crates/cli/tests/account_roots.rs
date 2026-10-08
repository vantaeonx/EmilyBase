use emilybase_auth::{
    accounts::{AccountInfo, AccountStore, IssuedSession},
    password::PasswordPool,
};
use emilybase_server::ProjectStore;
use std::fs::{self, File};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

static CASES: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn pool() -> PasswordPool {
    PasswordPool::new(1).unwrap()
}
fn run(parent: &Path, action: &str, paths: &[&Path], time: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    command.current_dir(parent).arg(action).args(paths);
    if let Some(time) = time {
        command.arg("--reset-at").arg(time);
    }
    command.output().unwrap()
}
fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
struct Seed {
    id: String,
    key: String,
    user: AccountInfo,
    old: Option<IssuedSession>,
    archive: PathBuf,
    data: PathBuf,
    private: PathBuf,
}
fn seed(parent: &Path, version: u16, compact: bool) -> Seed {
    let root = parent.join("registry");
    let mut registry = ProjectStore::open(&root).unwrap();
    let created = registry.create("synthetic_private_project").unwrap();
    registry.authorize(&created.project.id,&created.api_key).unwrap().execute("CREATE TABLE t(id INT PRIMARY KEY,v TEXT); INSERT INTO t VALUES(1,'synthetic_private_row')",&[]).unwrap();
    let private = parent.join("private");
    let mut account = AccountStore::create(&private, &created.project.id, pool()).unwrap();
    let user = account
        .create_user("synthetic_login", b"synthetic-password")
        .unwrap();
    let old = if version == 3 {
        account.enable_session_clock(100).unwrap();
        Some(
            account
                .sign_in("synthetic_login", b"synthetic-password", 100)
                .unwrap(),
        )
    } else {
        if version == 2 {
            account.enable_session_storage().unwrap();
        }
        None
    };
    let data = root.join(&created.project.id).join("data");
    if compact {
        account.compact().unwrap();
        emilybase_transactions::Database::open(&data)
            .unwrap()
            .compact()
            .unwrap();
    }
    let archive = parent.join("копия 界 с пробелами.account-bundle");
    registry
        .backup_account_bundle(std::slice::from_mut(&mut account), &archive)
        .unwrap();
    Seed {
        id: created.project.id,
        key: created.api_key,
        user,
        old,
        archive,
        data,
        private,
    }
}
fn redacted(output: &Output, seed: &Seed, extra: &[&str]) {
    let printed = text(output);
    for secret in [
        &seed.id,
        &seed.key,
        "synthetic_private_project",
        "synthetic_private_row",
        "synthetic_login",
        "synthetic-password",
    ]
    .into_iter()
    .chain(extra.iter().copied())
    {
        assert!(!printed.contains(secret));
    }
    if let Some(old) = &seed.old {
        assert!(!printed.contains(old.access.expose()));
        assert!(!printed.contains(old.refresh.expose()));
    }
}

#[test]
fn real_cli_restore_verify_rebackup_and_second_restore_preserve_credentials_and_reset_sessions() {
    let _serial = CASES.lock().unwrap();
    for version in [1, 2, 3] {
        for compact in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let s = seed(dir.path(), version, compact);
            let before = [
                fs::read(s.data.join("redo.wal")).unwrap(),
                fs::read(s.private.join("redo.wal")).unwrap(),
                fs::read(&s.archive).unwrap(),
            ];
            let root = dir.path().join("восстановлено 界 с пробелами");
            let output = run(
                dir.path(),
                "account-bundle-restore",
                &[
                    Path::new("./копия 界 с пробелами.account-bundle"),
                    Path::new("./восстановлено 界 с пробелами"),
                ],
                Some("50"),
            );
            assert!(output.status.success(), "{}", text(&output));
            assert!(output.stderr.is_empty());
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                format!(
                    "verified root projects=1 private_stores=1 tables=1 rows=1 accounts=1 session_families={} reset_at=50\n",
                    usize::from(version == 3)
                )
            );
            redacted(&output, &s, &[]);
            let verified = run(dir.path(), "account-root-verify", &[&root], None);
            assert!(verified.status.success());
            assert_eq!(verified.stdout, output.stdout);
            redacted(&verified, &s, &[]);
            let source_scope = AccountStore::open(&s.private, &s.id, pool())
                .unwrap()
                .session_storage_scope()
                .unwrap();
            let new = {
                let mut store =
                    AccountStore::open(root.join("private").join(&s.id), &s.id, pool()).unwrap();
                assert_eq!(
                    store
                        .check_password("synthetic_login", b"synthetic-password")
                        .unwrap(),
                    Some(s.user.clone())
                );
                assert_ne!(store.session_storage_scope().unwrap(), source_scope);
                if let Some(old) = &s.old {
                    assert!(store.verify_access(old.access.expose(), 50).is_err());
                    assert!(store.refresh_session(old.refresh.expose(), 50).is_err());
                }
                store
                    .sign_in("synthetic_login", b"synthetic-password", 50)
                    .unwrap()
            };
            let private_path = root.join("private").join(&s.id);
            let private_before = fs::read(private_path.join("redo.wal")).unwrap();
            let copy = dir.path().join("повторная копия.account-bundle");
            let backup = run(dir.path(), "account-root-backup", &[&root, &copy], None);
            assert!(backup.status.success(), "{}", text(&backup));
            redacted(&backup, &s, &[new.access.expose(), new.refresh.expose()]);
            assert_eq!(
                fs::read(private_path.join("redo.wal")).unwrap(),
                private_before
            );
            let mut store = AccountStore::open(&private_path, &s.id, pool()).unwrap();
            assert!(store.verify_access(new.access.expose(), 50).is_ok());
            drop(store);
            let copy_bytes = fs::read(&copy).unwrap();
            let copied = dir.path().join("second");
            let output = run(
                dir.path(),
                "account-bundle-restore",
                &[&copy, &copied],
                Some("50"),
            );
            assert!(output.status.success());
            redacted(&output, &s, &[new.access.expose(), new.refresh.expose()]);
            let mut store =
                AccountStore::open(copied.join("private").join(&s.id), &s.id, pool()).unwrap();
            assert!(store.verify_access(new.access.expose(), 50).is_err());
            assert!(store.refresh_session(new.refresh.expose(), 50).is_err());
            assert_eq!(
                store
                    .check_password("synthetic_login", b"synthetic-password")
                    .unwrap(),
                Some(s.user.clone())
            );
            drop(store);
            for root in [&root, &copied] {
                let registry = ProjectStore::open_existing(root.join("registry")).unwrap();
                assert_eq!(
                    registry
                        .authorize(&s.id, &s.key)
                        .unwrap()
                        .status()
                        .unwrap()
                        .rows,
                    1
                );
            }
            assert_eq!(
                [
                    fs::read(s.data.join("redo.wal")).unwrap(),
                    fs::read(s.private.join("redo.wal")).unwrap(),
                    fs::read(&s.archive).unwrap()
                ],
                before
            );
            assert_eq!(fs::read(copy).unwrap(), copy_bytes);
        }
    }
}

#[test]
fn real_cli_root_backup_captures_later_data_private_state_and_new_projects() {
    let _serial = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let s = seed(dir.path(), 3, false);
    let root = dir.path().join("restored");
    assert!(
        run(
            dir.path(),
            "account-bundle-restore",
            &[&s.archive, &root],
            Some("50")
        )
        .status
        .success()
    );
    let new_project = {
        let mut registry = ProjectStore::open_existing(root.join("registry")).unwrap();
        registry
            .authorize(&s.id, &s.key)
            .unwrap()
            .execute("INSERT INTO t VALUES(2,'synthetic_later_row')", &[])
            .unwrap();
        registry.create("synthetic_later_project").unwrap()
    };
    {
        let mut account =
            AccountStore::open(root.join("private").join(&s.id), &s.id, pool()).unwrap();
        account.set_disabled("synthetic_login", true).unwrap();
        account
            .create_user("synthetic_later_user", b"synthetic-later-password")
            .unwrap();
    }
    let archive = dir.path().join("later.account-bundle");
    let output = run(dir.path(), "account-root-backup", &[&root, &archive], None);
    assert!(output.status.success());
    redacted(
        &output,
        &s,
        &[
            &new_project.api_key,
            &new_project.project.id,
            "synthetic_later_user",
            "synthetic-later-password",
            "synthetic_later_row",
            "synthetic_later_project",
        ],
    );
    let report = emilybase_server::inspect_account_bundle(&archive).unwrap();
    assert_eq!(report.registry.projects.len(), 2);
    assert_eq!(report.private_accounts.len(), 1);
    assert_eq!(report.private_accounts[0].inventory.accounts, 2);
    let target = dir.path().join("later-copy");
    assert!(
        run(
            dir.path(),
            "account-bundle-restore",
            &[&archive, &target],
            Some("0")
        )
        .status
        .success()
    );
    let store = AccountStore::open(target.join("private").join(&s.id), &s.id, pool()).unwrap();
    assert!(
        store
            .check_password("synthetic_login", b"synthetic-password")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .check_password("synthetic_later_user", b"synthetic-later-password")
            .unwrap()
            .is_some()
    );
    drop(store);
    let registry = ProjectStore::open_existing(target.join("registry")).unwrap();
    assert_eq!(
        registry
            .authorize(&s.id, &s.key)
            .unwrap()
            .status()
            .unwrap()
            .rows,
        2
    );
    assert_eq!(
        registry
            .authorize(&new_project.project.id, &new_project.api_key)
            .unwrap()
            .status()
            .unwrap()
            .rows,
        0
    );
    assert!(!target.join("private").join(new_project.project.id).exists());
}

#[test]
fn real_cli_restores_and_rebacks_empty_rosters_at_bounded_clock_extremes() {
    let _serial = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut registry = ProjectStore::open(dir.path().join("registry")).unwrap();
    for (i, now) in ["0", "9223372036854775807"].into_iter().enumerate() {
        if i == 1 {
            registry.create("synthetic_unattached_project").unwrap();
        }
        let archive = dir.path().join(format!("empty-{i}.account-bundle"));
        registry.backup_account_bundle(&mut [], &archive).unwrap();
        let root = dir.path().join(format!("root-{i}"));
        let restored = run(
            dir.path(),
            "account-bundle-restore",
            &[&archive, &root],
            Some(now),
        );
        assert!(restored.status.success());
        assert!(text(&restored).contains("private_stores=0"));
        let verified = run(dir.path(), "account-root-verify", &[&root], None);
        assert_eq!(verified.stdout, restored.stdout);
        assert!(verified.status.success());
        let copy = dir.path().join(format!("copy-{i}.account-bundle"));
        assert!(
            run(dir.path(), "account-root-backup", &[&root, &copy], None)
                .status
                .success()
        );
        assert_eq!(fs::read(copy).unwrap(), fs::read(archive).unwrap());
    }
}

#[test]
fn real_cli_rejects_invalid_reset_times_without_echo_or_filesystem_changes() {
    let _serial = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let s = seed(dir.path(), 1, false);
    for (i, time) in [
        "synthetic_secret_timestamp",
        "",
        "-1",
        " 50",
        "50 ",
        "9223372036854775808",
        "18446744073709551615",
        "1e2",
        "５０",
    ]
    .into_iter()
    .enumerate()
    {
        let root = dir.path().join(format!("refused-{i}"));
        let output = run(
            dir.path(),
            "account-bundle-restore",
            &[&s.archive, &root],
            Some(time),
        );
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(text(&output).contains("trusted reset time"));
        if time.len() > 2 {
            assert!(!text(&output).contains(time));
        }
        redacted(&output, &s, &[]);
        assert!(!root.exists());
    }
    let root = dir.path().join("missing-time");
    assert!(
        !run(
            dir.path(),
            "account-bundle-restore",
            &[&s.archive, &root],
            None
        )
        .status
        .success()
    );
    assert!(!root.exists());
    assert!(!fs::read_dir(dir.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".emilybase-account-restore-")
    }));
}

#[test]
fn real_cli_refuses_existing_targets_corrupt_archives_and_unsafe_roots_without_private_output() {
    let _serial = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let s = seed(dir.path(), 3, false);
    let before = fs::read(&s.archive).unwrap();
    let root = dir.path().join("root");
    assert!(
        run(
            dir.path(),
            "account-bundle-restore",
            &[&s.archive, &root],
            Some("50")
        )
        .status
        .success()
    );
    let manifest = fs::read(root.join("root.json")).unwrap();
    let protected = dir.path().join("protected");
    fs::write(&protected, b"preserve").unwrap();
    let alias = dir.path().join("alias");
    symlink(&protected, &alias).unwrap();
    for target in [&root, &protected, &alias] {
        let output = run(
            dir.path(),
            "account-bundle-restore",
            &[&s.archive, target],
            Some("50"),
        );
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        redacted(&output, &s, &[]);
    }
    assert_eq!(fs::read(protected).unwrap(), b"preserve");
    assert_eq!(fs::read(root.join("root.json")).unwrap(), manifest);
    let damaged = dir.path().join("damaged.account-bundle");
    let mut bytes = before.clone();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(&damaged, bytes).unwrap();
    fs::set_permissions(&damaged, fs::Permissions::from_mode(0o600)).unwrap();
    let target = dir.path().join("unpublished");
    let output = run(
        dir.path(),
        "account-bundle-restore",
        &[&damaged, &target],
        Some("50"),
    );
    assert!(!output.status.success());
    redacted(&output, &s, &[]);
    assert!(!target.exists());
    fs::set_permissions(&s.archive, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        !run(
            dir.path(),
            "account-bundle-restore",
            &[&s.archive, &target],
            Some("50")
        )
        .status
        .success()
    );
    fs::set_permissions(&s.archive, fs::Permissions::from_mode(0o600)).unwrap();
    let root_alias = dir.path().join("root-alias");
    symlink(&root, &root_alias).unwrap();
    let missing = dir.path().join("missing-root");
    for source in [&root_alias, &missing] {
        let output = run(dir.path(), "account-root-verify", &[source], None);
        assert!(!output.status.success());
        redacted(&output, &s, &[]);
        assert!(
            !run(dir.path(), "account-root-backup", &[source, &target], None)
                .status
                .success()
        );
    }
    assert!(!missing.exists());
    assert!(!target.exists());
    let inside = root.join("inside.account-bundle");
    assert!(
        !run(dir.path(), "account-root-backup", &[&root, &inside], None)
            .status
            .success()
    );
    assert!(!inside.exists());
    assert_eq!(fs::read(&s.archive).unwrap(), before);
}

#[test]
fn real_cli_refuses_busy_root_private_and_data_owners_then_retries_after_release() {
    let _serial = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let s = seed(dir.path(), 3, false);
    let root = dir.path().join("root");
    assert!(
        run(
            dir.path(),
            "account-bundle-restore",
            &[&s.archive, &root],
            Some("50")
        )
        .status
        .success()
    );
    let target = dir.path().join("copy.account-bundle");
    let owner = File::open(&root).unwrap();
    owner.lock().unwrap();
    assert!(
        !run(dir.path(), "account-root-verify", &[&root], None)
            .status
            .success()
    );
    assert!(
        !run(dir.path(), "account-root-backup", &[&root, &target], None)
            .status
            .success()
    );
    drop(owner);
    let private = AccountStore::open(root.join("private").join(&s.id), &s.id, pool()).unwrap();
    assert!(
        !run(dir.path(), "account-root-backup", &[&root, &target], None)
            .status
            .success()
    );
    drop(private);
    let data =
        emilybase_transactions::Database::open(root.join("registry").join(&s.id).join("data"))
            .unwrap();
    assert!(
        !run(dir.path(), "account-root-backup", &[&root, &target], None)
            .status
            .success()
    );
    drop(data);
    assert!(!target.exists());
    let output = run(dir.path(), "account-root-backup", &[&root, &target], None);
    assert!(output.status.success());
    redacted(&output, &s, &[]);
}
