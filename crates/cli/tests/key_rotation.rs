use emilybase_auth::{key_file::read_api_key_file, password::PasswordPool};
use emilybase_server::{AccountRoot, initialize_account_root};
use emilybase_transactions::Database;
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

static CASES: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn pool() -> PasswordPool {
    PasswordPool::new(1).unwrap()
}
struct Fixture {
    path: PathBuf,
    id: String,
}
impl Fixture {
    fn new(parent: &Path, compact: bool) -> Self {
        let path = parent.join("новый root 界");
        let report = initialize_account_root(&path, "synthetic-project", pool(), 50).unwrap();
        let id = report.registry.projects[0].id.clone();
        if compact {
            Database::open(path.join("private").join(&id))
                .unwrap()
                .compact()
                .unwrap();
            Database::open(path.join("registry").join(&id).join("data"))
                .unwrap()
                .compact()
                .unwrap();
        }
        Self { path, id }
    }
    fn history(&self) -> [Vec<u8>; 3] {
        [
            fs::read(
                self.path
                    .join("registry")
                    .join(&self.id)
                    .join("project.json"),
            )
            .unwrap(),
            fs::read(
                self.path
                    .join("registry")
                    .join(&self.id)
                    .join("data/redo.wal"),
            )
            .unwrap(),
            fs::read(self.path.join("private").join(&self.id).join("redo.wal")).unwrap(),
        ]
    }
    fn rotate(&self, output: &Path) -> Output {
        rotate(&self.path, &self.id, output)
    }
}
fn rotate(path: &Path, id: &str, target: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg("account-key-rotate")
        .arg(path)
        .arg(id)
        .arg("--output")
        .arg(target)
        .output()
        .unwrap()
}
fn printed(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}
fn ok(out: &Output) -> Value {
    assert!(out.status.success(), "{}", printed(out));
    assert!(out.stderr.is_empty());
    serde_json::from_slice(&out.stdout).unwrap()
}
fn redacted(out: &Output, secrets: &[&str]) {
    for secret in secrets {
        assert!(!printed(out).contains(secret), "secret or path leaked");
    }
}

#[test]
fn actual_cli_bootstraps_an_offline_root_then_new_key_provisions_users_without_secret_output() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact);
        let before = f.history();
        let listed = Command::new(env!("CARGO_BIN_EXE_emilybase"))
            .arg("account-root-projects")
            .arg(&f.path)
            .output()
            .unwrap();
        assert_eq!(
            ok(&listed),
            json!({"projects":[{"id":f.id,"name":"synthetic-project","key_epoch":"1"}]})
        );
        assert_eq!(f.history(), before);
        let output = dir.path().join("служебный ключ 界.key");
        let out = f.rotate(&output);
        assert_eq!(ok(&out), json!({"project":f.id,"key_epoch":"2"}));
        let key = read_api_key_file(&output).unwrap();
        redacted(
            &out,
            &[&key, output.to_str().unwrap(), f.path.to_str().unwrap()],
        );
        assert_eq!(fs::metadata(&output).unwrap().mode() & 0o7777, 0o600);
        assert_eq!(fs::metadata(&output).unwrap().len(), 64);
        assert_eq!(fs::metadata(&output).unwrap().nlink(), 1);
        let after = f.history();
        assert_ne!(after[0], before[0]);
        assert_eq!(after[1..], before[1..]);
        let password = b"synthetic-offline-bootstrap-password";
        let mut child = Command::new(env!("CARGO_BIN_EXE_emilybase"))
            .arg("account-user")
            .arg(&f.path)
            .arg(&f.id)
            .arg("--key-file")
            .arg(&output)
            .args(["create", "new_user"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(password).unwrap();
        let out = child.wait_with_output().unwrap();
        assert_eq!(ok(&out)["user"]["login"], "new_user");
        redacted(&out, &[&key, std::str::from_utf8(password).unwrap()]);
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        let session = root.sign_in(&f.id, &key, "new_user", password, 50).unwrap();
        root.with_access(&f.id, &key, session.access.expose(), 50, |_| ())
            .unwrap();
    }
}

#[test]
fn actual_cli_never_overwrites_files_links_directories_or_internal_root_inventory() {
    let _case = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path(), false);
    let before = f.history();
    let protected = dir.path().join("protected");
    fs::write(&protected, b"synthetic protected target").unwrap();
    let alias = dir.path().join("alias");
    symlink(&protected, &alias).unwrap();
    let folder = dir.path().join("folder");
    fs::create_dir(&folder).unwrap();
    for target in [
        &protected,
        &alias,
        &folder,
        &f.path.join("inside.key"),
        &f.path.join("private").join(&f.id).join("inside.key"),
    ] {
        let out = f.rotate(target);
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        redacted(&out, &[target.to_str().unwrap()]);
    }
    assert_eq!(fs::read(&protected).unwrap(), b"synthetic protected target");
    assert!(
        fs::symlink_metadata(alias)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_dir(folder).unwrap().count(), 0);
    assert_eq!(f.history(), before);
    let target = dir.path().join("unknown.key");
    let out = rotate(&f.path, "../synthetic-invalid-project", &target);
    assert!(!out.status.success());
    redacted(&out, &["synthetic-invalid-project"]);
    assert!(!target.exists());
    assert_eq!(f.history(), before);
}

#[test]
fn current_live_owner_refuses_before_file_creation_and_missing_root_is_not_initialized() {
    let _case = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path(), false);
    let before = f.history();
    let owner = AccountRoot::open(&f.path, pool()).unwrap();
    let target = dir.path().join("new.key");
    let out = f.rotate(&target);
    assert!(!out.status.success());
    assert!(printed(&out).contains("ownership is busy"));
    assert!(!target.exists());
    assert_eq!(f.history(), before);
    drop(owner);
    let missing = dir.path().join("missing-root");
    let out = rotate(&missing, &f.id, &target);
    assert!(!out.status.success());
    assert!(!target.exists());
    assert!(!missing.exists());
    redacted(&out, &[missing.to_str().unwrap(), target.to_str().unwrap()]);
}

#[test]
fn subsequent_rotation_revokes_only_service_key_and_verified_clone_uses_external_new_file() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact);
        let first = dir.path().join("first.key");
        ok(&f.rotate(&first));
        let old = read_api_key_file(&first).unwrap();
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        root.create_user(&f.id, &old, "synthetic_user", b"synthetic-password")
            .unwrap();
        let session = root
            .sign_in(&f.id, &old, "synthetic_user", b"synthetic-password", 50)
            .unwrap();
        drop(root);
        let before = f.history();
        let second = dir.path().join("second.key");
        let out = f.rotate(&second);
        assert_eq!(ok(&out)["key_epoch"], "3");
        let current = read_api_key_file(&second).unwrap();
        redacted(
            &out,
            &[
                &old,
                &current,
                session.access.expose(),
                session.refresh.expose(),
            ],
        );
        assert_eq!(f.history()[1..], before[1..]);
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        assert!(root.list_users(&f.id, &old, None, 1).is_err());
        root.with_access(&f.id, &current, session.access.expose(), 50, |_| ())
            .unwrap();
        drop(root);
        let after = f.history();
        let file = fs::read(&second).unwrap();
        let out = f.rotate(&second);
        assert!(!out.status.success());
        assert_eq!(f.history(), after);
        assert_eq!(fs::read(&second).unwrap(), file);
        let bundle = dir.path().join("copy.bundle");
        emilybase_server::backup_account_bundle_root(&f.path, &bundle, pool()).unwrap();
        let bytes = fs::read(&bundle).unwrap();
        assert!(!bytes.windows(64).any(|w| w == current.as_bytes()));
        let copy = dir.path().join("copy-root");
        emilybase_server::restore_account_bundle(&bundle, &copy, pool(), 50).unwrap();
        let mut root = AccountRoot::open(&copy, pool()).unwrap();
        assert_eq!(
            root.list_users(&f.id, &current, None, 1)
                .unwrap()
                .users
                .len(),
            1
        );
        assert!(
            root.with_access(&f.id, &current, session.access.expose(), 50, |_| ())
                .is_err()
        );
        root.sign_in(&f.id, &current, "synthetic_user", b"synthetic-password", 50)
            .unwrap();
    }
}

#[test]
fn unwritable_stdout_reports_failure_after_durable_activation_without_panicking_or_printing_key() {
    let _case = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path(), false);
    let before = f.history();
    let target = dir.path().join("active.key");
    let out = Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg("account-key-rotate")
        .arg(&f.path)
        .arg(&f.id)
        .arg("--output")
        .arg(&target)
        .stdout(Stdio::from(
            fs::OpenOptions::new()
                .write(true)
                .open("/dev/full")
                .unwrap(),
        ))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!printed(&out).contains("panicked"));
    let key = read_api_key_file(&target).unwrap();
    redacted(&out, &[&key, target.to_str().unwrap()]);
    let mut root = AccountRoot::open(&f.path, pool()).unwrap();
    root.list_users(&f.id, &key, None, 1).unwrap();
    assert_eq!(root.projects().unwrap()[0].key_epoch, 2);
    assert_eq!(f.history()[1..], before[1..]);
}
