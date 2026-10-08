use emilybase_auth::password::PasswordPool;
use emilybase_server::{AccountRoot, capture_account_bundle_root, inspect_account_bundle_root};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

static CASES: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn pool() -> PasswordPool {
    PasswordPool::new(1).unwrap()
}
fn run(parent: &Path, target: &Path, name: &str, now: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .current_dir(parent)
        .arg("account-root-init")
        .arg(target)
        .arg("--name")
        .arg(name)
        .arg("--reset-at")
        .arg(now)
        .output()
        .unwrap()
}
fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}
#[test]
fn actual_cli_initializes_and_verifies_one_empty_private_root_without_secret_or_identity_output() {
    let _case = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("новый root 界");
    let out = run(dir.path(), &target, "synthetic-name", "50");
    assert!(out.status.success(), "{}", text(&out));
    assert!(out.stderr.is_empty());
    let printed = text(&out);
    assert!(printed.contains("verified root projects=1 private_stores=1 tables=0 rows=0 accounts=0 session_families=0 reset_at=50"));
    let report = inspect_account_bundle_root(&target, pool()).unwrap();
    let id = &report.registry.projects[0].id;
    assert!(!printed.contains(id));
    assert!(!printed.contains("synthetic-name"));
    assert!(!printed.contains(target.to_str().unwrap()));
    let before = capture_account_bundle_root(&target, pool()).unwrap();
    let verify = Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg("account-root-verify")
        .arg(&target)
        .output()
        .unwrap();
    assert!(verify.status.success());
    assert_eq!(verify.stdout, out.stdout);
    let repeat = run(dir.path(), &target, "synthetic-other", "0");
    assert!(!repeat.status.success());
    assert!(!text(&repeat).contains(target.to_str().unwrap()));
    assert_eq!(
        capture_account_bundle_root(&target, pool()).unwrap(),
        before
    );
    let mut service = AccountRoot::open(&target, pool()).unwrap();
    let key = service.rotate_project_key(id).unwrap();
    service
        .create_user(id, &key.api_key, "synthetic_user", b"synthetic-password")
        .unwrap();
    service
        .sign_in(
            id,
            &key.api_key,
            "synthetic_user",
            b"synthetic-password",
            50,
        )
        .unwrap();
}
#[test]
fn invalid_cli_time_and_names_refuse_before_staging_without_echo_and_protected_paths_remain() {
    let _case = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("new-root");
    for now in [
        "-1",
        "synthetic-private-time",
        "9223372036854775808",
        "18446744073709551616",
        "",
    ] {
        let out = run(dir.path(), &target, "synthetic-name", now);
        assert!(!out.status.success());
        assert!(!target.exists());
        if !now.is_empty() {
            assert!(!text(&out).contains(now));
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    for name in ["", " ", "synthetic\nprivate-name", &"x".repeat(129)] {
        let out = run(dir.path(), &target, name, "50");
        assert!(!out.status.success());
        assert!(!target.exists());
        if !name.trim().is_empty() {
            assert!(!text(&out).contains(name));
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    fs::write(&target, b"synthetic protected bytes").unwrap();
    let out = run(dir.path(), &target, "synthetic-name", "50");
    assert!(!out.status.success());
    assert_eq!(fs::read(&target).unwrap(), b"synthetic protected bytes");
    assert!(!text(&out).contains(target.to_str().unwrap()));
}
