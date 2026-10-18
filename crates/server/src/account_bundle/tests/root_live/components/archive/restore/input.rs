use super::*;
use crate::restore_account_file_archive;
use std::os::unix::fs::OpenOptionsExt;

fn input(parent: &Path, bytes: &[u8]) -> PathBuf {
    let path = parent.join("вход 界 с пробелами.backup");
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    path
}

#[test]
fn retained_path_restores_readonly_and_writable_inputs_without_source_or_descriptor_changes() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact, readonly) in [(0, false, false), (1, false, true), (2, true, false)] {
        let temp = tempfile::tempdir().unwrap();
        let original = source(temp.path(), count, compact);
        let archive = input(temp.path(), &original.bytes);
        if readonly {
            fs::set_permissions(&archive, fs::Permissions::from_mode(0o400)).unwrap();
        }
        let out = output(temp.path());
        let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
        let report =
            restore_account_file_archive(&archive, out.join("копия 界"), pool(), 40).unwrap();
        assert_eq!(fds(std::process::id(), &archive), 0);
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
        assert_eq!(fs::read(&archive).unwrap(), original.bytes);
        assert_eq!(
            fs::metadata(&archive).unwrap().mode() & 0o7777,
            if readonly { 0o400 } else { 0o600 }
        );
        verify_copy(&original, &out.join("копия 界"), &report);
        verify_source(&original);
    }
}

fn change_source(path: &Path, container: &Path, change: usize) {
    match change {
        0 => {
            let saved = container.join("original-source");
            fs::rename(path, &saved).unwrap();
            fs::copy(saved, path).unwrap();
        }
        1 => File::options()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(b"external source tail")
            .unwrap(),
        2 => fs::write(path, b"externally changed source").unwrap(),
        3 => fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap(),
        4 => fs::hard_link(path, container.join("source-link")).unwrap(),
        _ => {
            let parent = path.parent().unwrap();
            fs::rename(parent, container.join("original-parent")).unwrap();
            fs::DirBuilder::new().mode(0o700).create(parent).unwrap();
            fs::write(path, b"foreign source parent").unwrap();
        }
    }
}

#[test]
fn original_source_changes_refuse_before_selection_or_withhold_late_selected_receipts() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let original = source(temp.path(), 1, compact);
        for phase in [
            "joined_restore_input_read",
            "joined_restore_input_confirmed",
            "joined_restore_prepared",
            "joined_restore_selected",
            "joined_restore_finalizing",
        ] {
            for change in 0..6 {
                let container = temp.path().join(format!("{phase}-{change}"));
                fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&container)
                    .unwrap();
                let incoming = container.join("incoming");
                fs::DirBuilder::new().mode(0o700).create(&incoming).unwrap();
                let archive = input(&incoming, &original.bytes);
                let out = output(&container);
                let target = out.join("copy");
                let selected = matches!(
                    phase,
                    "joined_restore_selected" | "joined_restore_finalizing"
                );
                let observed = archive.clone();
                let copies = container.clone();
                let visible = target.clone();
                let _hook = durability::on_boundary(phase, move || {
                    assert!(fds(std::process::id(), &observed) >= 1);
                    if selected {
                        assert!(File::open(&visible).unwrap().try_lock().is_err());
                    }
                    change_source(&observed, &copies, change);
                });
                let error =
                    restore_account_file_archive(&archive, &target, pool(), 40).unwrap_err();
                assert_eq!(
                    matches!(error, Error::ComponentRestoreUnknown(_)),
                    selected,
                    "phase={phase},change={change},error={error:?}"
                );
                assert_eq!(target.exists(), selected);
                if change == 2 {
                    assert_eq!(fs::read(&archive).unwrap(), b"externally changed source");
                }
                if change == 5 {
                    assert_eq!(fs::read(&archive).unwrap(), b"foreign source parent");
                }
                verify_source(&original);
            }
        }
    }
}

#[test]
fn source_reader_and_common_lock_span_the_last_confirmation_after_component_unwind() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let original = source(temp.path(), 1, true);
    let archive = input(temp.path(), &original.bytes);
    let out = output(temp.path());
    let target = out.join("copy");
    let observed = archive.clone();
    let visible = target.clone();
    let id = original.root.credentials[0].0.clone();
    let _hook = durability::on_boundary("joined_restore_finalizing", move || {
        assert!(fds(std::process::id(), &observed) >= 1);
        assert!(File::open(&visible).unwrap().try_lock().is_err());
        let private = visible.join("private").join(&id);
        drop(Database::open(&private).unwrap());
        assert!(fds(std::process::id(), &private.join("redo.wal")) >= 1);
    });
    let report = restore_account_file_archive(&archive, &target, pool(), 40).unwrap();
    assert_eq!(fds(std::process::id(), &archive), 0);
    File::open(&target).unwrap().try_lock().unwrap();
    verify_copy(&original, &target, &report);
    verify_source(&original);
}

#[test]
fn source_path_stays_checked_while_original_destination_parent_moves_without_adoption() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let original = source(temp.path(), 1, true);
    let archive = input(temp.path(), &original.bytes);
    let out = output(temp.path());
    let moved = temp.path().join("moved");
    let old = out.clone();
    let new = moved.clone();
    let _hook = durability::on_boundary("joined_restore_selected", move || {
        fs::rename(&old, &new).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&old).unwrap();
        fs::write(old.join("keep"), b"foreign destination parent").unwrap();
    });
    let report = restore_account_file_archive(&archive, out.join("copy"), pool(), 40).unwrap();
    assert!(!out.join("copy").exists());
    assert_eq!(
        fs::read(out.join("keep")).unwrap(),
        b"foreign destination parent"
    );
    verify_copy(&original, &moved.join("copy"), &report);
    assert_eq!(fs::read(&archive).unwrap(), original.bytes);
    verify_source(&original);
}

#[test]
fn source_privacy_type_size_invalid_bytes_and_target_overlap_never_publish_or_repair() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let original = source(temp.path(), 0, false);
    let archive = input(temp.path(), &original.bytes);
    let out = output(temp.path());
    for mode in [0o644, 0o666, 0o4600] {
        fs::set_permissions(&archive, fs::Permissions::from_mode(mode)).unwrap();
        assert!(restore_account_file_archive(&archive, out.join("copy"), pool(), 40).is_err());
        assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
    }
    fs::set_permissions(&archive, fs::Permissions::from_mode(0o600)).unwrap();
    let alias = temp.path().join("source-alias");
    symlink(&archive, &alias).unwrap();
    let linked = temp.path().join("source-hardlink");
    fs::hard_link(&archive, &linked).unwrap();
    for path in [&alias, &linked, &out] {
        assert!(restore_account_file_archive(path, out.join("copy"), pool(), 40).is_err());
        assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
    }
    fs::remove_file(linked).unwrap();
    let sparse = temp.path().join("oversized-source");
    File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&sparse)
        .unwrap()
        .set_len(crate::MAX_ACCOUNT_FILE_ARCHIVE_BYTES as u64 + 1)
        .unwrap();
    assert!(restore_account_file_archive(&sparse, out.join("copy"), pool(), 40).is_err());
    assert_eq!(
        fs::metadata(&sparse).unwrap().len(),
        crate::MAX_ACCOUNT_FILE_ARCHIVE_BYTES as u64 + 1
    );
    let bad = input(&out, b"invalid immutable image");
    assert!(restore_account_file_archive(&bad, out.join("copy"), pool(), 40).is_err());
    assert!(!out.join("copy").exists());
    assert!(restore_account_file_archive(&archive, &archive, pool(), 40).is_err());
    assert_eq!(fs::read(&archive).unwrap(), original.bytes);
    assert!(restore_account_file_archive("missing", out.join("copy"), pool(), u64::MAX).is_err());
    assert!(!out.join("copy").exists());
    verify_source(&original);
}

#[test]
fn actual_path_kills_release_source_and_common_owners_before_and_after_receipt() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (1, true)] {
        let temp = tempfile::tempdir().unwrap();
        let original = source(temp.path(), count, compact);
        let archive = input(temp.path(), &original.bytes);
        for phase in [
            "joined_restore_input_read",
            "joined_restore_prepared",
            "joined_restore_selected",
            "joined_restore_finalizing",
            "joined_restore_path_ack",
        ] {
            let out = temp.path().join(phase);
            fs::DirBuilder::new().mode(0o700).create(&out).unwrap();
            let target = out.join("copy");
            let worker = Worker::start(&archive, &target, "", "joined-restore-path", phase);
            worker.reach(phase);
            let received = phase == "joined_restore_path_ack";
            assert_eq!(fds(worker.pid(), &archive) == 0, received);
            let selected = matches!(
                phase,
                "joined_restore_selected" | "joined_restore_finalizing" | "joined_restore_path_ack"
            );
            assert_eq!(target.exists(), selected);
            if phase != "joined_restore_input_read" {
                let root = if selected {
                    target.clone()
                } else {
                    common_stage(&out)
                };
                if received {
                    File::open(root).unwrap().try_lock().unwrap();
                } else {
                    assert!(File::open(root).unwrap().try_lock().is_err());
                }
            } else {
                assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
            }
            worker.kill();
            assert_eq!(fs::read(&archive).unwrap(), original.bytes);
            let report =
                restore_account_file_archive(&archive, out.join("fresh"), pool(), 40).unwrap();
            verify_copy(&original, &out.join("fresh"), &report);
            verify_source(&original);
        }
    }
}

fn generated_input(empty: bool, compact: bool, readonly: bool) {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let original = source(temp.path(), usize::from(!empty), compact);
    let archive = input(temp.path(), &original.bytes);
    if readonly {
        fs::set_permissions(&archive, fs::Permissions::from_mode(0o400)).unwrap();
    }
    let out = output(temp.path());
    let report = restore_account_file_archive(&archive, out.join("copy"), pool(), 40).unwrap();
    assert_eq!(report.private_accounts.len(), usize::from(!empty));
    assert_eq!(report.manifest.reset_at(), 40);
    assert_eq!(fs::read(&archive).unwrap(), original.bytes);
    verify_copy(&original, &out.join("copy"), &report);
    verify_source(&original);
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_retained_inputs_preserve_sources_and_create_independent_copies(empty in any::<bool>(), compact in any::<bool>(), readonly in any::<bool>()) {
        generated_input(empty, compact, readonly);
    }
}
