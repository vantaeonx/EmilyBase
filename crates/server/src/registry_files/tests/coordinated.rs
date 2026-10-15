use super::*;
use crate::with_restored_registry_backup_bytes_at;
use emilybase_storage::StagedPrivateDirectory;
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::cell::Cell;
use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn temporary() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}
fn descriptor(file: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}/.", file.as_raw_fd()))
}
fn copy_tree(source: &Path, target: &Path) {
    fs::DirBuilder::new().mode(0o700).create(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), &destination).unwrap();
            fs::set_permissions(destination, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
}
fn setup(parent: &Path, count: usize, compact: bool) -> (PathBuf, Vec<(String, String)>, Vec<u8>) {
    let source = parent.join("source");
    let mut store = ProjectStore::open(&source).unwrap();
    let mut credentials = Vec::new();
    for index in 0..count {
        let created = store.create("synthetic coordinated project").unwrap();
        store
            .authorize(&created.project.id, &created.api_key)
            .unwrap()
            .execute(
                "CREATE TABLE t(id INT PRIMARY KEY,v INT); INSERT INTO t VALUES(1,$1)",
                &[Value::Integer(index as i64)],
            )
            .unwrap();
        if compact && index % 2 == 0 {
            Database::open(source.join(&created.project.id).join("data"))
                .unwrap()
                .compact()
                .unwrap();
        }
        credentials.push((created.project.id, created.api_key));
    }
    let bytes = store.backup_image().unwrap();
    (source, credentials, bytes)
}
fn descriptors(pid: u32, path: &Path) -> usize {
    let expected = fs::metadata(path).unwrap();
    fs::read_dir(format!("/proc/{pid}/fd"))
        .unwrap()
        .filter_map(|entry| fs::metadata(entry.unwrap().path()).ok())
        .filter(|actual| (actual.dev(), actual.ino()) == (expected.dev(), expected.ino()))
        .count()
}
fn owned(root: &Path, report: &RegistryBackupReport, pid: u32) {
    assert!(matches!(
        ProjectStore::open_existing(root),
        Err(Error::Busy)
    ));
    for project in &report.projects {
        let data = root.join(&project.id).join("data");
        assert!(Database::open(&data).is_err());
        assert!(matches!(
            File::open(&data).unwrap().try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        assert!(descriptors(pid, &data.join("redo.wal")) > 0);
    }
}
fn released(root: &Path, report: &RegistryBackupReport, pid: u32) {
    for project in &report.projects {
        assert_eq!(
            descriptors(pid, &root.join(&project.id).join("data/redo.wal")),
            0
        );
    }
    drop(ProjectStore::open_existing(root).unwrap());
}

#[test]
fn nested_prepared_registries_keep_every_actual_writer_until_common_selection_and_release() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let first = temporary();
    let second = temporary();
    let output = temporary();
    let (_, keys_a, a) = setup(first.path(), 2, true);
    let (_, keys_b, b) = setup(second.path(), 1, false);
    let report_a = crate::inspect_registry_backup_bytes(&a).unwrap();
    let report_b = crate::inspect_registry_backup_bytes(&b).unwrap();
    let target = output.path().join("common");
    let stage = StagedPrivateDirectory::new(&target).unwrap();
    let root = stage.directory().try_clone().unwrap();
    let path = descriptor(&root);
    let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
    let receipt = with_restored_registry_backup_bytes_at(&a, &root, "first", |first_report| {
        assert_eq!(first_report, &report_a);
        owned(&path.join("first"), first_report, std::process::id());
        assert!(!target.exists());
        with_restored_registry_backup_bytes_at(&b, &root, "second", |second_report| {
            assert_eq!(second_report, &report_b);
            owned(&path.join("first"), first_report, std::process::id());
            owned(&path.join("second"), second_report, std::process::id());
            let selected = stage.publish().unwrap();
            selected.check().unwrap();
            owned(&target.join("first"), first_report, std::process::id());
            owned(&target.join("second"), second_report, std::process::id());
            (first_report.clone(), second_report.clone())
        })
    })
    .unwrap()
    .unwrap();
    assert_eq!(receipt, (report_a.clone(), report_b.clone()));
    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline - 2);
    for (name, report, bytes, credentials) in [
        ("first", report_a, a, keys_a),
        ("second", report_b, b, keys_b),
    ] {
        let selected = target.join(name);
        released(&selected, &report, std::process::id());
        assert_eq!(image(&selected), bytes);
        let registry = ProjectStore::open_existing(&selected).unwrap();
        for (id, key) in credentials {
            registry
                .authorize(&id, &key)
                .unwrap()
                .execute("INSERT INTO t VALUES(2,43)", &[])
                .unwrap();
        }
    }
}

#[test]
fn callback_refusal_preserves_selected_child_and_all_healthy_owners_are_released() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let source = temporary();
    let output = temporary();
    let (path, _, bytes) = setup(source.path(), 2, true);
    let report = crate::inspect_registry_backup_bytes(&bytes).unwrap();
    let parent = File::open(output.path()).unwrap();
    let result = with_restored_registry_backup_bytes_at(&bytes, &parent, "registry", |report| {
        owned(&output.path().join("registry"), report, std::process::id());
        Err::<(), _>("synthetic caller refusal")
    })
    .unwrap();
    assert_eq!(result, Err("synthetic caller refusal"));
    released(&output.path().join("registry"), &report, std::process::id());
    assert_eq!(image(&output.path().join("registry")), bytes);
    assert_eq!(image(&path), bytes);
}

#[test]
fn parent_descriptor_survives_moves_and_never_adopts_an_old_path_replacement() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let source = temporary();
    let output = temporary();
    let (_, _, bytes) = setup(source.path(), 2, true);
    let parent_path = output.path().join("original");
    let moved = output.path().join("moved");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&parent_path)
        .unwrap();
    let parent = File::open(&parent_path).unwrap();
    with_restored_registry_backup_bytes_at(&bytes, &parent, "registry", |report| {
        fs::rename(&parent_path, &moved).unwrap();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&parent_path)
            .unwrap();
        fs::write(parent_path.join("keep"), b"foreign replacement parent").unwrap();
        owned(&moved.join("registry"), report, std::process::id());
    })
    .unwrap();
    assert_eq!(
        fs::read(parent_path.join("keep")).unwrap(),
        b"foreign replacement parent"
    );
    assert!(!parent_path.join("registry").exists());
    assert_eq!(image(&moved.join("registry")), bytes);
}

#[test]
fn invalid_scope_image_parent_and_leaf_refuse_before_creating_any_stage() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let source = temporary();
    let output = temporary();
    let (_, _, bytes) = setup(source.path(), 1, false);
    let parent = File::open(output.path()).unwrap();
    let long = "x".repeat(256);
    for name in ["", ".", "..", "../escape", "a/b", "nul\0leaf", &long] {
        assert!(
            with_restored_registry_backup_bytes_at(&bytes, &parent, name, |_| panic!(
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
            with_restored_registry_backup_bytes_at(bad, &parent, "registry", |_| panic!(
                "invalid image ran work"
            ))
            .is_err()
        );
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    }
    for mode in [0o755, 0o770, 0o1700, 0o2700, 0o4700] {
        fs::set_permissions(output.path(), fs::Permissions::from_mode(mode)).unwrap();
        assert!(
            with_restored_registry_backup_bytes_at(&bytes, &parent, "registry", |_| panic!(
                "nonprivate parent ran work"
            ))
            .is_err()
        );
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    }
    fs::set_permissions(output.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let maximal = "x".repeat(255);
    with_restored_registry_backup_bytes_at(&bytes, &parent, &maximal, |_| ()).unwrap();
    assert_eq!(image(&output.path().join(maximal)), bytes);
}

#[test]
fn existing_file_directory_and_symlink_preserve_original_targets_without_work() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let source = temporary();
    let (_, _, bytes) = setup(source.path(), 1, false);
    for kind in 0..3 {
        let output = temporary();
        let parent = File::open(output.path()).unwrap();
        let target = output.path().join("registry");
        match kind {
            0 => fs::write(&target, b"existing").unwrap(),
            1 => {
                fs::DirBuilder::new().mode(0o700).create(&target).unwrap();
                fs::write(target.join("keep"), b"existing directory").unwrap();
            }
            _ => std::os::unix::fs::symlink(source.path(), &target).unwrap(),
        }
        assert!(
            with_restored_registry_backup_bytes_at(&bytes, &parent, "registry", |_| panic!(
                "occupied registry ran work"
            ))
            .is_err()
        );
        match kind {
            0 => assert_eq!(fs::read(target).unwrap(), b"existing"),
            1 => assert_eq!(
                fs::read(target.join("keep")).unwrap(),
                b"existing directory"
            ),
            _ => assert_eq!(fs::read_link(target).unwrap(), source.path()),
        }
    }
}

#[derive(Debug)]
struct Receipt(Arc<AtomicUsize>);
impl Drop for Receipt {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn mutate(root: &Path, report: &RegistryBackupReport, saved: &Path, change: usize) {
    let project = root.join(&report.projects[0].id);
    let data = project.join("data");
    let wal = data.join("redo.wal");
    match change {
        0..=2 => {
            let path = match change {
                0 => root.to_path_buf(),
                1 => project,
                _ => data,
            };
            fs::rename(&path, saved).unwrap();
            fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            fs::write(path.join("keep"), b"foreign directory").unwrap();
        }
        3 => {
            fs::rename(&wal, saved).unwrap();
            fs::copy(saved, &wal).unwrap();
            fs::set_permissions(&wal, fs::Permissions::from_mode(0o600)).unwrap();
            assert!(descriptors(std::process::id(), saved) > 0);
        }
        4 => fs::write(wal, b"corrupted acknowledged history").unwrap(),
        5 => File::options()
            .append(true)
            .open(wal)
            .unwrap()
            .write_all(b"unacknowledged external tail")
            .unwrap(),
        6 => fs::hard_link(wal, saved).unwrap(),
        7 => fs::set_permissions(wal, fs::Permissions::from_mode(0o644)).unwrap(),
        8 => fs::write(root.join("unexpected"), b"foreign inventory member").unwrap(),
        9 => {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(".creating-foreign"))
                .unwrap();
        }
        10 => fs::set_permissions(root, fs::Permissions::from_mode(0o755)).unwrap(),
        _ => fs::write(project.join("project.json"), b"changed metadata").unwrap(),
    }
}

#[test]
fn late_substitutions_corruption_links_modes_and_tails_withhold_every_receipt() {
    let _serial = PROCESS_TESTS.blocking_lock();
    for count in [1, 2] {
        for change in 0..12 {
            let source = temporary();
            let output = temporary();
            let (source_path, _, bytes) = setup(source.path(), count, count == 2);
            let parent = File::open(output.path()).unwrap();
            let target = output.path().join("registry");
            let saved = output.path().join("original-saved");
            let drops = Arc::new(AtomicUsize::new(0));
            let result =
                with_restored_registry_backup_bytes_at(&bytes, &parent, "registry", |report| {
                    owned(&target, report, std::process::id());
                    mutate(&target, report, &saved, change);
                    Receipt(Arc::clone(&drops))
                });
            assert!(
                matches!(result, Err(Error::RegistryRestoreUnknown(_))),
                "change={change}, result={result:?}"
            );
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            assert!(target.exists());
            if change <= 3 || change == 6 {
                assert!(saved.exists());
            }
            assert_eq!(image(&source_path), bytes);
        }
    }
}

#[test]
fn appended_unacknowledged_tail_during_work_must_not_receive_a_restored_receipt() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let source = temporary();
    let output = temporary();
    let (_, _, bytes) = setup(source.path(), 1, false);
    let parent = File::open(output.path()).unwrap();
    let target = output.path().join("registry");
    let result = with_restored_registry_backup_bytes_at(&bytes, &parent, "registry", |report| {
        mutate(&target, report, &output.path().join("unused"), 5);
    });
    assert!(
        matches!(result, Err(Error::RegistryRestoreUnknown(_))),
        "result={result:?}"
    );
    assert!(target.is_dir());
}

#[test]
fn final_original_selection_is_checked_after_history_validation_before_owner_release() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let source = temporary();
    let output = temporary();
    let (_, _, bytes) = setup(source.path(), 2, true);
    let parent = File::open(output.path()).unwrap();
    let target = output.path().join("registry");
    let saved = output.path().join("original-registry");
    let report = crate::inspect_registry_backup_bytes(&bytes).unwrap();
    let callback_target = target.clone();
    let callback_saved = saved.clone();
    let action =
        crate::durability::on_boundary("registry_restore_coordinated_verifying", move || {
            owned(&callback_target, &report, std::process::id());
            fs::rename(&callback_target, &callback_saved).unwrap();
            copy_tree(&callback_saved, &callback_target);
        });
    let result = with_restored_registry_backup_bytes_at(&bytes, &parent, "registry", |_| ());
    drop(action);
    assert!(matches!(result, Err(Error::RegistryRestoreUnknown(_))));
    assert_eq!(image(&target), bytes);
    assert_eq!(image(&saved), bytes);
}

#[test]
fn a_changed_physical_wal_before_selection_keeps_target_absent_and_does_not_run_work() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let source = temporary();
    let output = temporary();
    let (source_path, _, bytes) = setup(source.path(), 2, true);
    let report = crate::inspect_registry_backup_bytes(&bytes).unwrap();
    let parent = File::open(output.path()).unwrap();
    let directory = output.path().to_path_buf();
    let action = crate::durability::on_boundary("registry_restore_coordinated_owned", move || {
        let staged = private_stage(&directory, ".emilybase-registry-restore-");
        owned(&staged, &report, std::process::id());
        mutate(&staged, &report, &directory.join("unused"), 5);
    });
    let calls = Cell::new(0);
    let result = with_restored_registry_backup_bytes_at(&bytes, &parent, "registry", |_| {
        calls.set(calls.get() + 1);
    });
    drop(action);
    assert!(matches!(result, Err(Error::Path)));
    assert_eq!(calls.get(), 0);
    assert!(!output.path().join("registry").exists());
    assert!(private_stage(output.path(), ".emilybase-registry-restore-").is_dir());
    assert_eq!(image(&source_path), bytes);
}

#[test]
fn changed_parent_or_special_permissions_during_work_withhold_success() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let source = temporary();
    let (_, _, bytes) = setup(source.path(), 1, false);
    for change in 0..4 {
        let output = temporary();
        let parent = File::open(output.path()).unwrap();
        let target = output.path().join("registry");
        let result =
            with_restored_registry_backup_bytes_at(&bytes, &parent, "registry", |report| {
                let (path, mode) = match change {
                    0 => (output.path().to_path_buf(), 0o775),
                    1 => (target.clone(), 0o1700),
                    2 => (target.join(&report.projects[0].id).join("data"), 0o775),
                    _ => (
                        target.join(&report.projects[0].id).join("data/redo.wal"),
                        0o1600,
                    ),
                };
                fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
            });
        assert!(
            matches!(result, Err(Error::RegistryRestoreUnknown(_))),
            "change={change},result={result:?}"
        );
        assert!(target.is_dir());
    }
}

#[test]
fn trusted_callback_unwinding_releases_original_writers_and_retains_selected_registry() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let source = temporary();
    let output = temporary();
    let (_, _, bytes) = setup(source.path(), 2, true);
    let parent = File::open(output.path()).unwrap();
    let report = crate::inspect_registry_backup_bytes(&bytes).unwrap();
    let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = with_restored_registry_backup_bytes_at(&bytes, &parent, "registry", |report| {
            owned(&output.path().join("registry"), report, std::process::id());
            panic!("synthetic trusted coordination unwind");
        });
    }));
    assert!(result.is_err());
    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
    released(&output.path().join("registry"), &report, std::process::id());
    assert_eq!(image(&output.path().join("registry")), bytes);
}

#[test]
fn sync_failures_before_and_after_child_selection_preserve_distinct_safe_outcomes() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let source = temporary();
    let (_, _, bytes) = setup(source.path(), 2, true);
    for phase in [
        "registry_restore_coordinated_stage_sync",
        "registry_restore_coordinated_stage_sync_after",
        "registry_restore_coordinated_parent_sync",
        "registry_restore_coordinated_parent_sync_after",
        "registry_restore_coordinated_confirm_sync",
        "registry_restore_coordinated_confirm_sync_after",
    ] {
        let output = temporary();
        let parent = File::open(output.path()).unwrap();
        let calls = Cell::new(0);
        inject(phase);
        let result = with_restored_registry_backup_bytes_at(&bytes, &parent, "registry", |_| {
            calls.set(calls.get() + 1);
        });
        let selected = !phase.contains("stage_sync");
        assert_eq!(output.path().join("registry").exists(), selected);
        if selected {
            assert!(matches!(result, Err(Error::RegistryRestoreUnknown(_))));
            assert_eq!(image(&output.path().join("registry")), bytes);
        } else {
            assert!(matches!(result, Err(Error::Io(_))));
        }
        assert_eq!(calls.get(), usize::from(phase.contains("confirm_sync")));
    }
}

#[test]
fn maximum_project_roster_holds128_original_public_writers_until_final_proof() {
    let _serial = PROCESS_TESTS.blocking_lock();
    let source = temporary();
    let output = temporary();
    let (_, _, bytes) = setup(source.path(), crate::MAX_PROJECTS, false);
    let parent = File::open(output.path()).unwrap();
    let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
    let report = with_restored_registry_backup_bytes_at(&bytes, &parent, "registry", |report| {
        assert_eq!(report.projects.len(), 128);
        owned(&output.path().join("registry"), report, std::process::id());
        report.clone()
    })
    .unwrap();
    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
    released(&output.path().join("registry"), &report, std::process::id());
    assert_eq!(image(&output.path().join("registry")), bytes);
}

pub(super) fn worker(archive: &Path, target: &Path) {
    let bytes = fs::read(archive).unwrap();
    let stage = StagedPrivateDirectory::new(target).unwrap();
    let parent = stage.directory().try_clone().unwrap();
    let report = with_restored_registry_backup_bytes_at(&bytes, &parent, "registry", |report| {
        owned(
            &descriptor(&parent).join("registry"),
            report,
            std::process::id(),
        );
        let selected = stage.publish().unwrap();
        selected.check().unwrap();
        owned(&target.join("registry"), report, std::process::id());
        checkpoint("registry_restore_coordinated_common_selected");
        report.clone()
    })
    .unwrap();
    released(&target.join("registry"), &report, std::process::id());
    checkpoint("registry_restore_coordinated_returned");
}

fn private_stage(parent: &Path, prefix: &str) -> PathBuf {
    let entries = fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(prefix))
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    entries[0].clone()
}

#[test]
fn actual_kills_preserve_private_preparations_or_complete_common_root_and_next_write() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = PROCESS_TESTS.blocking_lock();
    for kind in 0..3 {
        for phase in [
            "registry_restore_coordinated_owned",
            "registry_restore_coordinated_selected",
            "registry_restore_coordinated_ready",
            "registry_restore_coordinated_common_selected",
            "registry_restore_coordinated_verifying",
            "registry_restore_coordinated_returned",
        ] {
            let source = temporary();
            let output = temporary();
            let (source_path, credentials, bytes) = setup(source.path(), kind, kind == 2);
            let report = crate::inspect_registry_backup_bytes(&bytes).unwrap();
            let archive = output.path().join("archive");
            fs::write(&archive, &bytes).unwrap();
            let target = output.path().join("common");
            let mut child = Worker::start(
                &source_path,
                &archive,
                &target,
                "coordinated-restore",
                phase,
            );
            child.reach(phase);
            let outer_selected = matches!(
                phase,
                "registry_restore_coordinated_common_selected"
                    | "registry_restore_coordinated_verifying"
                    | "registry_restore_coordinated_returned"
            );
            assert_eq!(target.exists(), outer_selected);
            let root = if outer_selected {
                target.clone()
            } else {
                private_stage(output.path(), ".emilybase-directory-")
            };
            let registry = if phase == "registry_restore_coordinated_owned" {
                private_stage(&root, ".emilybase-registry-restore-")
            } else {
                root.join("registry")
            };
            if phase == "registry_restore_coordinated_returned" {
                released(&registry, &report, child.child.id());
            } else {
                owned(&registry, &report, child.child.id());
            }
            child.child.kill().unwrap();
            let status = child.child.wait().unwrap();
            child.reader.take().unwrap().join().unwrap();
            assert_eq!(status.signal(), Some(9));
            assert_eq!(image(&registry), bytes);
            assert_eq!(image(&source_path), bytes);
            assert_eq!(fs::read(&archive).unwrap(), bytes);
            let restored = ProjectStore::open_existing(&registry).unwrap();
            for (id, key) in credentials {
                let changed = restored
                    .authorize(&id, &key)
                    .unwrap()
                    .execute("INSERT INTO t VALUES(2,61)", &[])
                    .unwrap();
                assert_eq!(changed.transaction, 3);
            }
            drop(restored);
            with_restored_registry_backup_bytes_at(
                &bytes,
                &File::open(output.path()).unwrap(),
                "fresh-retry",
                |_| (),
            )
            .unwrap();
            assert_eq!(image(&output.path().join("fresh-retry")), bytes);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_prepared_rosters_match_independent_counts_keys_wal_and_row_models(count in 0usize..5,compact in any::<bool>()) {
        let _serial=PROCESS_TESTS.blocking_lock();let source=temporary();let output=temporary();let (source_path,credentials,bytes)=setup(source.path(),count,compact);let expected=crate::inspect_registry_backup_bytes(&bytes).unwrap();let parent=File::open(output.path()).unwrap();
        let report=with_restored_registry_backup_bytes_at(&bytes,&parent,"registry",|report|{owned(&output.path().join("registry"),report,std::process::id());report.clone()}).unwrap();prop_assert_eq!(&report,&expected);prop_assert_eq!(report.projects.len(),count);released(&output.path().join("registry"),&report,std::process::id());prop_assert_eq!(image(&output.path().join("registry")),bytes.clone());prop_assert_eq!(image(&source_path),bytes);
        let restored=ProjectStore::open_existing(output.path().join("registry")).unwrap();for (index,(id,key)) in credentials.iter().enumerate(){let query=restored.authorize(id,key).unwrap().execute("SELECT * FROM t",&[]).unwrap();prop_assert_eq!(query.results[0].rows.clone(),vec![vec![Value::Integer(1),Value::Integer(index as i64)]]);}
    }
}
