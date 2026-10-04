use std::path::Path;
use std::process::{Command, Output};

use emilybase_storage::{MAX_RECORD_SIZE, Pager};

// Keep ownership checks outside another case's fork/exec inheritance window.
static CASES: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    let _serial = CASES.lock().unwrap();
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
    let _serial = CASES.lock().unwrap();
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
    let _serial = CASES.lock().unwrap();
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
    let _serial = CASES.lock().unwrap();
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

#[test]
fn raw_cli_relative_unicode_path_is_private_and_no_clobber() {
    use std::os::unix::fs::MetadataExt;
    let _serial = CASES.lock().unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let parent = temporary.path().join("страницы 界 с пробелами");
    std::fs::create_dir(&parent).unwrap();
    let path = parent.join("данные.emily");
    let run = |action: &str, args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_emilybase"))
            .current_dir(&parent)
            .arg(action)
            .arg("./данные.emily")
            .args(args)
            .output()
            .unwrap()
    };
    assert!(run("init", &[]).status.success());
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    let record = "synthetic private CLI record";
    assert!(run("append", &[record]).status.success());
    assert_eq!(
        run("get", &["1", "0"]).stdout,
        format!("{record}\n").as_bytes()
    );
    let original = std::fs::read(&path).unwrap();
    for action in ["init", "db-init"] {
        let denied = run(action, &[]);
        assert!(!denied.status.success());
        let error = String::from_utf8_lossy(&denied.stderr);
        assert!(!error.contains(record));
        assert!(!error.contains("данные"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 1);
    }
    assert!(run("verify", &[]).status.success());
}

#[test]
fn raw_cli_refuses_link_aliases_without_touching_source_or_disclosing_data() {
    let _serial = CASES.lock().unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("private-source.emily");
    assert!(invoke("init", &source, &[]).status.success());
    let record = "synthetic private alias record";
    assert!(invoke("append", &source, &[record]).status.success());
    let bytes = std::fs::read(&source).unwrap();
    for symbolic in [false, true] {
        let alias = temporary.path().join("private-alias.emily");
        if symbolic {
            std::os::unix::fs::symlink(&source, &alias).unwrap();
        } else {
            std::fs::hard_link(&source, &alias).unwrap();
        }
        for (action, args) in [
            ("init", Vec::new()),
            ("get", vec!["1", "0"]),
            ("append", vec!["must not appear"]),
            ("verify", Vec::new()),
        ] {
            let denied = invoke(action, &alias, &args);
            assert!(!denied.status.success());
            assert!(denied.stdout.is_empty());
            let error = String::from_utf8_lossy(&denied.stderr);
            for private in [record, "private-source", "private-alias", "must not appear"] {
                assert!(!error.contains(private));
            }
            assert_eq!(std::fs::read(&source).unwrap(), bytes);
            assert!(std::fs::symlink_metadata(&alias).is_ok());
        }
        std::fs::remove_file(&alias).unwrap();
        assert_eq!(
            invoke("get", &source, &["1", "0"]).stdout,
            format!("{record}\n").as_bytes()
        );
    }
}
