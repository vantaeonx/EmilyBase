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
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "accounts::recovery_tests::account_kill_worker",
                    "--ignored",
                    "--nocapture",
                ])
                .env("EMILYBASE_ACCOUNT_KILL_PATH", &path)
                .env("EMILYBASE_ACCOUNT_KILL_MODE", mode)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
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
        _ => panic!("unknown synthetic worker boundary"),
    }
}
