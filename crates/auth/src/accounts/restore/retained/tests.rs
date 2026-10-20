use super::*;
use crate::accounts::{
    IssuedSession, SCOPE, TEST_IO, USERS, policy_catalog_tests, tests as fixtures,
};
use emilybase_catalog::{Key, Value};
use emilybase_storage::StagedPrivateDirectory;
use proptest::prelude::*;
use std::cell::Cell;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const PROJECT: &str = fixtures::PROJECT;
fn temporary() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    temp
}
fn pool() -> PasswordPool {
    PasswordPool::new(1).unwrap()
}
fn activate(store: &mut AccountStore, version: u16) {
    if version == 2 {
        store.enable_session_storage().unwrap();
    }
    if version >= 3 {
        store.enable_session_clock(100).unwrap();
    }
    if version >= 4 {
        store.enable_row_policy_catalog().unwrap();
        let schema = policy_catalog_tests::schema();
        store
            .install_row_policy(
                policy_catalog_tests::context(&schema),
                0,
                policy_catalog_tests::DENY,
            )
            .unwrap();
    }
    if version >= 5 {
        let receipt = store.enable_public_admission_catalog().unwrap();
        store.set_public_admission(receipt.revision, true).unwrap();
    }
    if version == 6 {
        store.enable_file_policy_catalog().unwrap();
        store
            .install_file_policy(
                super::super::super::file_policy_tests::context(),
                0,
                super::super::super::file_policy_tests::OWN,
            )
            .unwrap();
    }
}
fn source(
    path: &Path,
    version: u16,
    compact: bool,
    tokens: bool,
) -> (AccountStore, Vec<u8>, Option<IssuedSession>) {
    let mut disabled = fixtures::fixture_record("disabled", [8; 16], 3);
    disabled.info.disabled = true;
    fixtures::raw_store(
        path,
        vec![
            fixtures::fixture_record("synthetic", [7; 16], 2).encode(),
            disabled.encode(),
        ],
    );
    let mut store = AccountStore::open(path, PROJECT, pool()).unwrap();
    activate(&mut store, version);
    let token = if tokens && version >= 3 {
        Some(
            store
                .sign_in("synthetic", b"synthetic-password", 100)
                .unwrap(),
        )
    } else {
        None
    };
    if compact {
        store.compact().unwrap();
    }
    let bytes = store.backup_image().unwrap();
    (store, bytes, token)
}
fn descriptors(pid: u32, path: &Path) -> usize {
    let expected = fs::metadata(path).unwrap();
    fs::read_dir(format!("/proc/{pid}/fd"))
        .unwrap()
        .filter_map(|entry| fs::metadata(entry.unwrap().path()).ok())
        .filter(|m| (m.dev(), m.ino()) == (expected.dev(), expected.ino()))
        .count()
}
fn owned(path: &Path, pid: u32) {
    assert!(AccountStore::open(path, PROJECT, pool()).is_err());
    assert!(matches!(
        File::open(path).unwrap().try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    assert!(descriptors(pid, &path.join("redo.wal")) >= 2);
}
fn released(path: &Path, pid: u32) {
    assert_eq!(descriptors(pid, &path.join("redo.wal")), 0);
    drop(AccountStore::open(path, PROJECT, pool()).unwrap());
}
fn stage(parent: &Path) -> PathBuf {
    let entries = fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".emilybase-directory-")
        })
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    entries[0].clone()
}
fn user_rows(store: &AccountStore) -> Vec<Vec<Value>> {
    store
        .database
        .view()
        .unwrap()
        .primary_rows(USERS, None, None)
        .unwrap()
        .map(|r| r.unwrap().clone())
        .collect()
}
fn uncertain(error: &Error) -> bool {
    matches!(
        error,
        Error::RetainedRestoreUnknown(_)
            | Error::Backup(emilybase_backup::Error::RetainedRestoreUnknown(_))
    )
}

#[test]
fn historical_v2_families_remain_records_without_gaining_the_new_clock_scope() {
    let _serial = TEST_IO.lock().unwrap();
    use crate::accounts::session_schema_tests::{family_row, meta, raw_v2};
    for compact in [false, true] {
        let input = temporary();
        let output = temporary();
        let path = input.path().join("legacy");
        let family = family_row([3; 16], [7; 16], 2, [8; 16]);
        raw_v2(&path, vec![family.clone()], vec![meta([9; 16])]);
        let mut original = AccountStore::open(&path, PROJECT, pool()).unwrap();
        if compact {
            original.compact().unwrap();
        }
        let before = original.database.committed_wal().unwrap();
        let bytes = original.backup_image().unwrap();
        let report = with_restored_private_account_bytes_at(
            &bytes,
            PROJECT,
            &File::open(output.path()).unwrap(),
            "copy",
            pool(),
            50,
            |report| report.clone(),
        )
        .unwrap();
        assert_eq!(
            (
                report.private_version,
                report.accounts,
                report.session_families
            ),
            (3, 1, 1)
        );
        let copy = AccountStore::open(output.path().join("copy"), PROJECT, pool()).unwrap();
        assert_eq!(copy.session_clock_floor().unwrap(), Some(50));
        assert_ne!(
            copy.session_storage_scope().unwrap(),
            original.session_storage_scope().unwrap()
        );
        assert_eq!(
            copy.database
                .view()
                .unwrap()
                .get(
                    crate::accounts::session_schema::FAMILIES,
                    &Key::Text("03".repeat(16))
                )
                .unwrap(),
            Some(&family)
        );
        assert_eq!(original.session_clock_floor().unwrap(), None);
        assert_eq!(original.database.committed_wal().unwrap(), before);
    }
}

#[test]
fn every_private_version_and_wal_preserves_users_verifiers_policies_and_resets_only_copy_sessions()
{
    let _serial = TEST_IO.lock().unwrap();
    for version in 1..=6 {
        for compact in [false, true] {
            let input = temporary();
            let output = temporary();
            let (mut original, bytes, token) =
                source(&input.path().join("source"), version, compact, true);
            let before = original.database.committed_wal().unwrap();
            let users = user_rows(&original);
            let scope = original.session_storage_scope().unwrap();
            let policies = if version >= 4 {
                Some(original.row_policy_receipts().unwrap())
            } else {
                None
            };
            let report = with_restored_private_account_bytes_at(
                &bytes,
                PROJECT,
                &File::open(output.path()).unwrap(),
                "copy",
                pool(),
                50,
                |report| {
                    owned(&output.path().join("copy"), std::process::id());
                    assert_eq!(report.clock_floor, Some(50));
                    report.clone()
                },
            )
            .unwrap();
            assert_eq!(report.private_version, version.max(3));
            assert_eq!(report.accounts, 2);
            assert_eq!(report.database.database_id, original.database.database_id());
            assert_eq!(
                report.database.last_transaction,
                original.database.last_transaction() + 1
            );
            assert_eq!(report.database.wal_version, if compact { 2 } else { 1 });
            released(&output.path().join("copy"), std::process::id());
            let mut copy = AccountStore::open(output.path().join("copy"), PROJECT, pool()).unwrap();
            assert_eq!(copy.session_clock_floor().unwrap(), Some(50));
            assert_ne!(copy.session_storage_scope().unwrap(), scope);
            assert_eq!(user_rows(&copy), users);
            assert_eq!(
                copy.database
                    .view()
                    .unwrap()
                    .get(SCOPE, &Key::Integer(1))
                    .unwrap()
                    .unwrap()[3],
                original
                    .database
                    .view()
                    .unwrap()
                    .get(SCOPE, &Key::Integer(1))
                    .unwrap()
                    .unwrap()[3]
            );
            if let Some(policies) = policies {
                assert_eq!(copy.row_policy_receipts().unwrap(), policies);
            }
            if version >= 5 {
                assert!(!copy.public_admission().unwrap().enabled);
                assert!(original.public_admission().unwrap().enabled);
            }
            if version == 6 {
                assert_eq!(
                    copy.file_policy_receipts().unwrap(),
                    original.file_policy_receipts().unwrap()
                );
            }
            if let Some(token) = token {
                assert!(copy.verify_access(token.access.expose(), 100).is_err());
                assert!(copy.refresh_session(token.refresh.expose(), 100).is_err());
                assert!(original.verify_access(token.access.expose(), 100).is_ok());
            }
            let next = copy
                .sign_in("synthetic", b"synthetic-password", 101)
                .unwrap();
            assert_eq!(
                copy.verify_access(next.access.expose(), 101)
                    .unwrap()
                    .account()
                    .id,
                [7; 16]
            );
            assert!(
                copy.sign_in("disabled", b"synthetic-password", 101)
                    .is_err()
            );
            assert_eq!(original.database.committed_wal().unwrap(), before);
            drop(copy);
            let mut reopened =
                AccountStore::open(output.path().join("copy"), PROJECT, pool()).unwrap();
            assert!(reopened.verify_access(next.access.expose(), 101).is_ok());
        }
    }
}

#[test]
fn nested_private_preparations_hold_all_original_owners_through_common_selection() {
    let _serial = TEST_IO.lock().unwrap();
    let input = temporary();
    let output = temporary();
    let (_, bytes, _) = source(&input.path().join("source"), 5, true, false);
    let target = output.path().join("common");
    let stage = StagedPrivateDirectory::new(&target).unwrap();
    let root = stage.directory().try_clone().unwrap();
    let descriptor = PathBuf::from(format!("/proc/self/fd/{}/.", root.as_raw_fd()));
    let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
    let reports = with_restored_private_account_bytes_at(
        &bytes,
        PROJECT,
        &root,
        "first",
        pool(),
        50,
        |first| {
            owned(&descriptor.join("first"), std::process::id());
            with_restored_private_account_bytes_at(
                &bytes,
                PROJECT,
                &root,
                "second",
                pool(),
                50,
                |second| {
                    owned(&descriptor.join("first"), std::process::id());
                    owned(&descriptor.join("second"), std::process::id());
                    let selected = stage.publish().unwrap();
                    selected.check().unwrap();
                    owned(&target.join("first"), std::process::id());
                    owned(&target.join("second"), std::process::id());
                    (first.clone(), second.clone())
                },
            )
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(reports.0, reports.1);
    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline - 2);
    released(&target.join("first"), std::process::id());
    released(&target.join("second"), std::process::id());
    let mut first = AccountStore::open(target.join("first"), PROJECT, pool()).unwrap();
    let mut second = AccountStore::open(target.join("second"), PROJECT, pool()).unwrap();
    assert_ne!(
        first.session_storage_scope().unwrap(),
        second.session_storage_scope().unwrap()
    );
    let session = first
        .sign_in("synthetic", b"synthetic-password", 50)
        .unwrap();
    assert!(first.verify_access(session.access.expose(), 50).is_ok());
    assert!(second.verify_access(session.access.expose(), 50).is_err());
}

#[test]
fn existing_open_still_validates_project_before_filesystem_and_constructor_checks_private_snapshot()
{
    let _serial = TEST_IO.lock().unwrap();
    let input = temporary();
    let missing = input.path().join("missing");
    assert!(matches!(
        AccountStore::open(&missing, "invalid", pool()),
        Err(Error::Scope)
    ));
    assert!(!missing.exists());
    let db = Database::create(input.path().join("engine-only")).unwrap();
    assert!(matches!(
        AccountStore::from_database(db, PROJECT, pool()),
        Err(Error::Corrupt)
    ));
    let (source, _, _) = source(&input.path().join("source"), 1, false, false);
    drop(source);
    let db = Database::open(input.path().join("source")).unwrap();
    assert!(matches!(
        AccountStore::from_database(db, "22222222222222222222222222222222", pool()),
        Err(Error::ScopeMismatch)
    ));
}

#[test]
fn wrong_project_time_private_records_and_native_bounds_refuse_without_staging() {
    let _serial = TEST_IO.lock().unwrap();
    let input = temporary();
    let output = temporary();
    let (_, bytes, _) = source(&input.path().join("source"), 3, false, false);
    let parent = File::open(output.path()).unwrap();
    for (project, now) in [
        ("invalid", 50),
        ("22222222222222222222222222222222", 50),
        (PROJECT, i64::MAX as u64 + 1),
    ] {
        assert!(
            with_restored_private_account_bytes_at(
                &bytes,
                project,
                &parent,
                "copy",
                pool(),
                now,
                |_| panic!("invalid scope/time ran work")
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    }
    for bad in [
        &bytes[..127],
        &bytes[..bytes.len() - 1],
        b"invalid".as_slice(),
    ] {
        assert!(
            with_restored_private_account_bytes_at(
                bad,
                PROJECT,
                &parent,
                "copy",
                pool(),
                50,
                |_| ()
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    }
    let mut row = fixtures::fixture_record("synthetic", [7; 16], 2).encode();
    row[2] = Value::Bytes(vec![]);
    fixtures::raw_store(&input.path().join("bad-private"), vec![row]);
    let mut db = Database::open(input.path().join("bad-private")).unwrap();
    let bad = emilybase_backup::encode(&db.committed_wal().unwrap()).unwrap();
    assert!(
        with_restored_private_account_bytes_at(&bad, PROJECT, &parent, "copy", pool(), 50, |_| ())
            .is_err()
    );
    assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    let long = "x".repeat(256);
    for name in ["", ".", "..", "../escape", "a/b", "nul\0leaf", &long] {
        assert!(
            with_restored_private_account_bytes_at(
                &bytes,
                PROJECT,
                &parent,
                name,
                pool(),
                50,
                |_| ()
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    }
    for mode in [0o755, 0o770, 0o1700] {
        fs::set_permissions(output.path(), fs::Permissions::from_mode(mode)).unwrap();
        assert!(
            with_restored_private_account_bytes_at(
                &bytes,
                PROJECT,
                &parent,
                "copy",
                pool(),
                50,
                |_| ()
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    }
    fs::set_permissions(output.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let maximal = "x".repeat(255);
    with_restored_private_account_bytes_at(&bytes, PROJECT, &parent, &maximal, pool(), 50, |_| ())
        .unwrap();
    released(&output.path().join(maximal), std::process::id());
}

#[derive(Debug)]
struct Receipt(Arc<AtomicUsize>);
impl Drop for Receipt {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn mutate(path: &Path, parent: &Path, saved: &Path, change: usize) {
    let wal = path.join("redo.wal");
    match change {
        0 => {
            fs::rename(&wal, saved).unwrap();
            fs::copy(saved, &wal).unwrap();
            fs::set_permissions(wal, fs::Permissions::from_mode(0o600)).unwrap();
            assert!(descriptors(std::process::id(), saved) >= 2);
        }
        1 => fs::write(wal, b"corrupted original private WAL").unwrap(),
        2 => File::options()
            .append(true)
            .open(wal)
            .unwrap()
            .write_all(b"unacknowledged private tail")
            .unwrap(),
        3 => fs::hard_link(wal, saved).unwrap(),
        4 => fs::set_permissions(wal, fs::Permissions::from_mode(0o644)).unwrap(),
        5 => {
            fs::rename(path, saved).unwrap();
            fs::DirBuilder::new().mode(0o700).create(path).unwrap();
            fs::write(path.join("keep"), b"foreign private directory").unwrap();
        }
        6 => fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap(),
        _ => fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap(),
    }
}

#[test]
fn physical_changes_around_reset_and_work_withhold_results_without_mutating_original_accounts() {
    let _serial = TEST_IO.lock().unwrap();
    for compact in [false, true] {
        for phase in [
            Boundary::Original,
            Boundary::Reset,
            Boundary::Selected,
            Boundary::Worked,
        ] {
            for change in 0..8 {
                let input = temporary();
                let output = temporary();
                let (mut original, bytes, _) =
                    source(&input.path().join("source"), 5, compact, false);
                let before = original.database.committed_wal().unwrap();
                let parent = File::open(output.path()).unwrap();
                let target = output.path().join("copy");
                let saved = output.path().join("original-saved");
                let calls = Cell::new(0);
                let drops = Arc::new(AtomicUsize::new(0));
                let result = with_boundary(
                    &bytes,
                    PROJECT,
                    &parent,
                    "copy",
                    pool(),
                    50,
                    (
                        |_| {
                            calls.set(calls.get() + 1);
                            Receipt(Arc::clone(&drops))
                        },
                        |at| {
                            if at == phase {
                                let path = if matches!(phase, Boundary::Original | Boundary::Reset)
                                {
                                    stage(output.path())
                                } else {
                                    target.clone()
                                };
                                owned(&path, std::process::id());
                                mutate(&path, output.path(), &saved, change);
                            }
                        },
                    ),
                );
                let selected = matches!(phase, Boundary::Selected | Boundary::Worked);
                assert_eq!(
                    target.exists(),
                    selected,
                    "wal2={compact},phase={phase:?},change={change},result={result:?}"
                );
                assert_eq!(calls.get(), usize::from(selected));
                assert_eq!(drops.load(Ordering::SeqCst), usize::from(selected));
                let error = result.unwrap_err();
                assert_eq!(
                    uncertain(&error),
                    selected,
                    "change={change},error={error:?}"
                );
                assert_eq!(original.database.committed_wal().unwrap(), before);
                if matches!(change, 0 | 3 | 5) {
                    assert!(saved.exists());
                }
            }
        }
    }
}

#[test]
fn callback_refusal_and_native_parent_move_preserve_fresh_healthy_private_copy() {
    let _serial = TEST_IO.lock().unwrap();
    let input = temporary();
    let output = temporary();
    let (_, bytes, _) = source(&input.path().join("source"), 3, true, false);
    let parent_path = output.path().join("parent");
    let moved = output.path().join("moved");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&parent_path)
        .unwrap();
    let parent = File::open(&parent_path).unwrap();
    let result = with_restored_private_account_bytes_at(
        &bytes,
        PROJECT,
        &parent,
        "copy",
        pool(),
        50,
        |_| {
            fs::rename(&parent_path, &moved).unwrap();
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&parent_path)
                .unwrap();
            fs::write(parent_path.join("keep"), b"foreign parent").unwrap();
            Err::<(), _>("synthetic native work refusal")
        },
    )
    .unwrap();
    assert_eq!(result, Err("synthetic native work refusal"));
    released(&moved.join("copy"), std::process::id());
    assert_eq!(
        fs::read(parent_path.join("keep")).unwrap(),
        b"foreign parent"
    );
    assert!(!parent_path.join("copy").exists());
    let copy = AccountStore::open(moved.join("copy"), PROJECT, pool()).unwrap();
    assert_eq!(copy.session_clock_floor().unwrap(), Some(50));
}

#[test]
fn existing_file_directory_and_alias_are_preserved_and_user_work_never_runs() {
    let _serial = TEST_IO.lock().unwrap();
    let input = temporary();
    let (_, bytes, _) = source(&input.path().join("source"), 5, false, false);
    for kind in 0..3 {
        let output = temporary();
        let parent = File::open(output.path()).unwrap();
        let target = output.path().join("copy");
        match kind {
            0 => fs::write(&target, b"keep").unwrap(),
            1 => {
                fs::DirBuilder::new().mode(0o700).create(&target).unwrap();
                fs::write(target.join("keep"), b"keep directory").unwrap();
            }
            _ => std::os::unix::fs::symlink(input.path(), &target).unwrap(),
        };
        assert!(
            with_restored_private_account_bytes_at(
                &bytes,
                PROJECT,
                &parent,
                "copy",
                pool(),
                50,
                |_| panic!("occupied target ran work")
            )
            .is_err()
        );
        match kind {
            0 => assert_eq!(fs::read(target).unwrap(), b"keep"),
            1 => assert_eq!(fs::read(target.join("keep")).unwrap(), b"keep directory"),
            _ => assert_eq!(fs::read_link(target).unwrap(), input.path()),
        };
    }
}

pub(crate) fn worker(parent: &Path, mode: &str) {
    let phase = mode.strip_prefix("retained-restore-").unwrap();
    let bytes = fs::read(parent.join("private.backup")).unwrap();
    let target = parent.join("restored");
    let ready = || {
        println!("EMILYBASE_ACCOUNT_READY");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    };
    let report = with_boundary(
        &bytes,
        PROJECT,
        &File::open(parent).unwrap(),
        "restored",
        pool(),
        50,
        (
            |report| report.clone(),
            |at| {
                if matches!(
                    (phase, at),
                    ("owner", Boundary::Original)
                        | ("reset", Boundary::Reset)
                        | ("selected", Boundary::Selected)
                        | ("worked", Boundary::Worked)
                ) {
                    ready();
                }
            },
        ),
    )
    .unwrap();
    assert_eq!(phase, "returned");
    assert_eq!(report.clock_floor, Some(50));
    released(&target, std::process::id());
    ready();
}

#[test]
fn actual_kills_recover_only_acknowledged_reset_and_keep_original_sessions_current() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = TEST_IO.lock().unwrap();
    for (version, compact) in [(1, false), (5, true)] {
        for phase in ["owner", "reset", "selected", "worked", "returned"] {
            let input = temporary();
            let output = temporary();
            let (mut original, bytes, token) =
                source(&input.path().join("source"), version, compact, true);
            let before = original.database.committed_wal().unwrap();
            let old_scope = original.session_storage_scope().unwrap();
            fs::write(output.path().join("private.backup"), &bytes).unwrap();
            let mode = format!("retained-restore-{phase}");
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "accounts::recovery_tests::account_kill_worker",
                    "--ignored",
                    "--nocapture",
                ])
                .env("EMILYBASE_ACCOUNT_KILL_PATH", output.path())
                .env("EMILYBASE_ACCOUNT_KILL_MODE", mode)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let stdout = child.stdout.take().unwrap();
            let (send, receive) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    if line.unwrap() == "EMILYBASE_ACCOUNT_READY" {
                        send.send(()).unwrap();
                        return;
                    }
                }
            });
            let reached = receive.recv_timeout(std::time::Duration::from_secs(30));
            let selected = matches!(phase, "selected" | "worked" | "returned");
            if reached.is_ok() {
                let path = if selected {
                    output.path().join("restored")
                } else {
                    stage(output.path())
                };
                assert_eq!(output.path().join("restored").exists(), selected);
                if phase == "returned" {
                    released(&path, child.id());
                } else {
                    owned(&path, child.id());
                }
            }
            let _ = child.kill();
            let status = child.wait().unwrap();
            reader.join().unwrap();
            let mut stderr = String::new();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut stderr)
                .unwrap();
            assert!(reached.is_ok(), "phase={phase},stderr={stderr}");
            assert_eq!(status.signal(), Some(9));
            let path = if selected {
                output.path().join("restored")
            } else {
                stage(output.path())
            };
            let mut copy = AccountStore::open(&path, PROJECT, pool()).unwrap();
            let reset = phase != "owner";
            let copy_report =
                inspect_private_account_backup_bytes(&copy.backup_image().unwrap(), PROJECT)
                    .unwrap();
            assert_eq!(
                copy_report.private_version,
                if reset { version.max(3) } else { version }
            );
            assert_eq!(
                copy_report.database.last_transaction,
                original.database.last_transaction() + u64::from(reset)
            );
            assert_eq!(
                copy.session_clock_floor().unwrap(),
                if reset {
                    Some(50)
                } else if version >= 3 {
                    Some(100)
                } else {
                    None
                }
            );
            assert_eq!(user_rows(&copy), user_rows(&original));
            if reset {
                assert_ne!(copy.session_storage_scope().unwrap(), old_scope);
                if version == 5 {
                    assert!(!copy.public_admission().unwrap().enabled);
                }
                if let Some(token) = &token {
                    assert!(copy.verify_access(token.access.expose(), 100).is_err());
                    assert!(copy.refresh_session(token.refresh.expose(), 100).is_err());
                }
                let next = copy
                    .sign_in("synthetic", b"synthetic-password", 101)
                    .unwrap();
                assert!(copy.verify_access(next.access.expose(), 101).is_ok());
            }
            if let Some(token) = &token {
                assert!(original.verify_access(token.access.expose(), 100).is_ok());
            }
            assert_eq!(original.database.committed_wal().unwrap(), before);
            assert_eq!(
                fs::read(output.path().join("private.backup")).unwrap(),
                bytes
            );
            drop(copy);
            with_restored_private_account_bytes_at(
                &bytes,
                PROJECT,
                &File::open(output.path()).unwrap(),
                "fresh-retry",
                pool(),
                50,
                |report| report.clone(),
            )
            .unwrap();
            assert_eq!(
                AccountStore::open(output.path().join("fresh-retry"), PROJECT, pool())
                    .unwrap()
                    .session_clock_floor()
                    .unwrap(),
                Some(50)
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_private_copies_preserve_exact_user_records_versions_and_policy_receipts(
        version in 1u16..6,
        compact in any::<bool>(),
        disabled in prop::collection::vec(any::<bool>(), 0..5),
    ) {
        let _serial = TEST_IO.lock().unwrap();
        let input = temporary();
        let output = temporary();
        let path = input.path().join("source");
        let rows = disabled.iter().enumerate().map(|(index, disabled)| {
            let mut record = fixtures::fixture_record(&format!("u{index}"), [index as u8 + 1; 16], 2);
            record.info.disabled = *disabled;
            record.encode()
        }).collect();
        fixtures::raw_store(&path, rows);
        let mut original = AccountStore::open(&path, PROJECT, pool()).unwrap();
        activate(&mut original, version);
        if compact {
            original.compact().unwrap();
        }
        let before = original.database.committed_wal().unwrap();
        let bytes = original.backup_image().unwrap();
        let users = user_rows(&original);
        let policy = if version >= 4 {
            Some(original.row_policy_receipts().unwrap())
        } else {
            None
        };
        let report = with_restored_private_account_bytes_at(
            &bytes, PROJECT, &File::open(output.path()).unwrap(), "copy", pool(), 50,
            |report| report.clone(),
        ).unwrap();
        let copy = AccountStore::open(output.path().join("copy"), PROJECT, pool()).unwrap();
        prop_assert_eq!(user_rows(&copy), users);
        prop_assert_eq!(report.accounts, disabled.len());
        prop_assert_eq!(report.private_version, version.max(3));
        prop_assert_eq!(report.clock_floor, Some(50));
        if let Some(policy) = policy {
            prop_assert_eq!(copy.row_policy_receipts().unwrap(), policy);
        }
        if version == 5 {
            prop_assert!(!copy.public_admission().unwrap().enabled);
        }
        prop_assert_eq!(original.database.committed_wal().unwrap(), before);
    }
}
