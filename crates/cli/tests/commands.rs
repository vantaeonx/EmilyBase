use std::path::Path;
use std::process::{Command, Output};

use emilybase_storage::{MAX_RECORD_SIZE, Pager};

fn invoke(command: &str, path: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg(command)
        .arg(path)
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn real_cli_record_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cli.emily");
    assert!(invoke("init", &path, &[]).status.success());
    let append = invoke("append", &path, &["Привет, EmilyBase"]);
    assert!(append.status.success());
    assert_eq!(append.stdout, b"page=1 slot=0\n");
    let read = invoke("get", &path, &["1", "0"]);
    assert!(read.status.success());
    assert_eq!(
        String::from_utf8(read.stdout).unwrap(),
        "Привет, EmilyBase\n"
    );
    assert!(
        invoke("replace", &path, &["1", "0", "updated"])
            .status
            .success()
    );
    assert_eq!(invoke("get", &path, &["1", "0"]).stdout, b"updated\n");
    assert!(invoke("verify", &path, &[]).status.success());
    assert!(invoke("delete", &path, &["1", "0"]).status.success());
    assert!(!invoke("get", &path, &["1", "0"]).status.success());
    assert!(invoke("info", &path, &[]).status.success());
    assert!(!invoke("init", &path, &[]).status.success());
}

#[test]
fn errors_leave_existing_bytes_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.emily");
    assert!(invoke("init", &path, &[]).status.success());
    let before = std::fs::read(&path).unwrap();
    for arguments in [["0", "0"], ["18446744073709551615", "0"], ["1", "65536"]] {
        assert!(!invoke("get", &path, &arguments).status.success());
    }
    let oversized = "x".repeat(MAX_RECORD_SIZE + 1);
    assert!(!invoke("append", &path, &[&oversized]).status.success());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn subprocess_cannot_write_while_another_owner_holds_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("busy.emily");
    let _owner = Pager::create(&path).unwrap();
    let output = invoke("append", &path, &["must not appear"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("already locked")
    );
}

#[test]
fn full_last_page_allocates_another_page() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("full.emily");
    assert!(invoke("init", &path, &[]).status.success());
    let payload = "x".repeat(MAX_RECORD_SIZE);
    assert!(invoke("append", &path, &[&payload]).status.success());
    let output = invoke("append", &path, &["next page"]);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"page=2 slot=0\n");
    assert!(invoke("verify", &path, &[]).status.success());
}
