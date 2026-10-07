use super::tests::{PROJECT, fixture_record, raw_store};
use super::*;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const READY: &str = "EMILYBASE_ACCOUNT_READY";

#[test]
fn account_process_kills_preserve_acknowledged_state_and_exclude_staged_changes() {
    // Fork briefly inherits other tests' locked descriptors before exec closes them.
    let _io = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for mode in ["stage", "commit"] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("synthetic-accounts");
            raw_store(
                &path,
                vec![fixture_record("synthetic", [5; 16], 1).encode()],
            );
            let pool = PasswordPool::new(1).unwrap();
            let mut before = AccountStore::open(&path, PROJECT, pool.clone()).unwrap();
            if compacted {
                before.compact().unwrap();
            }
            let original = before.database.committed_wal().unwrap();
            drop(before);
            kill_worker_at(&path, mode);
            let mut recovered = AccountStore::open(&path, PROJECT, pool).unwrap();
            let info = recovered.record("synthetic").unwrap().unwrap().info;
            assert_eq!(info.id, [5; 16]);
            assert_eq!(info.disabled, mode == "commit");
            assert_eq!(info.credential_epoch, if mode == "commit" { 2 } else { 1 });
            if mode == "stage" {
                assert_eq!(recovered.database.committed_wal().unwrap(), original);
            }
            assert_eq!(recovered.count().unwrap(), 1);
            assert_eq!(
                recovered
                    .check_password("synthetic", b"synthetic-password")
                    .unwrap()
                    .is_some(),
                mode == "stage"
            );
            let archive = directory.path().join("recovered.backup");
            let report = recovered.backup(&archive).unwrap();
            assert_eq!(report.wal_version, if compacted { 2 } else { 1 });
            let restored_path = directory.path().join("restored");
            emilybase_backup::restore(archive, &restored_path).unwrap();
            let restored =
                AccountStore::open(restored_path, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
            assert_eq!(restored.record("synthetic").unwrap().unwrap().info, info);
        }
    }
}

#[test]
#[ignore = "subprocess fixture invoked by the account recovery test"]
fn account_kill_worker() {
    let path = std::env::var_os("EMILYBASE_ACCOUNT_KILL_PATH").unwrap();
    let mode = std::env::var("EMILYBASE_ACCOUNT_KILL_MODE").unwrap();
    let mut store =
        AccountStore::open(Path::new(&path), PROJECT, PasswordPool::new(1).unwrap()).unwrap();
    match mode.as_str() {
        "commit" => {
            let committed = store.set_disabled("synthetic", true).unwrap();
            assert_eq!(committed.credential_epoch, 2);
            println!("{READY}");
            std::io::stdout().flush().unwrap();
            loop {
                std::thread::park();
            }
        }
        "stage" => {
            let mut record = store.record("synthetic").unwrap().unwrap();
            record.info.disabled = true;
            record.info.credential_epoch = 2;
            let mut staged = store.database.begin().unwrap();
            staged
                .update(USERS, &Key::Text("synthetic".into()), record.encode())
                .unwrap();
            println!("{READY}");
            std::io::stdout().flush().unwrap();
            loop {
                std::thread::park();
            }
        }
        "migration-commit" => {
            store.enable_session_storage().unwrap();
            println!("{READY}");
            std::io::stdout().flush().unwrap();
            loop {
                std::thread::park();
            }
        }
        "migration-stage" => {
            use super::session_schema::{META, family_schema, meta_schema};
            let mut staged = store.database.begin().unwrap();
            staged.create_table(meta_schema()).unwrap();
            staged.create_table(family_schema()).unwrap();
            staged
                .insert(
                    META,
                    vec![
                        Value::Integer(1),
                        Value::Integer(1),
                        Value::Bytes(vec![9; 16]),
                    ],
                )
                .unwrap();
            staged
                .update(
                    SCOPE,
                    &Key::Integer(1),
                    vec![
                        Value::Integer(1),
                        Value::Integer(2),
                        Value::Text(PROJECT.into()),
                        Value::Bytes(store.dummy.encode().to_vec()),
                    ],
                )
                .unwrap();
            println!("{READY}");
            std::io::stdout().flush().unwrap();
            loop {
                std::thread::park();
            }
        }
        "clock-enable-commit" | "clock-advance-commit" | "clock-reset-commit" => {
            match mode.as_str() {
                "clock-enable-commit" => {
                    store.enable_session_clock(100).unwrap();
                }
                "clock-advance-commit" => {
                    store.advance_session_clock(200).unwrap();
                }
                _ => {
                    store.reset_session_clock(0).unwrap();
                }
            }
            println!("{READY}");
            std::io::stdout().flush().unwrap();
            loop {
                std::thread::park();
            }
        }
        "clock-enable-stage" => {
            use super::session_clock::{CLOCK, clock_schema};
            use super::session_schema::{META, family_schema, meta_schema};
            let mut staged = store.database.begin().unwrap();
            staged.create_table(meta_schema()).unwrap();
            staged.create_table(family_schema()).unwrap();
            staged.create_table(clock_schema()).unwrap();
            staged
                .insert(
                    META,
                    vec![
                        Value::Integer(1),
                        Value::Integer(1),
                        Value::Bytes(vec![9; 16]),
                    ],
                )
                .unwrap();
            staged
                .insert(
                    CLOCK,
                    vec![Value::Integer(1), Value::Integer(1), Value::Integer(100)],
                )
                .unwrap();
            staged
                .update(
                    SCOPE,
                    &Key::Integer(1),
                    vec![
                        Value::Integer(1),
                        Value::Integer(3),
                        Value::Text(PROJECT.into()),
                        Value::Bytes(store.dummy.encode().to_vec()),
                    ],
                )
                .unwrap();
            println!("{READY}");
            std::io::stdout().flush().unwrap();
            loop {
                std::thread::park();
            }
        }
        "clock-advance-stage" | "clock-reset-stage" => {
            use super::session_clock::CLOCK;
            use super::session_schema::META;
            let reset = mode == "clock-reset-stage";
            let mut staged = store.database.begin().unwrap();
            staged
                .update(
                    CLOCK,
                    &Key::Integer(1),
                    vec![
                        Value::Integer(1),
                        Value::Integer(1),
                        Value::Integer(if reset { 0 } else { 200 }),
                    ],
                )
                .unwrap();
            if reset {
                staged
                    .update(
                        META,
                        &Key::Integer(1),
                        vec![
                            Value::Integer(1),
                            Value::Integer(1),
                            Value::Bytes(vec![9; 16]),
                        ],
                    )
                    .unwrap();
            }
            println!("{READY}");
            std::io::stdout().flush().unwrap();
            loop {
                std::thread::park();
            }
        }
        "session-refresh-commit"
        | "session-refresh-stage"
        | "session-logout-commit"
        | "session-logout-stage" => {
            use std::io::Read;
            let mut input = String::new();
            std::io::stdin()
                .lock()
                .take(103)
                .read_to_string(&mut input)
                .unwrap();
            assert_eq!(input.len(), 102);
            if mode == "session-refresh-commit" {
                let next = store.refresh_session(&input, 100).unwrap();
                assert!(store.verify_access(next.access.expose(), 100).is_ok());
                println!("{READY}");
                std::io::stdout().flush().unwrap();
                loop {
                    std::thread::park();
                }
            } else if mode == "session-logout-commit" {
                store.logout_session(&input, 100).unwrap();
                println!("{READY}");
                std::io::stdout().flush().unwrap();
                loop {
                    std::thread::park();
                }
            } else {
                use super::session_schema::FAMILIES;
                use crate::tokens::{TokenKind, issue, metadata};
                let family = metadata(&input).unwrap().family_id;
                let key = Key::Text(family.iter().map(|b| format!("{b:02x}")).collect());
                let mut row = store
                    .database
                    .view()
                    .unwrap()
                    .get(FAMILIES, &key)
                    .unwrap()
                    .unwrap()
                    .clone();
                if mode == "session-logout-stage" {
                    row[13] = Value::Boolean(true);
                } else {
                    let scope = store.session_storage_scope().unwrap().unwrap();
                    let (_, access) = issue(TokenKind::Access, &scope, family).unwrap();
                    let (_, refresh) = issue(TokenKind::Refresh, &scope, family).unwrap();
                    row[5] = Value::Integer(2);
                    row[11] = Value::Bytes(access.encode().to_vec());
                    row[12] = Value::Bytes(refresh.encode().to_vec());
                }
                let mut staged = store.database.begin().unwrap();
                staged.update(FAMILIES, &key, row).unwrap();
                println!("{READY}");
                std::io::stdout().flush().unwrap();
                loop {
                    std::thread::park();
                }
            }
        }
        _ => panic!("unknown synthetic worker boundary"),
    }
}

pub(super) fn kill_worker_at(path: &Path, mode: &str) {
    kill_worker_with_input(path, mode, "");
}
pub(super) fn kill_worker_with_input(path: &Path, mode: &str, input: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "accounts::recovery_tests::account_kill_worker",
            "--ignored",
            "--nocapture",
        ])
        .env("EMILYBASE_ACCOUNT_KILL_PATH", path)
        .env("EMILYBASE_ACCOUNT_KILL_MODE", mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let found = BufReader::new(stdout)
            .lines()
            .take(64)
            .any(|line| line.is_ok_and(|line| line.contains(READY)));
        let _ = send.send(found);
    });
    let ready = receive.recv_timeout(Duration::from_secs(10));
    // Always clean up the worker before assertions, including handshake failure.
    let killed = child.kill();
    let stopped = child.wait();
    reader.join().unwrap();
    assert!(ready.unwrap(), "worker did not reach requested boundary");
    killed.unwrap();
    assert!(!stopped.unwrap().success());
}
