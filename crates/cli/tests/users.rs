use emilybase_auth::{accounts::IssuedSession, password::PasswordPool};
use emilybase_server::{AccountRoot, initialize_account_root};
use emilybase_transactions::Database;
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

static CASES: std::sync::Mutex<()> = std::sync::Mutex::new(());
const PASSWORD: &[u8] = b"synthetic-existing-password";
const NEXT: &[u8] = b"synthetic-new-password\n";
const DENY: &[u8] = br#"{"version":1,"select":{"kind":"deny"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}}"#;
fn pool() -> PasswordPool {
    PasswordPool::new(1).unwrap()
}
struct Fixture {
    path: PathBuf,
    id: String,
    key: String,
    key_file: PathBuf,
    old: IssuedSession,
}
impl Fixture {
    fn new(parent: &Path, compact: bool, policies: bool) -> Self {
        let path = parent.join("учётные записи 界");
        let report = initialize_account_root(&path, "synthetic-project", pool(), 50).unwrap();
        let id = report.registry.projects[0].id.clone();
        let mut root = AccountRoot::open(&path, pool()).unwrap();
        let key = root.rotate_project_key(&id).unwrap().api_key;
        root.execute(
            &id,
            &key,
            "CREATE TABLE t(id INT PRIMARY KEY,n INT); INSERT INTO t VALUES(1,10)",
            &[],
        )
        .unwrap();
        root.create_user(&id, &key, "existing", PASSWORD).unwrap();
        let old = root.sign_in(&id, &key, "existing", PASSWORD, 50).unwrap();
        if policies {
            root.enable_row_policy_catalog(&id, &key).unwrap();
            root.install_row_policy(&id, &key, "t", 0, DENY).unwrap();
        }
        drop(root);
        let key_file = parent.join("synthetic-private-service.key");
        let mut file = File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&key_file)
            .unwrap();
        file.write_all(key.as_bytes()).unwrap();
        file.sync_all().unwrap();
        let f = Self {
            path,
            id,
            key,
            key_file,
            old,
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
    fn histories(&self) -> [Vec<u8>; 2] {
        [
            fs::read(self.public().join("redo.wal")).unwrap(),
            fs::read(self.private().join("redo.wal")).unwrap(),
        ]
    }
    fn command(&self, args: &[&str]) -> Command {
        command(&self.path, &self.id, &self.key_file, args)
    }
    fn run(&self, args: &[&str], bytes: &[u8]) -> Output {
        finish(self.command(args), bytes)
    }
    fn redacted(&self, out: &Output) {
        for secret in [
            &self.id,
            &self.key,
            self.old.access.expose(),
            self.old.refresh.expose(),
            self.path.to_str().unwrap(),
            self.key_file.to_str().unwrap(),
            std::str::from_utf8(PASSWORD).unwrap(),
            std::str::from_utf8(NEXT).unwrap(),
        ] {
            assert!(
                !printed(out).contains(secret),
                "private user administration value leaked"
            );
        }
        for field in [
            "verifier",
            "salt",
            "access_token",
            "refresh_token",
            "password",
        ] {
            assert!(!String::from_utf8_lossy(&out.stdout).contains(&format!("\"{field}\"")));
        }
    }
}
fn command(path: &Path, id: &str, file: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    cmd.arg("account-user")
        .arg(path)
        .arg(id)
        .arg("--key-file")
        .arg(file)
        .args(args);
    cmd
}
fn finish(mut cmd: Command, bytes: &[u8]) -> Output {
    let mut child = cmd
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
            panic!("user CLI exceeded deadline");
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
fn refused(out: &Output, code: &str) {
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(printed(out).contains(code), "{}", printed(out));
}
fn blocked(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(child.try_wait().unwrap().is_none());
        if fs::read_to_string(format!("/proc/{}/wchan", child.id()))
            .unwrap_or_default()
            .trim()
            == "anon_pipe_read"
        {
            return;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("password stream did not reach pipe wait");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn actual_cli_provisions_exact_password_without_issuing_session_or_changing_public_history() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        for policies in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let f = Fixture::new(dir.path(), compact, policies);
            let before = f.histories();
            let out = f.run(&["create", "created"], NEXT);
            let user = ok(&out)["user"].clone();
            f.redacted(&out);
            assert_eq!(user["login"], "created");
            assert_eq!(user["credential_epoch"], "1");
            assert_eq!(user["disabled"], false);
            let id = user["id"].as_str().unwrap();
            assert_eq!(id.len(), 32);
            assert!(
                id.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            );
            assert_eq!(user.as_object().unwrap().len(), 4);
            let after = f.histories();
            assert_eq!(after[0], before[0]);
            assert_ne!(after[1], before[1]);
            let mut root = AccountRoot::open(&f.path, pool()).unwrap();
            root.with_access(&f.id, &f.key, f.old.access.expose(), 50, |_| ())
                .unwrap();
            assert!(
                root.sign_in(&f.id, &f.key, "created", &NEXT[..NEXT.len() - 1], 50)
                    .is_err()
            );
            let issued = root.sign_in(&f.id, &f.key, "created", NEXT, 50).unwrap();
            root.with_access(&f.id, &f.key, issued.access.expose(), 50, |_| ())
                .unwrap();
            if policies {
                assert_eq!(root.row_policy_receipts(&f.id, &f.key).unwrap().len(), 1);
            }
            drop(root);
            let current = f.histories();
            let out = f.run(&["create", "created"], NEXT);
            refused(&out, "account login already exists");
            f.redacted(&out);
            assert_eq!(f.histories(), current);
        }
    }
}

#[test]
fn disable_and_enable_revoke_old_tokens_and_idempotent_repeats_do_not_commit() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact, true);
        let initial = f.histories();
        let out = f.run(&["disable", "existing"], &[]);
        let disabled = ok(&out)["user"].clone();
        f.redacted(&out);
        assert_eq!(disabled["disabled"], true);
        assert_eq!(disabled["credential_epoch"], "2");
        let after = f.histories();
        assert_eq!(after[0], initial[0]);
        assert_ne!(after[1], initial[1]);
        assert_eq!(ok(&f.run(&["disable", "existing"], &[]))["user"], disabled);
        assert_eq!(f.histories(), after);
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        assert!(
            root.with_access(&f.id, &f.key, f.old.access.expose(), 50, |_| ())
                .is_err()
        );
        assert!(
            root.refresh_session(&f.id, &f.key, f.old.refresh.expose(), 50)
                .is_err()
        );
        assert!(
            root.sign_in(&f.id, &f.key, "existing", PASSWORD, 50)
                .is_err()
        );
        drop(root);
        let out = f.run(&["enable", "existing"], &[]);
        let enabled = ok(&out)["user"].clone();
        f.redacted(&out);
        assert_eq!(enabled["disabled"], false);
        assert_eq!(enabled["credential_epoch"], "3");
        assert_eq!(enabled["id"], disabled["id"]);
        let after = f.histories();
        assert_eq!(ok(&f.run(&["enable", "existing"], &[]))["user"], enabled);
        assert_eq!(f.histories(), after);
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        assert!(
            root.with_access(&f.id, &f.key, f.old.access.expose(), 50, |_| ())
                .is_err()
        );
        let fresh = root
            .sign_in(&f.id, &f.key, "existing", PASSWORD, 50)
            .unwrap();
        root.with_access(&f.id, &f.key, fresh.access.expose(), 50, |_| ())
            .unwrap();
        assert_eq!(f.histories()[0], initial[0]);
    }
}

#[test]
fn metadata_pages_match_independent_sorted_logins_and_keep_both_wals_unchanged() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact, false);
        let names = ["z-last", "a_first", "middle", "a.second"];
        for name in names {
            ok(&f.run(&["create", name], NEXT));
        }
        ok(&f.run(&["disable", "middle"], &[]));
        let model: std::collections::BTreeSet<_> = names.into_iter().chain(["existing"]).collect();
        let before = f.histories();
        for limit in [1, 2, 3, 128] {
            let mut result = Vec::new();
            let mut after: Option<String> = None;
            loop {
                let limit = limit.to_string();
                let mut args = vec!["list", "--limit", &limit];
                if let Some(after) = &after {
                    args.extend(["--after", after]);
                }
                let out = f.run(&args, &[]);
                let page = ok(&out);
                f.redacted(&out);
                let users = page["users"].as_array().unwrap();
                for user in users {
                    let login = user["login"].as_str().unwrap();
                    result.push(login.to_string());
                    assert_eq!(user["disabled"], login == "middle");
                    assert!(user["credential_epoch"].is_string());
                }
                if let Some(next) = page["next_after"].as_str() {
                    assert_eq!(users.last().unwrap()["login"], next);
                    after = Some(next.into());
                } else {
                    break;
                }
                assert!(result.len() <= model.len());
            }
            assert_eq!(
                result,
                model.iter().map(|s| s.to_string()).collect::<Vec<_>>()
            );
        }
        let page = ok(&f.run(&["list", "--after", "b_missing", "--limit", "128"], &[]));
        assert_eq!(page["users"].as_array().unwrap().len(), 3);
        assert_eq!(
            ok(&f.run(&["list", "--after", "zzzz", "--limit", "1"], &[])),
            json!({"users":[],"next_after":null})
        );
        assert_eq!(f.histories(), before);
    }
}

#[test]
fn invalid_password_login_cursor_and_limits_refuse_without_committing_or_echoing_secrets() {
    let _case = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path(), false, true);
    let before = f.histories();
    for bytes in [vec![], vec![42; 1025], vec![42; 100_000]] {
        let out = f.run(&["create", "new_user"], &bytes);
        refused(&out, "password must contain 1..1024 bytes");
        f.redacted(&out);
    }
    for login in [
        "",
        "UPPER",
        " spaces ",
        "../synthetic-login",
        "a@b",
        "&".repeat(65).as_str(),
    ] {
        for action in ["create", "disable", "enable"] {
            let out = f.run(&[action, login], NEXT);
            refused(&out, "invalid canonical account login");
            f.redacted(&out);
            if !login.is_empty() {
                assert!(!printed(&out).contains(login));
            }
        }
    }
    for limit in ["0", "129", "18446744073709551615"] {
        let out = f.run(&["list", "--limit", limit], &[]);
        refused(&out, "invalid bounded account page limit");
        f.redacted(&out);
    }
    let out = f.run(&["list", "--after", "../synthetic-cursor"], &[]);
    refused(&out, "invalid canonical account login");
    f.redacted(&out);
    for action in ["enable", "disable"] {
        let out = f.run(&[action, "absent"], &[]);
        refused(&out, "credential check failed");
        f.redacted(&out);
    }
    assert_eq!(f.histories(), before);
}

#[test]
fn live_owner_and_wrong_key_refuse_all_commands_and_missing_roots_are_never_created() {
    let _case = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path(), false, false);
    let before = f.histories();
    let owner = AccountRoot::open(&f.path, pool()).unwrap();
    for args in [
        &["list"][..],
        &["create", "new_user"],
        &["disable", "existing"],
        &["enable", "existing"],
    ] {
        let out = f.run(args, NEXT);
        refused(&out, "ownership is busy");
        f.redacted(&out);
    }
    drop(owner);
    fs::write(&f.key_file, "c".repeat(64)).unwrap();
    for args in [
        &["list"][..],
        &["create", "new_user"],
        &["disable", "existing"],
        &["enable", "existing"],
    ] {
        let out = f.run(args, NEXT);
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        f.redacted(&out);
        assert!(!printed(&out).contains(&"c".repeat(64)));
    }
    assert_eq!(f.histories(), before);
    let missing = dir.path().join("absent-root");
    let out = finish(command(&missing, &f.id, &f.key_file, &["list"]), &[]);
    assert!(!out.status.success());
    assert!(!missing.exists());
}

#[test]
fn password_wait_holds_no_root_current_key_rotation_refuses_loaded_key_and_kill_is_readonly() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact, true);
        let before = f.histories();
        let mut child = f
            .command(&["create", "new_user"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        blocked(&mut child);
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        let key = root.rotate_project_key(&f.id).unwrap().api_key;
        drop(root);
        fs::write(&f.key_file, &key).unwrap();
        child.stdin.take().unwrap().write_all(NEXT).unwrap();
        let out = wait(child);
        assert!(!out.status.success());
        f.redacted(&out);
        assert!(!printed(&out).contains(&key));
        assert_eq!(f.histories(), before);
        let mut child = f
            .command(&["create", "new_user"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.as_mut().unwrap().write_all(&NEXT[..5]).unwrap();
        blocked(&mut child);
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        assert_eq!(f.histories(), before);
        let out = f.run(&["create", "new_user"], NEXT);
        assert_eq!(ok(&out)["user"]["login"], "new_user");
        assert!(!printed(&out).contains(&key));
    }
}

#[test]
fn maximum_binary_password_and_disabled_state_survive_nonempty_verified_clone() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact, true);
        let secret: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
        let out = f.run(&["create", "binary"], &secret);
        let user = ok(&out)["user"].clone();
        f.redacted(&out);
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        let session = root.sign_in(&f.id, &f.key, "binary", &secret, 50).unwrap();
        drop(root);
        ok(&f.run(&["disable", "existing"], &[]));
        let before = f.histories();
        let archive = dir.path().join("users.bundle");
        emilybase_server::backup_account_bundle_root(&f.path, &archive, pool()).unwrap();
        emilybase_server::inspect_account_bundle(&archive).unwrap();
        let copy = dir.path().join("copy-root");
        emilybase_server::restore_account_bundle(&archive, &copy, pool(), 50).unwrap();
        let out = finish(command(&copy, &f.id, &f.key_file, &["list"]), &[]);
        let page = ok(&out);
        f.redacted(&out);
        assert_eq!(page["users"][0], user);
        assert_eq!(page["users"][1]["disabled"], true);
        assert_eq!(page["users"][1]["credential_epoch"], "2");
        let mut root = AccountRoot::open(&copy, pool()).unwrap();
        assert!(
            root.with_access(&f.id, &f.key, session.access.expose(), 50, |_| ())
                .is_err()
        );
        assert!(
            root.sign_in(&f.id, &f.key, "existing", PASSWORD, 50)
                .is_err()
        );
        let fresh = root.sign_in(&f.id, &f.key, "binary", &secret, 50).unwrap();
        root.with_access(&f.id, &f.key, fresh.access.expose(), 50, |_| ())
            .unwrap();
        assert_eq!(root.row_policy_receipts(&f.id, &f.key).unwrap().len(), 1);
        drop(root);
        let mut source = AccountRoot::open(&f.path, pool()).unwrap();
        source
            .with_access(&f.id, &f.key, session.access.expose(), 50, |_| ())
            .unwrap();
        assert_eq!(f.histories(), before);
    }
}

#[test]
fn oversized_password_pipe_refuses_before_eof_and_before_root_acquisition() {
    let _case = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path(), false, false);
    let before = f.histories();
    let mut child = f
        .command(&["create", "new_user"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stream = child.stdin.take().unwrap();
    stream.write_all(&vec![42; 1025]).unwrap();
    let out = wait(child);
    refused(&out, "password must contain 1..1024 bytes");
    drop(stream);
    f.redacted(&out);
    assert_eq!(f.histories(), before);
}

#[test]
fn terminal_input_refuses_before_a_password_can_be_echoed() {
    let _case = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path(), false, false);
    let before = f.histories();
    // Linux test fixture supplies a real pseudoterminal, not an asserted flag.
    let script = r#"
import os,pty,subprocess,sys
master,slave=pty.openpty()
try:
    result=subprocess.run(sys.argv[1:],stdin=slave,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=5)
    sys.stdout.buffer.write(result.stdout)
    sys.stderr.buffer.write(result.stderr)
    sys.exit(result.returncode)
finally:
    os.close(slave)
    os.close(master)
"#;
    let out = Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_emilybase"))
        .arg("account-user")
        .arg(&f.path)
        .arg(&f.id)
        .arg("--key-file")
        .arg(&f.key_file)
        .args(["create", "new_user"])
        .output()
        .unwrap();
    refused(&out, "password requires redirected stdin");
    f.redacted(&out);
    assert_eq!(f.histories(), before);
}

#[test]
fn competing_cli_provisions_have_one_durable_login_and_never_replace_its_password() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact, false);
        let before = f.histories();
        let prior = Database::open(f.private()).unwrap().last_transaction();
        let mut first = f
            .command(&["create", "competing"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut second = f
            .command(&["create", "competing"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        blocked(&mut first);
        blocked(&mut second);
        let a = b"synthetic-concurrent-first";
        let b = b"synthetic-concurrent-second";
        first.stdin.take().unwrap().write_all(a).unwrap();
        second.stdin.take().unwrap().write_all(b).unwrap();
        let first = wait(first);
        let second = wait(second);
        assert_eq!(
            usize::from(first.status.success()) + usize::from(second.status.success()),
            1
        );
        for out in [&first, &second] {
            f.redacted(out);
            for secret in [a.as_slice(), b.as_slice()] {
                assert!(!printed(out).contains(std::str::from_utf8(secret).unwrap()));
            }
        }
        let (winner, loser) = if first.status.success() {
            (a.as_slice(), b.as_slice())
        } else {
            (b.as_slice(), a.as_slice())
        };
        assert_eq!(
            Database::open(f.private()).unwrap().last_transaction(),
            prior + 1
        );
        assert_eq!(f.histories()[0], before[0]);
        let current = f.histories();
        let out = f.run(&["create", "competing"], loser);
        refused(&out, "account login already exists");
        assert_eq!(f.histories(), current);
        let listed = ok(&f.run(&["list"], &[]));
        assert_eq!(listed["users"].as_array().unwrap().len(), 2);
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        assert!(root.sign_in(&f.id, &f.key, "competing", loser, 50).is_err());
        let issued = root
            .sign_in(&f.id, &f.key, "competing", winner, 50)
            .unwrap();
        root.with_access(&f.id, &f.key, issued.access.expose(), 50, |_| ())
            .unwrap();
    }
}
