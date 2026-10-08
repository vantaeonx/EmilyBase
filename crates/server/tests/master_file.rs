// Shared native fixtures serve several independent integration binaries.
#[allow(dead_code)]
mod support;
use serde_json::json;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
const KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NEXT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
fn write_key(path: &Path, bytes: &[u8], mode: u32) {
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}
fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_emilybase-server"));
    for name in [
        "EMILYBASE_MASTER_KEY",
        "EMILYBASE_MASTER_KEY_FILE",
        "EMILYBASE_DATA_DIR",
        "EMILYBASE_ACCOUNT_ROOT",
        "EMILYBASE_LISTEN",
    ] {
        command.env_remove(name);
    }
    command
}
fn refuses(mut command: Command, root: &Path, private: &[&str]) {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("configuration read exceeded startup deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(!root.exists());
    assert!(output.stderr.is_empty());
    let log = String::from_utf8(output.stdout).unwrap();
    assert!(log.contains("startup_failed"));
    for secret in private {
        assert!(!log.contains(secret), "private configuration leaked");
    }
}
#[test]
fn invalid_file_configuration_refuses_before_either_data_mode_is_opened() {
    let dir = tempfile::tempdir().unwrap();
    let valid = dir.path().join("synthetic-secret-path");
    write_key(&valid, KEY.as_bytes(), 0o600);
    let wide = dir.path().join("wide");
    write_key(&wide, KEY.as_bytes(), 0o600);
    fs::set_permissions(&wide, fs::Permissions::from_mode(0o640)).unwrap();
    let alias = dir.path().join("symlink");
    symlink(&valid, &alias).unwrap();
    let linked = dir.path().join("linked");
    write_key(&linked, KEY.as_bytes(), 0o600);
    fs::hard_link(&linked, dir.path().join("hard-alias")).unwrap();
    let huge = dir.path().join("oversized");
    write_key(&huge, &[b'a'; 4096], 0o600);
    let invalid = dir.path().join("invalid");
    write_key(&invalid, &[255; 64], 0o600);
    let newline = dir.path().join("crlf");
    write_key(&newline, format!("{KEY}\r\n").as_bytes(), 0o600);
    let missing = dir.path().join("missing");
    for data_variable in ["EMILYBASE_DATA_DIR", "EMILYBASE_ACCOUNT_ROOT"] {
        let root = dir.path().join(data_variable);
        for path in [
            &missing,
            dir.path(),
            &wide,
            &alias,
            &linked,
            &huge,
            &invalid,
            &newline,
            Path::new(""),
        ] {
            let mut child = command();
            child
                .env(data_variable, &root)
                .env("EMILYBASE_MASTER_KEY_FILE", path);
            refuses(child, &root, &[KEY, "synthetic-secret-path"]);
        }
        for environment in ["", KEY] {
            let mut child = command();
            child
                .env(data_variable, &root)
                .env("EMILYBASE_MASTER_KEY_FILE", &valid)
                .env("EMILYBASE_MASTER_KEY", environment);
            refuses(child, &root, &[KEY, "synthetic-secret-path"]);
        }
    }
    assert_eq!(fs::read(&valid).unwrap(), KEY.as_bytes());
    assert_eq!(fs::read(&wide).unwrap(), KEY.as_bytes());
}
#[test]
fn unopened_fifo_refuses_with_a_native_process_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("synthetic-private-fifo");
    assert!(
        Command::new("mkfifo")
            .arg("--mode=600")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let root = dir.path().join("absent");
    let mut child = command();
    child
        .env("EMILYBASE_DATA_DIR", &root)
        .env("EMILYBASE_MASTER_KEY_FILE", &fifo);
    refuses(child, &root, &[KEY, "synthetic-private-fifo"]);
}
#[test]
fn legacy_server_loads_file_once_and_new_key_requires_restart_without_data_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic-private-path");
    write_key(&path, format!("{KEY}\n").as_bytes(), 0o600);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    let root = dir.path().join("registry");
    let server = support::Server::start_file(&root, &path);
    let (status, created) = support::call(
        server.address,
        "POST",
        "/v1/projects",
        KEY,
        &json!({"name":"synthetic-file-project"}),
    )
    .unwrap();
    assert_eq!(status, 201);
    let id = created["project"]["id"].as_str().unwrap();
    let service = created["api_key"].as_str().unwrap();
    let replacement = dir.path().join("replacement");
    write_key(&replacement, NEXT.as_bytes(), 0o600);
    fs::rename(replacement, &path).unwrap();
    assert_eq!(
        support::call(server.address, "GET", "/v1/projects", KEY, &json!({}))
            .unwrap()
            .0,
        200
    );
    assert_eq!(
        support::call(server.address, "GET", "/v1/projects", NEXT, &json!({}))
            .unwrap()
            .0,
        401
    );
    let first_log = server.stop();
    let data = root.join(id).join("data");
    let before_wal = emilybase_transactions::Database::open(&data)
        .unwrap()
        .committed_wal()
        .unwrap();
    let inventory = serde_json::to_value(
        emilybase_server::ProjectStore::open(&root)
            .unwrap()
            .list()
            .unwrap(),
    )
    .unwrap();
    let server = support::Server::start_file(&root, &path);
    assert_eq!(
        support::call(server.address, "GET", "/v1/projects", KEY, &json!({}))
            .unwrap()
            .0,
        401
    );
    let (status, listed) =
        support::call(server.address, "GET", "/v1/projects", NEXT, &json!({})).unwrap();
    assert_eq!(status, 200);
    assert_eq!(listed[0]["id"], id);
    assert_eq!(
        support::call(
            server.address,
            "GET",
            &format!("/v1/projects/{id}/status"),
            service,
            &json!({})
        )
        .unwrap()
        .0,
        200
    );
    let second_log = server.stop();
    assert_eq!(
        emilybase_transactions::Database::open(data)
            .unwrap()
            .committed_wal()
            .unwrap(),
        before_wal
    );
    assert_eq!(
        serde_json::to_value(
            emilybase_server::ProjectStore::open(&root)
                .unwrap()
                .list()
                .unwrap()
        )
        .unwrap(),
        inventory
    );
    for secret in [KEY, NEXT, id, service, "synthetic-private-path"] {
        assert!(!first_log.contains(secret));
        assert!(!second_log.contains(secret));
    }
    assert_eq!(fs::read(path).unwrap(), NEXT.as_bytes());
}
