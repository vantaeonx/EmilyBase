use super::*;
use crate::ownership_tests::FailureGuard;
use crate::publication_tests::PROCESS_TESTS;
use emilybase_catalog::{Column, DataType, Schema, Value};
use proptest::prelude::*;
use std::cell::Cell;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Debug, thiserror::Error)]
#[error("synthetic native preparation refusal")]
struct Refusal;

fn temporary() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    dir
}
fn schema() -> Schema {
    Schema {
        name: "t".into(),
        columns: vec![
            Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
            Column {
                name: "v".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
        ],
        primary_key: 0,
    }
}
fn source(parent: &Path, compact: bool, empty: bool) -> (Database, Vec<u8>) {
    let mut db = Database::create(parent.join("source")).unwrap();
    if !empty {
        let mut tx = db.begin().unwrap();
        tx.create_table(schema()).unwrap();
        tx.insert("t", vec![Value::Integer(1), Value::Integer(17)])
            .unwrap();
        tx.commit().unwrap();
    }
    if compact {
        db.compact().unwrap();
    }
    let bytes = crate::encode(&db.committed_wal().unwrap()).unwrap();
    (db, bytes)
}
fn prepare(db: &mut Database, value: i64) {
    let empty = db.view().unwrap().schemas().is_empty();
    let mut tx = db.begin().unwrap();
    if empty {
        tx.create_table(schema()).unwrap();
    }
    tx.insert("t", vec![Value::Integer(2), Value::Integer(value)])
        .unwrap();
    tx.commit().unwrap();
}
fn stage(parent: &Path) -> PathBuf {
    let entries = fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
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
pub(crate) fn descriptors(pid: u32, path: &Path) -> usize {
    let expected = fs::metadata(path).unwrap();
    fs::read_dir(format!("/proc/{pid}/fd"))
        .unwrap()
        .filter_map(|entry| fs::metadata(entry.unwrap().path()).ok())
        .filter(|m| (m.dev(), m.ino()) == (expected.dev(), expected.ino()))
        .count()
}
pub(crate) fn owned(path: &Path, pid: u32) {
    assert!(Database::open(path).is_err());
    assert!(matches!(
        File::open(path).unwrap().try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    assert!(descriptors(pid, &path.join("redo.wal")) >= 2);
}
pub(crate) fn released(path: &Path, pid: u32) {
    assert_eq!(descriptors(pid, &path.join("redo.wal")), 0);
    drop(Database::open(path).unwrap());
}

struct ApplicationOwner {
    database: Database,
    drops: Arc<AtomicUsize>,
    marker: u64,
}
impl PreparedDatabaseOwner for ApplicationOwner {
    fn database(&mut self) -> &mut Database {
        &mut self.database
    }
}
impl Drop for ApplicationOwner {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn actual_original_owner_moves_into_application_and_spans_preparation_selection_and_work() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    for compact in [false, true] {
        for empty in [false, true] {
            let input = temporary();
            let output = temporary();
            let (mut original, bytes) = source(input.path(), compact, empty);
            let wal = original.committed_wal().unwrap();
            let input_report = inspect_bytes(&bytes).unwrap();
            let parent = File::open(output.path()).unwrap();
            let target = output.path().join("copy");
            let drops = Arc::new(AtomicUsize::new(0));
            let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
            let (report, prepared) = with_boundary(
                &bytes,
                &parent,
                "copy",
                |mut database| {
                    let path = stage(output.path());
                    owned(&path, std::process::id());
                    assert!(!target.exists());
                    assert_eq!(database.committed_wal().unwrap(), wal);
                    prepare(&mut database, 43);
                    Ok::<_, Refusal>(ApplicationOwner {
                        database,
                        drops: Arc::clone(&drops),
                        marker: 91,
                    })
                },
                |owner, report| {
                    assert_eq!(owner.marker, 91);
                    assert_eq!(drops.load(Ordering::SeqCst), 0);
                    owned(&target, std::process::id());
                    assert_eq!(report.database_id, input_report.database_id);
                    assert_eq!(report.last_transaction, input_report.last_transaction + 1);
                    assert_eq!(report.rows, input_report.rows + 1);
                    assert_eq!(report.wal_version, input_report.wal_version);
                    (report.clone(), owner.database().committed_wal().unwrap())
                },
                |at| {
                    if at == Boundary::Worked {
                        owned(&target, std::process::id());
                        assert_eq!(drops.load(Ordering::SeqCst), 0);
                    }
                },
            )
            .unwrap();
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
            released(&target, std::process::id());
            assert_eq!(fs::read(target.join("redo.wal")).unwrap(), prepared);
            assert!(prepared.starts_with(&wal));
            assert_eq!(original.committed_wal().unwrap(), wal);
            let mut restored = Database::open(&target).unwrap();
            assert_eq!(restored.view().unwrap().row_count(), report.rows);
            let mut tx = restored.begin().unwrap();
            tx.insert("t", vec![Value::Integer(3), Value::Integer(59)])
                .unwrap();
            tx.commit().unwrap();
            assert_eq!(restored.last_transaction(), report.last_transaction + 1);
        }
    }
}

#[test]
fn native_directory_comparison_refuses_foreign_descriptors_without_poisoning_original() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let input = temporary();
    let foreign = temporary();
    let (mut db, _) = source(input.path(), false, false);
    let owner = File::open(input.path().join("source")).unwrap();
    db.check_directory_owner(&owner).unwrap();
    for other in [
        File::open(foreign.path()).unwrap(),
        File::open(input.path().join("source/redo.wal")).unwrap(),
    ] {
        assert!(matches!(
            db.check_directory_owner(&other),
            Err(emilybase_transactions::Error::DirectoryChanged)
        ));
        db.check_directory_owner(&owner).unwrap();
    }
    prepare(&mut db, 31);
    assert_eq!(db.view().unwrap().row_count(), 2);
}

#[test]
fn same_identity_same_history_foreign_database_and_compaction_refuse_before_selection() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let input = temporary();
    let (_, bytes) = source(input.path(), false, false);
    for compact in [false, true] {
        let output = temporary();
        let parent = File::open(output.path()).unwrap();
        let target = output.path().join("copy");
        let other = output.path().join("foreign");
        crate::restore_bytes(&bytes, &other).unwrap();
        let foreign = Database::open(&other).unwrap();
        let result = with_prepared_owner_bytes_at(
            &bytes,
            &parent,
            "copy",
            |mut db| {
                if compact {
                    drop(foreign);
                    db.compact().unwrap();
                    Ok::<_, Refusal>(db)
                } else {
                    assert_eq!(db.database_id(), foreign.database_id());
                    drop(db);
                    Ok(foreign)
                }
            },
            |_, _| panic!("invalid owner ran work"),
        );
        assert!(matches!(result, Err(PreparedRestoreError::Backup(_))));
        assert!(!target.exists());
        assert_eq!(
            fs::read(other.join("redo.wal")).unwrap(),
            bytes[HEADER_SIZE..]
        );
    }
}

#[test]
fn factory_refusal_after_durable_preparation_keeps_private_stage_and_original_source() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let input = temporary();
    let output = temporary();
    let (mut original, bytes) = source(input.path(), false, false);
    let wal = original.committed_wal().unwrap();
    let parent = File::open(output.path()).unwrap();
    let result = with_prepared_owner_bytes_at(
        &bytes,
        &parent,
        "copy",
        |mut db| {
            prepare(&mut db, 37);
            Err::<Database, _>(Refusal)
        },
        |_, _| panic!("refused preparation ran work"),
    );
    assert!(matches!(
        result,
        Err(PreparedRestoreError::Preparation(Refusal))
    ));
    assert!(!output.path().join("copy").exists());
    let staged = stage(output.path());
    released(&staged, std::process::id());
    assert_eq!(
        Database::open(staged).unwrap().view().unwrap().row_count(),
        2
    );
    assert_eq!(original.committed_wal().unwrap(), wal);
}

#[test]
fn readonly_callback_refusal_is_a_value_and_preserves_complete_healthy_copy() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let input = temporary();
    let output = temporary();
    let (_, bytes) = source(input.path(), true, false);
    let parent = File::open(output.path()).unwrap();
    let result = with_prepared_owner_bytes_at(&bytes, &parent, "copy", Ok::<_, Refusal>, |_, _| {
        Err::<(), _>(Refusal)
    })
    .unwrap();
    assert!(matches!(result, Err(Refusal)));
    released(&output.path().join("copy"), std::process::id());
    assert_eq!(
        fs::read(output.path().join("copy/redo.wal")).unwrap(),
        bytes[HEADER_SIZE..]
    );
}

#[test]
fn callback_commit_is_not_silently_adopted_or_rolled_back_after_selection() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let input = temporary();
    let output = temporary();
    let (_, bytes) = source(input.path(), false, false);
    let parent = File::open(output.path()).unwrap();
    let result =
        with_prepared_owner_bytes_at(&bytes, &parent, "copy", Ok::<_, Refusal>, |db, _| {
            prepare(db, 47)
        });
    assert!(matches!(
        result,
        Err(PreparedRestoreError::Backup(Error::RetainedRestoreUnknown(
            _
        )))
    ));
    let restored = Database::open(output.path().join("copy")).unwrap();
    assert_eq!(restored.view().unwrap().row_count(), 2);
    assert_eq!(restored.last_transaction(), 3);
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
        1 => fs::write(wal, b"corrupted original WAL").unwrap(),
        2 => File::options()
            .append(true)
            .open(wal)
            .unwrap()
            .write_all(b"unacknowledged tail")
            .unwrap(),
        3 => fs::hard_link(wal, saved).unwrap(),
        4 => fs::set_permissions(wal, fs::Permissions::from_mode(0o644)).unwrap(),
        5 => {
            fs::rename(path, saved).unwrap();
            fs::DirBuilder::new().mode(0o700).create(path).unwrap();
            fs::write(path.join("keep"), b"foreign root").unwrap();
        }
        6 => fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap(),
        _ => fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap(),
    }
}

#[test]
fn physical_changes_before_and_after_selection_withhold_results_and_preserve_foreign_files() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    for compact in [false, true] {
        for phase in [Boundary::Prepared, Boundary::Selected, Boundary::Worked] {
            for change in 0..8 {
                let input = temporary();
                let output = temporary();
                let (mut original, bytes) = source(input.path(), compact, false);
                let wal = original.committed_wal().unwrap();
                let parent = File::open(output.path()).unwrap();
                let target = output.path().join("copy");
                let saved = output.path().join("original-saved");
                let drops = Arc::new(AtomicUsize::new(0));
                let calls = Cell::new(0);
                let result = with_boundary(
                    &bytes,
                    &parent,
                    "copy",
                    |mut db| {
                        prepare(&mut db, 43);
                        Ok::<_, Refusal>(db)
                    },
                    |_, _| {
                        calls.set(calls.get() + 1);
                        Receipt(Arc::clone(&drops))
                    },
                    |at| {
                        if at == phase {
                            let path = if phase == Boundary::Prepared {
                                stage(output.path())
                            } else {
                                target.clone()
                            };
                            owned(&path, std::process::id());
                            mutate(&path, output.path(), &saved, change);
                        }
                    },
                );
                let selected = phase != Boundary::Prepared;
                assert_eq!(target.exists(), selected);
                assert_eq!(calls.get(), usize::from(phase == Boundary::Worked));
                assert_eq!(
                    drops.load(Ordering::SeqCst),
                    usize::from(phase == Boundary::Worked)
                );
                if selected {
                    assert!(
                        matches!(
                            result,
                            Err(PreparedRestoreError::Backup(Error::RetainedRestoreUnknown(
                                _
                            )))
                        ),
                        "change={change}"
                    );
                } else {
                    assert!(matches!(result, Err(PreparedRestoreError::Backup(_))));
                }
                if matches!(change, 0 | 3 | 5) {
                    assert!(saved.exists());
                }
                assert_eq!(original.committed_wal().unwrap(), wal);
            }
        }
    }
}

#[test]
fn original_parent_descriptor_survives_moves_without_adopting_old_path() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let input = temporary();
    let output = temporary();
    let (_, bytes) = source(input.path(), true, false);
    let path = output.path().join("parent");
    let moved = output.path().join("moved");
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    let parent = File::open(&path).unwrap();
    with_prepared_owner_bytes_at(&bytes, &parent, "copy", Ok::<_, Refusal>, |_, _| {
        fs::rename(&path, &moved).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        fs::write(path.join("keep"), b"foreign parent").unwrap();
        owned(&moved.join("copy"), std::process::id());
    })
    .unwrap();
    released(&moved.join("copy"), std::process::id());
    assert_eq!(fs::read(path.join("keep")).unwrap(), b"foreign parent");
    assert!(!path.join("copy").exists());
}

#[test]
fn invalid_image_leaf_privacy_and_existing_targets_never_run_application_work() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let input = temporary();
    let output = temporary();
    let (_, bytes) = source(input.path(), false, false);
    let parent = File::open(output.path()).unwrap();
    let long = "x".repeat(256);
    for name in ["", ".", "..", "../escape", "a/b", "nul\0leaf", &long] {
        assert!(
            with_prepared_owner_bytes_at(&bytes, &parent, name, Ok::<_, Refusal>, |_, _| panic!(
                "invalid leaf ran work"
            ))
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
            with_prepared_owner_bytes_at(bad, &parent, "copy", Ok::<_, Refusal>, |_, _| ())
                .is_err()
        );
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    }
    for mode in [0o755, 0o770, 0o1700, 0o2700] {
        fs::set_permissions(output.path(), fs::Permissions::from_mode(mode)).unwrap();
        assert!(
            with_prepared_owner_bytes_at(&bytes, &parent, "copy", Ok::<_, Refusal>, |_, _| ())
                .is_err()
        );
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    }
    fs::set_permissions(output.path(), fs::Permissions::from_mode(0o700)).unwrap();
    for kind in 0..3 {
        let leaf = format!("existing-{kind}");
        let target = output.path().join(&leaf);
        match kind {
            0 => fs::write(&target, b"keep").unwrap(),
            1 => {
                fs::DirBuilder::new().mode(0o700).create(&target).unwrap();
                fs::write(target.join("keep"), b"keep directory").unwrap();
            }
            _ => std::os::unix::fs::symlink(input.path(), &target).unwrap(),
        };
        assert!(
            with_prepared_owner_bytes_at(&bytes, &parent, &leaf, Ok::<_, Refusal>, |_, _| panic!(
                "existing target ran work"
            ))
            .is_err()
        );
        match kind {
            0 => assert_eq!(fs::read(target).unwrap(), b"keep"),
            1 => assert_eq!(fs::read(target.join("keep")).unwrap(), b"keep directory"),
            _ => assert_eq!(fs::read_link(target).unwrap(), input.path()),
        };
    }
    let maximal = "x".repeat(255);
    with_prepared_owner_bytes_at(&bytes, &parent, &maximal, Ok::<_, Refusal>, |_, _| ()).unwrap();
    released(&output.path().join(maximal), std::process::id());
}

#[test]
fn sync_faults_distinguish_private_preparation_from_selected_uncertainty() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let input = temporary();
    let (_, bytes) = source(input.path(), true, false);
    for point in [
        "owned_restore_input_sync",
        "owned_restore_prepared_sync",
        "owned_restore_stage_sync",
        "owned_restore_confirm_wal_sync",
        "owned_restore_confirm_parent_sync",
    ] {
        for after in [false, true] {
            let output = temporary();
            let parent = File::open(output.path()).unwrap();
            let calls = Cell::new(0);
            let fault = FailureGuard::new(point, after);
            let result = with_prepared_owner_bytes_at(
                &bytes,
                &parent,
                "copy",
                |mut db| {
                    prepare(&mut db, 53);
                    Ok::<_, Refusal>(db)
                },
                |_, _| {
                    calls.set(calls.get() + 1);
                },
            );
            drop(fault);
            let selected = point.contains("confirm_");
            assert_eq!(output.path().join("copy").exists(), selected);
            assert_eq!(calls.get(), usize::from(selected));
            if selected {
                assert!(matches!(
                    result,
                    Err(PreparedRestoreError::Backup(Error::RetainedRestoreUnknown(
                        _
                    )))
                ));
                assert_eq!(
                    Database::open(output.path().join("copy"))
                        .unwrap()
                        .view()
                        .unwrap()
                        .row_count(),
                    2
                );
            } else {
                assert!(matches!(
                    result,
                    Err(PreparedRestoreError::Backup(Error::Io(_)))
                ));
            }
        }
    }
}

pub(crate) fn worker(input: &Path, target: &Path, phase: &str) {
    let bytes = fs::read(input).unwrap();
    let parent = File::open(target.parent().unwrap()).unwrap();
    let name = target.file_name().unwrap().to_str().unwrap();
    let barrier = || {
        println!("READY");
        std::io::stdout().flush().unwrap();
        let _ = std::io::stdin().read(&mut [0; 1]);
    };
    with_boundary(
        &bytes,
        &parent,
        name,
        |mut db| {
            prepare(&mut db, 43);
            if phase == "factory" {
                owned(&stage(target.parent().unwrap()), std::process::id());
                barrier();
            }
            Ok::<_, Refusal>(db)
        },
        |_, report| report.clone(),
        |at| {
            if matches!(
                (phase, at),
                ("source", Boundary::Source)
                    | ("prepared", Boundary::Prepared)
                    | ("selected", Boundary::Selected)
                    | ("worked", Boundary::Worked)
            ) {
                barrier();
            }
        },
    )
    .unwrap();
    assert_eq!(phase, "returned");
    released(target, std::process::id());
    barrier();
}

#[test]
fn process_kills_preserve_unselected_preparation_and_recover_selected_prepared_history() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};
    let _serial = PROCESS_TESTS.lock().unwrap();
    for compact in [false, true] {
        for empty in [false, true] {
            for phase in [
                "source", "factory", "prepared", "selected", "worked", "returned",
            ] {
                let input = temporary();
                let output = temporary();
                let (mut original, bytes) = source(input.path(), compact, empty);
                let before = original.committed_wal().unwrap();
                let archive = output.path().join("archive");
                fs::write(&archive, &bytes).unwrap();
                let target = output.path().join("copy");
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "publication_tests::publication_worker",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("EMILYBASE_PUBLICATION_INPUT", &archive)
                    .env("EMILYBASE_PUBLICATION_TARGET", &target)
                    .env("EMILYBASE_PUBLICATION_KIND", "owned-restore")
                    .env("EMILYBASE_PUBLICATION_PHASE", phase)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let stdout = child.stdout.take().unwrap();
                let (send, receive) = std::sync::mpsc::channel();
                let reader = std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines() {
                        if line.unwrap() == "READY" {
                            send.send(()).unwrap();
                            return;
                        }
                    }
                });
                let reached = receive.recv_timeout(std::time::Duration::from_secs(30));
                if reached.is_ok() {
                    let selected = matches!(phase, "selected" | "worked" | "returned");
                    assert_eq!(target.exists(), selected);
                    let path = if selected {
                        target.clone()
                    } else {
                        stage(output.path())
                    };
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
                let selected = matches!(phase, "selected" | "worked" | "returned");
                let path = if selected {
                    target.clone()
                } else {
                    stage(output.path())
                };
                let mut restored = Database::open(path).unwrap();
                let prepared = phase != "source";
                assert_eq!(
                    restored.view().unwrap().row_count(),
                    usize::from(!empty) + usize::from(prepared)
                );
                assert_eq!(
                    restored.last_transaction(),
                    inspect_bytes(&bytes).unwrap().last_transaction + u64::from(prepared)
                );
                if prepared {
                    let mut tx = restored.begin().unwrap();
                    tx.insert("t", vec![Value::Integer(3), Value::Integer(61)])
                        .unwrap();
                    tx.commit().unwrap();
                }
                drop(restored);
                assert_eq!(original.committed_wal().unwrap(), before);
                assert_eq!(fs::read(&archive).unwrap(), bytes);
                with_prepared_owner_bytes_at(
                    &bytes,
                    &File::open(output.path()).unwrap(),
                    "fresh-retry",
                    Ok::<_, Refusal>,
                    |_, report| report.clone(),
                )
                .unwrap();
                assert_eq!(
                    fs::read(output.path().join("fresh-retry/redo.wal")).unwrap(),
                    before
                );
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_prepared_states_preserve_source_prefix_identity_and_exact_rows(compact in any::<bool>(),empty in any::<bool>(),value in any::<i64>()) {
        let _serial=PROCESS_TESTS.lock().unwrap();let input=temporary();let output=temporary();let (mut original,bytes)=source(input.path(),compact,empty);let before=original.committed_wal().unwrap();let parent=File::open(output.path()).unwrap();let target=output.path().join("copy");
        let report=with_prepared_owner_bytes_at(&bytes,&parent,"copy",|mut db|{prepare(&mut db,value);Ok::<_,Refusal>(db)},|_,report|{owned(&target,std::process::id());report.clone()}).unwrap();released(&target,std::process::id());let mut db=Database::open(&target).unwrap();
        let mut expected=Vec::new();if !empty{expected.push(vec![Value::Integer(1),Value::Integer(17)]);}expected.push(vec![Value::Integer(2),Value::Integer(value)]);
        prop_assert_eq!(db.view().unwrap().scan("t",100).unwrap(),expected);prop_assert_eq!(report.rows,usize::from(!empty)+1);prop_assert_eq!(report.database_id,original.database_id());prop_assert!(db.committed_wal().unwrap().starts_with(&before));prop_assert_eq!(original.committed_wal().unwrap(),before);
    }
}
