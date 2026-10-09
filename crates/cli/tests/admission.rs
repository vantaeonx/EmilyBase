use emilybase_auth::{
    accounts::{AccountStore, IssuedSession},
    password::PasswordPool,
};
use emilybase_catalog::Key;
use emilybase_server::{AccountRoot, ProjectStore, UserTableOperation, UserTableResult};
use emilybase_transactions::Database;
use proptest::prelude::*;
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

static CASES: std::sync::Mutex<()> = std::sync::Mutex::new(());
const LOGIN: &str = "synthetic_user";
const PASSWORD: &[u8] = b"synthetic-private-password";
const OWN: &[u8] = br#"{"version":1,"select":{"kind":"owner","column":"owner"},"insert":{"kind":"owner","column":"owner"},"update_using":{"kind":"owner","column":"owner"},"update_check":{"kind":"owner","column":"owner"},"delete":{"kind":"owner","column":"owner"}}"#;
fn pool() -> PasswordPool {
    PasswordPool::new(1).unwrap()
}
struct Fixture {
    path: PathBuf,
    credentials: Vec<(String, String)>,
    key_file: PathBuf,
    session: IssuedSession,
}
impl Fixture {
    fn new(parent: &Path, compact: bool) -> Self {
        let mut registry = ProjectStore::open(parent.join("seed-registry")).unwrap();
        let mut stores = Vec::new();
        let mut credentials = Vec::new();
        for index in 0..2 {
            let created = registry.create("synthetic-project").unwrap();
            let id = created.project.id;
            let key = created.api_key;
            let mut store =
                AccountStore::create(parent.join(format!("seed-{index}")), &id, pool()).unwrap();
            let user = store.create_user(LOGIN, PASSWORD).unwrap();
            store.enable_session_clock(50).unwrap();
            registry.authorize(&id, &key).unwrap().execute(
                "CREATE TABLE t(id INT PRIMARY KEY,owner BYTES,n INT); INSERT INTO t VALUES(1,$1,$2)",
                &[emilybase_catalog::Value::Bytes(user.id.to_vec()),emilybase_catalog::Value::Integer(index)],
            ).unwrap();
            if compact {
                store.compact().unwrap();
                Database::open(parent.join("seed-registry").join(&id).join("data"))
                    .unwrap()
                    .compact()
                    .unwrap();
            }
            stores.push(store);
            credentials.push((id, key));
        }
        let archive = parent.join("seed.bundle");
        registry
            .backup_account_bundle(&mut stores, &archive)
            .unwrap();
        let path = parent.join("приватный root 界");
        emilybase_server::restore_account_bundle(&archive, &path, pool(), 50).unwrap();
        let (id, key) = &credentials[0];
        let mut root = AccountRoot::open(&path, pool()).unwrap();
        let session = root.sign_in(id, key, LOGIN, PASSWORD, 50).unwrap();
        drop(root);
        let key_file = parent.join("служебный ключ 界.key");
        key_file_write(&key_file, format!("{key}\n").as_bytes());
        Self {
            path,
            credentials,
            key_file,
            session,
        }
    }
    fn id(&self) -> &str {
        &self.credentials[0].0
    }
    fn key(&self) -> &str {
        &self.credentials[0].1
    }
    fn private(&self) -> PathBuf {
        self.path.join("private").join(self.id())
    }
    fn history(&self) -> Vec<Vec<u8>> {
        let mut bytes = vec![fs::read(self.path.join("root.json")).unwrap()];
        for (id, _) in &self.credentials {
            bytes.push(fs::read(self.path.join("registry").join(id).join("project.json")).unwrap());
            bytes
                .push(fs::read(self.path.join("registry").join(id).join("data/redo.wal")).unwrap());
            bytes.push(fs::read(self.path.join("private").join(id).join("redo.wal")).unwrap());
        }
        bytes
    }
    fn only_admission_changed(&self, before: &[Vec<u8>]) {
        let after = self.history();
        for (i, (old, new)) in before.iter().zip(&after).enumerate() {
            if i != 3 {
                assert_eq!(old, new, "unrelated project/history changed");
            }
        }
    }
    fn command(&self, action: &[&str]) -> Command {
        command(&self.path, self.id(), &self.key_file, action)
    }
    fn run(&self, action: &[&str]) -> Output {
        wait(
            self.command(action)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        )
    }
    fn policy(&self) {
        let out = Command::new(env!("CARGO_BIN_EXE_emilybase"))
            .arg("account-policy")
            .arg(&self.path)
            .arg(self.id())
            .arg("--key-file")
            .arg(&self.key_file)
            .arg("enable")
            .output()
            .unwrap();
        assert_eq!(ok(&out), json!({"private_version":4}));
        let mut root = AccountRoot::open(&self.path, pool()).unwrap();
        root.install_row_policy(self.id(), self.key(), "t", 0, OWN)
            .unwrap();
    }
    fn redacted(&self, out: &Output) {
        let text = printed(out);
        for secret in [
            self.key(),
            self.id(),
            LOGIN,
            std::str::from_utf8(PASSWORD).unwrap(),
            self.session.access.expose(),
            self.session.refresh.expose(),
            self.path.to_str().unwrap(),
            self.key_file.to_str().unwrap(),
            "synthetic-project",
        ] {
            assert!(!text.contains(secret), "command data leaked");
        }
        for (id, key) in &self.credentials {
            assert!(!text.contains(id));
            assert!(!text.contains(key));
        }
        assert!(!text.contains("\"owner\""));
    }
    fn admission(&self) -> Value {
        ok(&self.run(&["status"]))["admission"].clone()
    }
    fn set(&self, open: bool, revision: &str) -> Output {
        self.run(&[if open { "open" } else { "close" }, "--expected", revision])
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
fn command(path: &Path, id: &str, key: &Path, action: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    command
        .arg("account-admission")
        .arg(path)
        .arg(id)
        .arg("--key-file")
        .arg(key)
        .args(action);
    command
}
fn wait(mut child: Child) -> Output {
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("admission command exceeded deadline");
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

#[test]
fn real_cli_requires_explicit_policy_then_closed_catalog_and_preserves_current_user_on_suspend_resume()
 {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact);
        let initial = f.history();
        for action in [
            &["enable-catalog"][..],
            &["status"],
            &["open", "--expected", "0"],
            &["close", "--expected", "0"],
        ] {
            let out = f.run(action);
            refused(&out, "public admission catalog is not enabled");
            f.redacted(&out);
        }
        assert_eq!(f.history(), initial);
        f.policy();
        let before = f.history();
        let closed = ok(&f.run(&["enable-catalog"]));
        assert_eq!(closed["private_version"], 5);
        assert_eq!(closed["admission"]["enabled"], false);
        assert_eq!(closed["admission"]["previous"], "0");
        let closed_revision = closed["admission"]["revision"].as_str().unwrap();
        assert_eq!(
            closed_revision.parse::<u64>().unwrap(),
            Database::open(f.private()).unwrap().last_transaction()
        );
        let migrated = f.history();
        assert_ne!(migrated[3], before[3]);
        f.only_admission_changed(&before);
        assert_eq!(ok(&f.run(&["enable-catalog"])), closed);
        assert_eq!(ok(&f.run(&["status"])), closed);
        assert_eq!(ok(&f.set(false, "0")), closed);
        assert_eq!(f.history(), migrated);
        let opened = ok(&f.set(true, closed_revision));
        f.redacted(&f.run(&["status"]));
        assert_eq!(opened["admission"]["enabled"], true);
        assert_eq!(opened["admission"]["previous"], closed_revision);
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        assert!(matches!(
            root.public_user_table(
                f.id(),
                "t",
                f.session.access.expose(),
                50,
                UserTableOperation::Get(Key::Integer(1))
            )
            .unwrap(),
            UserTableResult::Row(Some(_))
        ));
        let epoch = root
            .public_user(f.id(), f.session.access.expose(), 50)
            .unwrap()
            .credential_epoch;
        drop(root);
        let open_history = f.history();
        assert_eq!(ok(&f.set(true, closed_revision)), opened);
        assert_eq!(f.history(), open_history);
        let shut = ok(&f.set(false, opened["admission"]["revision"].as_str().unwrap()));
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        assert!(
            root.public_user(f.id(), f.session.access.expose(), 50)
                .is_err()
        );
        root.with_access(f.id(), f.key(), f.session.access.expose(), 50, |user| {
            assert_eq!(user.account().credential_epoch, epoch)
        })
        .unwrap();
        drop(root);
        let shut_history = f.history();
        refused(
            &f.set(true, closed_revision),
            "public admission revision does not match",
        );
        assert_eq!(f.history(), shut_history);
        let reopened = ok(&f.set(true, shut["admission"]["revision"].as_str().unwrap()));
        assert_eq!(reopened["admission"]["enabled"], true);
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        assert_eq!(
            root.public_user(f.id(), f.session.access.expose(), 50)
                .unwrap()
                .credential_epoch,
            epoch
        );
        assert_eq!(root.row_policy_receipts(f.id(), f.key()).unwrap().len(), 1);
        drop(root);
        f.only_admission_changed(&before);
    }
}

#[test]
fn malformed_revisions_and_project_names_refuse_before_key_loading_or_root_selection() {
    let _case = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("absent");
    let missing = dir.path().join("missing-key");
    let id = "a".repeat(32);
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
        for action in ["open", "close"] {
            let out = command(&absent, &id, &missing, &[action, "--expected", revision])
                .output()
                .unwrap();
            refused(&out, "invalid canonical expected admission revision");
            for private in [
                &id,
                absent.to_str().unwrap(),
                missing.to_str().unwrap(),
                "synthetic-private-revision",
            ] {
                assert!(!printed(&out).contains(private));
            }
        }
    }
    for id in [
        "../synthetic-escape",
        "",
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ",
    ] {
        let out = command(&absent, id, &missing, &["status"])
            .output()
            .unwrap();
        refused(&out, "invalid admission project identity");
        assert!(!printed(&out).contains("synthetic-escape"));
    }
    assert!(!absent.exists());
}

#[test]
fn live_owner_wrong_rotated_key_and_cross_project_key_never_change_admission_or_other_histories() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact);
        f.policy();
        let initial = ok(&f.run(&["enable-catalog"]));
        let revision = initial["admission"]["revision"].as_str().unwrap();
        let before = f.history();
        let root = AccountRoot::open(&f.path, pool()).unwrap();
        for action in [
            &["enable-catalog"][..],
            &["status"],
            &["open", "--expected", revision],
            &["close", "--expected", revision],
        ] {
            let out = f.run(action);
            refused(&out, "ownership is busy");
            f.redacted(&out);
        }
        drop(root);
        assert_eq!(f.history(), before);
        let mut root = AccountRoot::open(&f.path, pool()).unwrap();
        let new = root.rotate_project_key(f.id()).unwrap();
        drop(root);
        let after_rotation = f.history();
        for secret in [f.key(), &f.credentials[1].1, &"b".repeat(64)] {
            fs::write(&f.key_file, secret).unwrap();
            for action in [
                &["enable-catalog"][..],
                &["status"],
                &["open", "--expected", revision],
                &["close", "--expected", revision],
            ] {
                let out = f.run(action);
                assert!(!out.status.success());
                assert!(out.stdout.is_empty());
                f.redacted(&out);
                assert!(!printed(&out).contains(secret));
            }
            assert_eq!(f.history(), after_rotation);
        }
        fs::write(&f.key_file, &new.api_key).unwrap();
        assert_eq!(f.admission(), initial["admission"]);
        let out = f.set(true, revision);
        assert_eq!(ok(&out)["admission"]["enabled"], true);
        assert!(!printed(&out).contains(&new.api_key));
        for id in [
            &f.credentials[1].0,
            &"00000000000000000000000000000000".to_owned(),
        ] {
            let out = command(&f.path, id, &f.key_file, &["status"])
                .output()
                .unwrap();
            assert!(!out.status.success());
            f.redacted(&out);
        }
    }
}

#[test]
fn key_file_modes_aliases_fifos_and_malformed_bytes_refuse_without_creating_a_root() {
    let _case = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("absent");
    let id = "a".repeat(32);
    let secret = "b".repeat(64);
    let valid = dir.path().join("valid");
    key_file_write(&valid, secret.as_bytes());
    let wide = dir.path().join("wide");
    key_file_write(&wide, secret.as_bytes());
    fs::set_permissions(&wide, fs::Permissions::from_mode(0o644)).unwrap();
    let alias = dir.path().join("alias");
    symlink(&valid, &alias).unwrap();
    let hard = dir.path().join("hard");
    key_file_write(&hard, secret.as_bytes());
    fs::hard_link(&hard, dir.path().join("hard-other")).unwrap();
    let fifo = dir.path().join("fifo");
    assert!(
        Command::new("mkfifo")
            .arg("--mode=600")
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
        hard,
        fifo,
    ] {
        let out = wait(
            command(&absent, &id, &path, &["enable-catalog"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        for private in [
            &id,
            &secret,
            path.to_str().unwrap(),
            absent.to_str().unwrap(),
        ] {
            assert!(!printed(&out).contains(private));
        }
        assert!(!absent.exists());
    }
    for (i, bytes) in [
        vec![255; 64],
        vec![b'A'; 64],
        vec![b'b'; 63],
        vec![b'b'; 66],
        format!("{secret}\r\n").into_bytes(),
        format!("{secret}\n\n").into_bytes(),
    ]
    .into_iter()
    .enumerate()
    {
        let path = dir.path().join(format!("invalid-{i}"));
        key_file_write(&path, &bytes);
        let out = command(&absent, &id, &path, &["status"]).output().unwrap();
        assert!(!out.status.success());
        assert!(!printed(&out).contains(&secret));
        assert!(!absent.exists());
    }
}

#[test]
fn terminal_output_failure_after_native_commit_requires_inspection_and_accepts_only_exact_retry() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact);
        f.policy();
        let closed = ok(&f.run(&["enable-catalog"]));
        let revision = closed["admission"]["revision"].as_str().unwrap();
        let before = f.history();
        let out = f
            .command(&["open", "--expected", revision])
            .stdout(File::options().write(true).open("/dev/full").unwrap())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        refused(
            &out,
            "admission output unavailable; inspect current state before retry",
        );
        f.redacted(&out);
        let actual = f.admission();
        assert_eq!(actual["enabled"], true);
        assert_eq!(actual["previous"], revision);
        assert_ne!(f.history()[3], before[3]);
        f.only_admission_changed(&before);
        let committed = f.history();
        assert_eq!(ok(&f.set(true, revision))["admission"], actual);
        assert_eq!(f.history(), committed);
        refused(
            &f.set(false, revision),
            "public admission revision does not match",
        );
        assert_eq!(f.history(), committed);
    }
}

#[test]
fn verified_nonempty_copy_stays_closed_until_cli_reopens_and_source_credentials_never_revive() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = Fixture::new(dir.path(), compact);
        f.policy();
        let closed = ok(&f.run(&["enable-catalog"]));
        let opened = ok(&f.set(true, closed["admission"]["revision"].as_str().unwrap()));
        let before = f.history();
        let archive = dir.path().join("copy.bundle");
        emilybase_server::backup_account_bundle_root(&f.path, &archive, pool()).unwrap();
        let copy = dir.path().join("copy-root");
        emilybase_server::restore_account_bundle(&archive, &copy, pool(), 50).unwrap();
        let current = ok(&command(&copy, f.id(), &f.key_file, &["status"])
            .output()
            .unwrap());
        assert_eq!(current["admission"]["enabled"], false);
        assert_eq!(
            current["admission"]["previous"],
            opened["admission"]["revision"]
        );
        let out = command(
            &copy,
            f.id(),
            &f.key_file,
            &[
                "open",
                "--expected",
                current["admission"]["revision"].as_str().unwrap(),
            ],
        )
        .output()
        .unwrap();
        assert_eq!(ok(&out)["admission"]["enabled"], true);
        f.redacted(&out);
        let mut root = AccountRoot::open(&copy, pool()).unwrap();
        assert!(
            root.public_user(f.id(), f.session.access.expose(), 50)
                .is_err()
        );
        let fresh = root.public_sign_in(f.id(), LOGIN, PASSWORD, 50).unwrap();
        assert!(matches!(
            root.public_user_table(
                f.id(),
                "t",
                fresh.access.expose(),
                50,
                UserTableOperation::Get(Key::Integer(1))
            )
            .unwrap(),
            UserTableResult::Row(Some(_))
        ));
        drop(root);
        let mut source = AccountRoot::open(&f.path, pool()).unwrap();
        source
            .public_user(f.id(), f.session.access.expose(), 50)
            .unwrap();
        drop(source);
        assert_eq!(f.history(), before);
    }
}

#[test]
fn generated_real_cli_admission_histories_match_independent_current_previous_revision_model() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
            cases: 8,
            max_shrink_iters: 0,
            ..ProptestConfig::default()
        });
        runner.run(&prop::collection::vec((any::<bool>(),0u8..3),1..13),|steps|{
            let dir=tempfile::tempdir().unwrap();let f=Fixture::new(dir.path(),compact);f.policy();let initial=ok(&f.run(&["enable-catalog"]));let baseline=f.history();
            let mut enabled=false;let mut revision=initial["admission"]["revision"].as_str().unwrap().parse::<u64>().unwrap();let mut previous=0;
            for (next,which) in steps {
                let expected=match which {0=>revision,1=>previous,_=>u64::MAX};let before=f.history();let out=f.set(next,&expected.to_string());let unchanged=next==enabled&&(expected==revision||expected==previous);let changes=next!=enabled&&expected==revision;
                if unchanged||changes {
                    let value=ok(&out)["admission"].clone();
                    if changes {previous=revision;revision+=1;enabled=next;}
                    prop_assert_eq!(value,json!({"enabled":enabled,"revision":revision.to_string(),"previous":previous.to_string()}));
                    if unchanged {prop_assert_eq!(f.history(),before);}else{prop_assert_ne!(&f.history()[3],&before[3]);}
                }else{refused(&out,"public admission revision does not match");prop_assert_eq!(f.history(),before);}
                f.redacted(&out);prop_assert_eq!(f.admission(),json!({"enabled":enabled,"revision":revision.to_string(),"previous":previous.to_string()}));f.only_admission_changed(&baseline);
            }
            Ok(())
        }).unwrap();
    }
}
