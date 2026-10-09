use emilybase_auth::{accounts::IssuedSession, password::PasswordPool};
use emilybase_server::{AccountRoot, initialize_account_root};
use emilybase_transactions::Database;
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

static CASES: std::sync::Mutex<()> = std::sync::Mutex::new(());
const DENY: &[u8] = br#"{"version":1,"select":{"kind":"deny"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}}"#;
const OWN: &[u8] = br#"{"version":1,"select":{"kind":"owner","column":"owner"},"insert":{"kind":"owner","column":"owner"},"update_using":{"kind":"owner","column":"owner"},"update_check":{"kind":"owner","column":"owner"},"delete":{"kind":"owner","column":"owner"}}"#;
const LOGIN: &str = "synthetic_user";
const PASSWORD: &[u8] = b"synthetic-private-password";
fn pool() -> PasswordPool {
    PasswordPool::new(1).unwrap()
}
struct Fixture {
    path: PathBuf,
    id: String,
    key: String,
    key_file: PathBuf,
    session: IssuedSession,
}
impl Fixture {
    fn new(parent: &Path, compact: bool) -> Self {
        let path = parent.join("приватный root 界");
        let report = initialize_account_root(&path, "synthetic-project", pool(), 50).unwrap();
        let id = report.registry.projects[0].id.clone();
        let mut root = AccountRoot::open(&path, pool()).unwrap();
        let key = root.rotate_project_key(&id).unwrap().api_key;
        let user = root.create_user(&id, &key, LOGIN, PASSWORD).unwrap();
        root.execute(
            &id,
            &key,
            "CREATE TABLE t(id INT PRIMARY KEY,owner BYTES,n INT)",
            &[],
        )
        .unwrap();
        root.execute(
            &id,
            &key,
            "INSERT INTO t VALUES(1,$1,10)",
            &[emilybase_catalog::Value::Bytes(user.id.to_vec())],
        )
        .unwrap();
        let session = root.sign_in(&id, &key, LOGIN, PASSWORD, 50).unwrap();
        drop(root);
        let key_file = parent.join("ключ 界 service.key");
        key_file_write(&key_file, format!("{key}\n").as_bytes());
        let f = Self {
            path,
            id,
            key,
            key_file,
            session,
        };
        if compact {
            for path in [f.public(), f.private()] {
                Database::open(path).unwrap().compact().unwrap();
            }
        }
        f
    }
    fn public(&self) -> PathBuf {
        self.path.join("registry").join(&self.id).join("data")
    }
    fn private(&self) -> PathBuf {
        self.path.join("private").join(&self.id)
    }
    fn history(&self) -> [Vec<u8>; 2] {
        [
            fs::read(self.public().join("redo.wal")).unwrap(),
            fs::read(self.private().join("redo.wal")).unwrap(),
        ]
    }
    fn command(&self, action: &[&str]) -> Command {
        command(&self.path, &self.id, &self.key_file, action)
    }
    fn run(&self, action: &[&str], bytes: &[u8]) -> Output {
        finish(self.command(action), bytes)
    }
    fn redacted(&self, out: &Output) {
        let text = printed(out);
        for secret in [
            &self.key,
            &self.id,
            LOGIN,
            std::str::from_utf8(PASSWORD).unwrap(),
            self.session.access.expose(),
            self.session.refresh.expose(),
            "synthetic-project",
            self.path.to_str().unwrap(),
            self.key_file.to_str().unwrap(),
        ] {
            assert!(!text.contains(secret), "private command data leaked");
        }
        assert!(!text.contains("\"kind\""));
    }
}
fn key_file_write(path: &Path, bytes: &[u8]) {
    let mut file = File::options()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}
fn command(path: &Path, id: &str, key_file: &Path, action: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    cmd.arg("account-policy")
        .arg(path)
        .arg(id)
        .arg("--key-file")
        .arg(key_file)
        .args(action);
    cmd
}
fn finish(mut command: Command, bytes: &[u8]) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(bytes);
    wait(child)
}
fn wait(mut child: Child) -> Output {
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("offline policy process exceeded deadline");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.wait_with_output().unwrap()
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
fn refused(out: &Output, message: &str) {
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(printed(out).contains(message), "{}", printed(out));
}
fn blocked_stdin(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            child.try_wait().unwrap().is_none(),
            "child exited before stdin wait"
        );
        let wait_channel =
            fs::read_to_string(format!("/proc/{}/wchan", child.id())).unwrap_or_default();
        if wait_channel.trim() == "anon_pipe_read" {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("child did not reach the actual pipe read");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn real_cli_enable_install_list_retry_and_stale_refusal_preserve_public_rows_and_sessions() {
    let _serial = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact);
        let initial = f.history();
        for action in [&["list"][..], &["install", "t", "--expected", "0"]] {
            let out = f.run(action, DENY);
            refused(&out, "row policy catalog is not enabled");
            f.redacted(&out);
        }
        assert_eq!(f.history(), initial);
        assert_eq!(ok(&f.run(&["enable"], &[])), json!({"private_version":4}));
        let enabled = f.history();
        assert_eq!(enabled[0], initial[0]);
        assert_ne!(enabled[1], initial[1]);
        assert_eq!(ok(&f.run(&["enable"], &[])), json!({"private_version":4}));
        assert_eq!(ok(&f.run(&["list"], &[])), json!({"policies":[]}));
        assert_eq!(f.history(), enabled);
        let out = f.run(&["install", "t", "--expected", "0"], OWN);
        let first = ok(&out)["receipt"].clone();
        f.redacted(&out);
        for field in ["table", "revision", "previous"] {
            assert!(first[field].is_string());
        }
        assert_eq!(first["previous"], "0");
        assert_eq!(first["sha256"].as_str().unwrap().len(), 64);
        assert_eq!(
            first["revision"].as_str().unwrap().parse::<u64>().unwrap(),
            Database::open(f.private()).unwrap().last_transaction()
        );
        let installed = f.history();
        assert_eq!(installed[0], initial[0]);
        for expected in ["0", first["revision"].as_str().unwrap()] {
            assert_eq!(
                ok(&f.run(&["install", "t", "--expected", expected], OWN))["receipt"],
                first
            );
        }
        assert_eq!(
            ok(&f.run(&["list"], &[])),
            json!({"policies":[first.clone()]})
        );
        assert_eq!(f.history(), installed);
        let stale = f.run(&["install", "t", "--expected", "0"], DENY);
        refused(&stale, "row policy revision does not match");
        assert_eq!(f.history(), installed);
        let changed = ok(&f.run(
            &[
                "install",
                "t",
                "--expected",
                first["revision"].as_str().unwrap(),
            ],
            DENY,
        ))["receipt"]
            .clone();
        assert_eq!(changed["previous"], first["revision"]);
        assert_ne!(changed["revision"], first["revision"]);
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        root.with_access(&f.id, &f.key, f.session.access.expose(), 50, |_| ())
            .unwrap();
        assert_eq!(root.row_policy_receipts(&f.id, &f.key).unwrap().len(), 1);
        assert_eq!(
            root.execute(&f.id, &f.key, "SELECT * FROM t", &[])
                .unwrap()
                .results[0]
                .rows
                .len(),
            1
        );
        assert_eq!(f.history()[0], initial[0]);
    }
}

#[test]
fn malformed_revision_table_definition_and_oversized_stdin_never_change_either_wal() {
    let _serial = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path(), false);
    ok(&f.run(&["enable"], &[]));
    let before = f.history();
    for revision in [
        "",
        "00",
        "+0",
        "-1",
        "1e0",
        "1 ",
        "18446744073709551616",
        "synthetic-private-revision",
    ] {
        let out = f.run(&["install", "t", "--expected", revision], OWN);
        refused(&out, "invalid canonical expected policy revision");
        f.redacted(&out);
        if !revision.is_empty() {
            assert!(!printed(&out).contains(revision));
        }
    }
    for table in ["", &"x".repeat(64)] {
        let out = f.run(&["install", table, "--expected", "0"], OWN);
        refused(&out, "invalid policy table name");
    }
    for bytes in [
        b"".to_vec(),
        vec![255],
        b"{\"synthetic-private-field\":1}".to_vec(),
        [DENY, b"{}"].concat(),
        vec![b' '; 16_385],
        vec![b'x'; 1_000_000],
    ] {
        let out = f.run(&["install", "t", "--expected", "0"], &bytes);
        refused(&out, "invalid bounded row policy document");
        f.redacted(&out);
        assert!(!printed(&out).contains("synthetic-private-field"));
    }
    let out = f.run(
        &["install", "t", "--expected", "0"],
        &String::from_utf8(OWN.to_vec())
            .unwrap()
            .replace("\"owner\"", "\"missing\"")
            .into_bytes(),
    );
    assert!(!out.status.success());
    f.redacted(&out);
    let out = f.run(&["install", "t", "--expected", "18446744073709551615"], OWN);
    refused(&out, "row policy revision does not match");
    assert_eq!(f.history(), before);
}

#[test]
fn private_key_file_policy_and_fifo_are_checked_before_root_creation() {
    let _serial = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("absent");
    let id = "0".repeat(32);
    let secret = "a".repeat(64);
    let valid = dir.path().join("synthetic-private-key-path");
    key_file_write(&valid, secret.as_bytes());
    let wide = dir.path().join("wide");
    key_file_write(&wide, secret.as_bytes());
    fs::set_permissions(&wide, fs::Permissions::from_mode(0o640)).unwrap();
    let alias = dir.path().join("alias");
    symlink(&valid, &alias).unwrap();
    let linked = dir.path().join("linked");
    key_file_write(&linked, secret.as_bytes());
    fs::hard_link(&linked, dir.path().join("hard-alias")).unwrap();
    let fifo = dir.path().join("fifo");
    assert!(
        Command::new("mkfifo")
            .args(["--mode=600"])
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    for path in [
        dir.path().join("missing"),
        dir.path().to_path_buf(),
        wide,
        alias,
        linked,
        fifo,
    ] {
        let out = finish(command(&target, &id, &path, &["enable"]), &[]);
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        for value in [&secret, &id, path.to_str().unwrap()] {
            assert!(!printed(&out).contains(value));
        }
        assert!(!target.exists());
    }
    for (i, bytes) in [
        vec![b'A'; 64],
        vec![0; 64],
        vec![255; 64],
        vec![b'a'; 63],
        vec![b'a'; 66],
        format!("{secret}\r\n").into_bytes(),
        format!("{secret}\n\n").into_bytes(),
    ]
    .into_iter()
    .enumerate()
    {
        let path = dir.path().join(format!("invalid-{i}"));
        key_file_write(&path, &bytes);
        let out = finish(command(&target, &id, &path, &["list"]), &[]);
        assert!(!out.status.success());
        assert!(!printed(&out).contains(&secret));
        assert!(!target.exists());
    }
    let out = finish(
        command(&target, "../synthetic-escape", &valid, &["enable"]),
        &[],
    );
    refused(&out, "invalid policy project identity");
    assert!(!printed(&out).contains("synthetic-escape"));
    assert!(!target.exists());
}

#[test]
fn live_owner_refusal_and_wrong_current_service_key_preserve_all_histories() {
    let _serial = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact);
        let before = f.history();
        let owner = AccountRoot::open(&f.path, pool()).unwrap();
        for action in [
            &["enable"][..],
            &["list"],
            &["install", "t", "--expected", "0"],
        ] {
            let out = f.run(action, OWN);
            refused(&out, "ownership is busy");
            f.redacted(&out);
        }
        drop(owner);
        fs::write(&f.key_file, "b".repeat(64)).unwrap();
        for action in [
            &["enable"][..],
            &["list"],
            &["install", "t", "--expected", "0"],
        ] {
            let out = f.run(action, OWN);
            assert!(!out.status.success());
            f.redacted(&out);
            assert!(!printed(&out).contains(&"b".repeat(64)));
        }
        assert_eq!(f.history(), before);
    }
}

#[test]
fn stdin_wait_holds_no_root_and_current_key_rotation_refuses_the_previously_loaded_key() {
    let _serial = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact);
        ok(&f.run(&["enable"], &[]));
        let before = f.history();
        let mut cmd = f.command(&["install", "t", "--expected", "0"]);
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        blocked_stdin(&mut child);
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        let changed = root.rotate_project_key(&f.id).unwrap();
        drop(root);
        fs::write(&f.key_file, &changed.api_key).unwrap();
        child.stdin.take().unwrap().write_all(OWN).unwrap();
        let out = wait(child);
        assert!(!out.status.success());
        f.redacted(&out);
        assert!(!printed(&out).contains(&changed.api_key));
        assert_eq!(f.history(), before);
        assert_eq!(ok(&f.run(&["list"], &[])), json!({"policies":[]}));
        assert!(ok(&f.run(&["install", "t", "--expected", "0"], OWN))["receipt"].is_object());
    }
}

#[test]
fn killed_incomplete_stdin_has_no_commit_and_exact_bounded_document_survives_verified_restore() {
    let _serial = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact);
        ok(&f.run(&["enable"], &[]));
        let before = f.history();
        let mut child = f
            .command(&["install", "t", "--expected", "0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.as_mut().unwrap().write_all(&OWN[..20]).unwrap();
        blocked_stdin(&mut child);
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        assert_eq!(f.history(), before);
        let mut exact = OWN.to_vec();
        exact.resize(16_384, b' ');
        let installed = ok(&f.run(&["install", "t", "--expected", "0"], &exact));
        let receipt = installed["receipt"].clone();
        let out = f.run(&["install", "t", "--expected", "0"], OWN);
        refused(&out, "row policy revision does not match");
        let after = f.history();
        assert_eq!(
            ok(&f.run(&["install", "t", "--expected", "0"], &exact)),
            installed
        );
        assert_eq!(f.history(), after);
        let archive = dir.path().join("copy.bundle");
        emilybase_server::backup_account_bundle_root(&f.path, &archive, pool()).unwrap();
        emilybase_server::inspect_account_bundle(&archive).unwrap();
        let copy = dir.path().join("copy-root");
        emilybase_server::restore_account_bundle(&archive, &copy, pool(), 50).unwrap();
        let out = finish(command(&copy, &f.id, &f.key_file, &["list"]), &[]);
        assert_eq!(ok(&out), json!({"policies":[receipt.clone()]}));
        f.redacted(&out);
        let mut root = AccountRoot::open(&copy, pool()).unwrap();
        assert!(
            root.with_access(&f.id, &f.key, f.session.access.expose(), 50, |_| ())
                .is_err()
        );
        let fresh = root.sign_in(&f.id, &f.key, LOGIN, PASSWORD, 50).unwrap();
        let rows = root
            .user_table(
                &f.id,
                &f.key,
                "t",
                fresh.access.expose(),
                50,
                emilybase_server::UserTableOperation::Page {
                    after: None,
                    limit: 2,
                },
            )
            .unwrap();
        assert!(
            matches!(rows, emilybase_server::UserTableResult::Page { rows, next: None } if rows.len()==1)
        );
        let mut source = AccountRoot::open(&f.path, pool()).unwrap();
        source
            .with_access(&f.id, &f.key, f.session.access.expose(), 50, |_| ())
            .unwrap();
        assert_eq!(f.history(), after);
    }
}

#[test]
fn recreated_table_requires_a_new_identity_and_zero_expectation_without_reusing_old_policy() {
    let _serial = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact);
        fs::set_permissions(&f.key_file, fs::Permissions::from_mode(0o400)).unwrap();
        let key_bytes = fs::read(&f.key_file).unwrap();
        ok(&f.run(&["enable"], &[]));
        let original = ok(&f.run(&["install", "t", "--expected", "0"], OWN))["receipt"].clone();
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        root.execute(
            &f.id,
            &f.key,
            "DROP TABLE t; CREATE TABLE t(id INT PRIMARY KEY,owner BYTES,n INT)",
            &[],
        )
        .unwrap();
        assert!(
            root.user_table(
                &f.id,
                &f.key,
                "t",
                f.session.access.expose(),
                50,
                emilybase_server::UserTableOperation::Get(emilybase_catalog::Key::Integer(1)),
            )
            .is_err()
        );
        drop(root);
        let before = f.history();
        let out = f.run(
            &[
                "install",
                "t",
                "--expected",
                original["revision"].as_str().unwrap(),
            ],
            OWN,
        );
        refused(&out, "row policy revision does not match");
        assert_eq!(f.history(), before);
        let installed = ok(&f.run(&["install", "t", "--expected", "0"], OWN))["receipt"].clone();
        assert_ne!(installed["table"], original["table"]);
        assert_eq!(installed["previous"], "0");
        let out = f.run(&["list"], &[]);
        assert_eq!(ok(&out), json!({"policies":[original, installed]}));
        f.redacted(&out);
        let after = f.history();
        assert_eq!(after[0], before[0]);
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        let result = root
            .user_table(
                &f.id,
                &f.key,
                "t",
                f.session.access.expose(),
                50,
                emilybase_server::UserTableOperation::Get(emilybase_catalog::Key::Integer(1)),
            )
            .unwrap();
        assert!(matches!(
            result,
            emilybase_server::UserTableResult::Row(None)
        ));
        assert_eq!(f.history(), after);
        assert_eq!(fs::read(&f.key_file).unwrap(), key_bytes);
        assert_eq!(
            fs::metadata(&f.key_file).unwrap().permissions().mode() & 0o7777,
            0o400
        );
    }
}

#[test]
fn oversized_open_stream_refuses_without_waiting_for_eof_or_opening_a_missing_root() {
    let _serial = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("must-remain-absent");
    let file = dir.path().join("key");
    key_file_write(&file, "a".repeat(64).as_bytes());
    let mut cmd = command(
        &root,
        &"0".repeat(32),
        &file,
        &["install", "t", "--expected", "0"],
    );
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stream = child.stdin.take().unwrap();
    stream.write_all(&vec![b' '; 16_385]).unwrap();
    let out = wait(child);
    refused(&out, "invalid bounded row policy document");
    assert!(!root.exists());
    // The writer is still open: EOF did not release this command.
    drop(stream);
    assert!(!printed(&out).contains(file.to_str().unwrap()));
    assert!(!printed(&out).contains(&"a".repeat(64)));
}

#[test]
fn policy_enable_reports_actual_v5_and_preserves_open_admission_and_sessions() {
    let _serial = CASES.lock().unwrap();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compacted);
        assert_eq!(ok(&f.run(&["enable"], &[])), json!({"private_version":4}));
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        let closed = root.enable_public_admission_catalog(&f.id, &f.key).unwrap();
        let open = root
            .set_public_admission(&f.id, &f.key, closed.revision, true)
            .unwrap();
        drop(root);
        let before = f.history();
        let out = f.run(&["enable"], &[]);
        assert_eq!(ok(&out), json!({"private_version":5}));
        f.redacted(&out);
        assert_eq!(f.history(), before);
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        assert_eq!(root.public_admission(&f.id, &f.key).unwrap(), open);
        root.with_access(&f.id, &f.key, f.session.access.expose(), 50, |_| ())
            .unwrap();
    }
}
