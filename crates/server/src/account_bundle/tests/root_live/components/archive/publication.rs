use super::super::super::at::copy_directory;
use super::*;
use crate::{AccountFileSnapshot, inspect_account_file_archive, publish_account_file_archive};

fn captured(
    parent: &Path,
    count: usize,
    compact: bool,
) -> (Restored, Vec<PathBuf>, AccountFileSnapshot) {
    let (f, _, paths) = prepare(parent, count, compact);
    let mut root = AccountRoot::open(&f.root, pool()).unwrap();
    let mut files = open(&paths);
    let snapshot = capture_account_file_components(&mut root, &mut files).unwrap();
    (f, paths, snapshot)
}
fn output(parent: &Path) -> PathBuf {
    let path = parent.join("exports");
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    path
}
fn stages(path: &Path) -> Vec<PathBuf> {
    fs::read_dir(path)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".emilybase-create-")
        })
        .collect()
}
fn unknown<T>(result: crate::Result<T>) {
    assert!(matches!(result, Err(Error::ComponentPublicationUnknown(_))));
}

#[test]
fn private_streamed_publication_matches_exact_immutable_images_returns_only_counts_and_keeps_sources()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (1, false), (2, true)] {
        let temp = tempfile::tempdir().unwrap();
        let (f, paths, snapshot) = captured(temp.path(), count, compact);
        let before = history(&f, &paths);
        let directory = output(temp.path());
        let target = directory.join("snapshot");
        let expected = encode(&snapshot);
        let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
        let report = publish_account_file_archive(&snapshot, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), expected);
        assert_eq!(report, inspect_account_file_archive(&target).unwrap());
        assert_eq!(report.manifest(), snapshot.manifest());
        assert_eq!(report.physical_objects(), count * 2);
        assert_eq!(report.references(), count);
        assert_eq!(report.copied_bytes(), snapshot.copied_bytes());
        assert_eq!(report.encoded_bytes(), expected.len());
        assert_eq!(
            report.accounts().archive_bytes,
            snapshot.accounts().bundle_bytes().len()
        );
        assert!(!format!("{report:?}").contains("synthetic"));
        assert_eq!(report, report.clone());
        let meta = fs::metadata(&target).unwrap();
        assert_eq!(meta.permissions().mode() & 0o7777, 0o600);
        assert_eq!(meta.nlink(), 1);
        assert!(stages(&directory).is_empty());
        assert_eq!(fds(std::process::id(), &target), 0);
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
        assert_eq!(history(&f, &paths), before);
        fs::set_permissions(&target, fs::Permissions::from_mode(0o400)).unwrap();
        assert_eq!(inspect_account_file_archive(&target).unwrap(), report);
        assert_eq!(history(&f, &paths), before);
    }
}

#[test]
fn existing_targets_private_parent_admission_and_native_name_limits_never_overwrite_or_initialize()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let (f, paths, snapshot) = captured(temp.path(), 1, false);
    let before = history(&f, &paths);
    let directory = output(temp.path());
    let existing = directory.join("existing");
    fs::write(&existing, b"synthetic protected bytes").unwrap();
    fs::set_permissions(&existing, fs::Permissions::from_mode(0o600)).unwrap();
    let link = directory.join("link");
    symlink(&existing, &link).unwrap();
    let subdir = directory.join("directory");
    fs::DirBuilder::new().mode(0o700).create(&subdir).unwrap();
    for path in [&existing, &link, &subdir] {
        assert!(publish_account_file_archive(&snapshot, path).is_err());
        assert!(stages(&directory).is_empty());
    }
    assert_eq!(fs::read(&existing).unwrap(), b"synthetic protected bytes");
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(fs::read_dir(&subdir).unwrap().next().is_none());
    let missing = directory.join("missing/backup");
    assert!(publish_account_file_archive(&snapshot, &missing).is_err());
    assert!(!directory.join("missing").exists());
    for name in ["x".repeat(256), "bad\0name".into()] {
        assert!(publish_account_file_archive(&snapshot, directory.join(name)).is_err());
    }
    let target = directory.join("new");
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(publish_account_file_archive(&snapshot, &target).is_err());
    assert!(!target.exists());
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let longest = directory.join("x".repeat(255));
    publish_account_file_archive(&snapshot, &longest).unwrap();
    inspect_account_file_archive(&longest).unwrap();
    assert_eq!(history(&f, &paths), before);
}

#[test]
fn a_racing_target_is_preserved_without_success_or_unowned_cleanup() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for point in [
        "joined_archive_prepared",
        "joined_archive_copying",
        "joined_archive_copied",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let (f, paths, snapshot) = captured(temp.path(), 1, false);
        let before = history(&f, &paths);
        let directory = output(temp.path());
        let target = directory.join("snapshot");
        let copy = target.clone();
        let _guard = durability::on_boundary(point, move || {
            fs::write(&copy, b"foreign target").unwrap();
            fs::set_permissions(copy, fs::Permissions::from_mode(0o600)).unwrap();
        });
        assert!(publish_account_file_archive(&snapshot, &target).is_err());
        assert_eq!(fs::read(target).unwrap(), b"foreign target");
        assert!(stages(&directory).is_empty());
        assert_eq!(history(&f, &paths), before);
    }
}

#[test]
fn privacy_or_parent_rebinding_during_actual_copy_refuses_before_selection_and_keeps_foreign_stages()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for point in [
        "joined_archive_prepared",
        "joined_archive_copying",
        "joined_archive_copied",
    ] {
        for moved in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let (f, paths, snapshot) = captured(temp.path(), 1, false);
            let before = history(&f, &paths);
            let directory = output(temp.path());
            let target = directory.join("snapshot");
            let saved = temp.path().join("moved");
            let path = directory.clone();
            let moved_to = saved.clone();
            let _guard = durability::on_boundary(point, move || {
                if moved {
                    fs::rename(&path, &moved_to).unwrap();
                    copy_directory(&moved_to, &path);
                } else {
                    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
                }
            });
            assert!(publish_account_file_archive(&snapshot, &target).is_err());
            assert!(!target.exists());
            assert_eq!(history(&f, &paths), before);
            if moved {
                assert!(!saved.join("snapshot").exists());
                assert!(stages(&saved).is_empty());
                assert_eq!(
                    stages(&directory).len(),
                    usize::from(point != "joined_archive_prepared")
                );
            } else {
                fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
                assert!(stages(&directory).is_empty());
            }
        }
    }
}

#[test]
fn every_late_selected_change_withholds_ack_and_preserves_the_actual_or_foreign_files() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for point in [
        "joined_archive_selected",
        "joined_archive_readback",
        "joined_archive_acked",
    ] {
        for kind in 0..8 {
            let temp = tempfile::tempdir().unwrap();
            let (f, paths, snapshot) = captured(temp.path(), 1, false);
            let before = history(&f, &paths);
            let directory = output(temp.path());
            let target = directory.join("snapshot");
            let saved = temp.path().join("saved");
            let selected = target.clone();
            let saved_to = saved.clone();
            let parent = directory.clone();
            let expected = encode(&snapshot);
            let _guard = durability::on_boundary(point, move || {
                assert!(fds(std::process::id(), &selected) > 0);
                match kind {
                    0 => {
                        fs::rename(&selected, &saved_to).unwrap();
                        fs::copy(&saved_to, &selected).unwrap();
                        assert!(fds(std::process::id(), &saved_to) > 0);
                        assert_eq!(fds(std::process::id(), &selected), 0);
                    }
                    1 => {
                        let mut bytes = fs::read(&selected).unwrap();
                        *bytes.last_mut().unwrap() ^= 1;
                        fs::write(&selected, bytes).unwrap();
                    }
                    2 => fs::hard_link(&selected, &saved_to).unwrap(),
                    3 => fs::set_permissions(&selected, fs::Permissions::from_mode(0o644)).unwrap(),
                    4 => {
                        fs::rename(&parent, &saved_to).unwrap();
                        copy_directory(&saved_to, &parent);
                    }
                    5 => fs::rename(&selected, &saved_to).unwrap(),
                    6 => {
                        use std::io::Write;
                        File::options()
                            .append(true)
                            .open(&selected)
                            .unwrap()
                            .write_all(b"tail")
                            .unwrap();
                    }
                    _ => fs::set_permissions(&selected, fs::Permissions::from_mode(0o400)).unwrap(),
                }
            });
            unknown(publish_account_file_archive(&snapshot, &target));
            assert_eq!(history(&f, &paths), before);
            assert!(stages(&directory).is_empty());
            if matches!(kind, 0 | 2 | 4 | 5) {
                assert!(saved.exists());
            }
            if matches!(kind, 0 | 4 | 7) {
                assert_eq!(fs::read(&target).unwrap(), expected);
                inspect_account_file_archive(&target).unwrap();
            }
            if target.exists() {
                assert_eq!(fds(std::process::id(), &target), 0);
            }
        }
    }
}

#[test]
fn failed_final_file_and_parent_syncs_are_uncertain_and_recover_by_readonly_inspection() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for point in [
        "joined_archive_file_ack_sync",
        "joined_archive_file_ack_sync_after",
        "joined_archive_parent_ack_sync",
        "joined_archive_parent_ack_sync_after",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let (f, paths, snapshot) = captured(temp.path(), 1, true);
        let before = history(&f, &paths);
        let directory = output(temp.path());
        let target = directory.join("snapshot");
        durability::inject(point);
        unknown(publish_account_file_archive(&snapshot, &target));
        let report = inspect_account_file_archive(&target).unwrap();
        assert_eq!(report.encoded_bytes(), encode(&snapshot).len());
        assert_eq!(fs::read(&target).unwrap(), encode(&snapshot));
        assert!(publish_account_file_archive(&snapshot, &target).is_err());
        assert_eq!(history(&f, &paths), before);
        assert!(stages(&directory).is_empty());
    }
}

#[test]
fn readonly_inspection_never_adopts_same_bytes_replacement_or_repairs_changed_sources() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for point in ["joined_archive_input_read", "joined_archive_input_verified"] {
        for kind in 0..6 {
            let temp = tempfile::tempdir().unwrap();
            let (f, paths, snapshot) = captured(temp.path(), 1, false);
            let before = history(&f, &paths);
            let directory = output(temp.path());
            let target = directory.join("snapshot");
            publish_account_file_archive(&snapshot, &target).unwrap();
            let path = target.clone();
            let saved = temp.path().join("saved");
            let saved_to = saved.clone();
            let parent = directory.clone();
            let _guard = durability::on_boundary(point, move || {
                assert!(fds(std::process::id(), &path) > 0);
                match kind {
                    0 => {
                        fs::rename(&path, &saved_to).unwrap();
                        fs::copy(&saved_to, &path).unwrap();
                    }
                    1 => {
                        let mut bytes = fs::read(&path).unwrap();
                        *bytes.last_mut().unwrap() ^= 1;
                        fs::write(&path, bytes).unwrap();
                    }
                    2 => fs::hard_link(&path, &saved_to).unwrap(),
                    3 => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
                    4 => {
                        fs::rename(&parent, &saved_to).unwrap();
                        copy_directory(&saved_to, &parent);
                    }
                    _ => fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap(),
                }
            });
            assert!(inspect_account_file_archive(&target).is_err());
            assert_eq!(history(&f, &paths), before);
            if kind < 5 && kind != 1 && kind != 3 {
                assert!(saved.exists());
            }
            if kind == 5 {
                fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
            }
            assert_eq!(fds(std::process::id(), &target), 0);
        }
    }
}

#[test]
fn inspection_refuses_links_fifo_directory_broad_mode_oversize_and_invalid_bytes_without_writes() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let (_, _, snapshot) = captured(temp.path(), 1, false);
    let directory = output(temp.path());
    let target = directory.join("snapshot");
    publish_account_file_archive(&snapshot, &target).unwrap();
    let expected = fs::read(&target).unwrap();
    let link = directory.join("link");
    symlink(&target, &link).unwrap();
    assert!(inspect_account_file_archive(&link).is_err());
    let alias = directory.join("alias");
    fs::hard_link(&target, &alias).unwrap();
    assert!(inspect_account_file_archive(&target).is_err());
    fs::remove_file(alias).unwrap();
    assert_eq!(fs::read(&target).unwrap(), expected);
    assert!(inspect_account_file_archive(&directory).is_err());
    let fifo = directory.join("fifo");
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .unwrap();
    assert!(inspect_account_file_archive(&fifo).is_err());
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(inspect_account_file_archive(&target).is_err());
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    File::options()
        .write(true)
        .open(&target)
        .unwrap()
        .set_len(crate::MAX_ACCOUNT_FILE_ARCHIVE_BYTES as u64 + 1)
        .unwrap();
    assert!(matches!(
        inspect_account_file_archive(&target),
        Err(Error::Limit)
    ));
    fs::write(&target, b"invalid").unwrap();
    assert!(inspect_account_file_archive(&target).is_err());
    assert_eq!(fs::read(target).unwrap(), b"invalid");
}

#[test]
fn killed_joined_publishers_never_expose_partial_targets_and_release_actual_selected_owners() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (1, false), (2, true)] {
        for point in [
            "joined_archive_prepared",
            "joined_archive_copying",
            "joined_archive_copied",
            "joined_archive_selected",
            "joined_archive_readback",
            "joined_archive_acked",
            "joined_archive_returned",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let (f, paths, snapshot) = captured(temp.path(), count, compact);
            let before = history(&f, &paths);
            let directory = output(temp.path());
            let target = directory.join("snapshot");
            let expected = encode(&snapshot);
            let worker = Worker::start(&f.root, &target, "", "joined-publish", point);
            worker.reach(point);
            assert!(File::open(&f.root).unwrap().try_lock().is_err());
            let selected = matches!(
                point,
                "joined_archive_selected"
                    | "joined_archive_readback"
                    | "joined_archive_acked"
                    | "joined_archive_returned"
            );
            assert_eq!(target.exists(), selected);
            if selected {
                assert!(stages(&directory).is_empty());
                assert_eq!(fs::read(&target).unwrap(), expected);
                assert_eq!(
                    fds(worker.pid(), &target) > 0,
                    point != "joined_archive_returned"
                );
            } else {
                let paths = stages(&directory);
                assert_eq!(paths.len(), usize::from(point != "joined_archive_prepared"));
                if let Some(stage) = paths.first() {
                    assert!(fds(worker.pid(), stage) > 0);
                    assert_eq!(
                        fs::metadata(stage).unwrap().permissions().mode() & 0o7777,
                        0o600
                    );
                    if point == "joined_archive_copied" {
                        assert_eq!(fs::read(stage).unwrap(), expected);
                    }
                }
            }
            worker.kill();
            assert_eq!(history(&f, &paths), before);
            if selected {
                assert_eq!(
                    inspect_account_file_archive(&target)
                        .unwrap()
                        .physical_objects(),
                    count * 2
                );
            }
            let mut root = AccountRoot::open(&f.root, pool()).unwrap();
            let mut files = open(&paths);
            let next = capture_account_file_components(&mut root, &mut files).unwrap();
            assert_eq!(encode(&next), expected);
            publish_account_file_archive(&next, directory.join("after")).unwrap();
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_published_native_payloads_match_independent_content_and_sources(compact in any::<bool>(),payload in prop::collection::vec(any::<u8>(),0..8194)) {
        let _serial=durability::PROCESS_TESTS.blocking_lock();let temp=tempfile::tempdir().unwrap();let(f,_,paths)=prepare(temp.path(),1,compact);let mut root=AccountRoot::open(&f.root,pool()).unwrap();let mut files=open(&paths);files[0].publish(FileId::from_bytes([6;16]),ObjectId::from_bytes([7;16]),[4;16],"generated",&payload).unwrap();let snapshot=capture_account_file_components(&mut root,&mut files).unwrap();let before=history(&f,&paths);let directory=output(temp.path());let target=directory.join("snapshot");let report=publish_account_file_archive(&snapshot,&target).unwrap();prop_assert_eq!(inspect_account_file_archive(&target).unwrap(),report);let bytes=fs::read(&target).unwrap();let view=verify_account_file_archive(&bytes).unwrap();prop_assert_eq!(view.files()[0].objects().objects().iter().find(|o|o.object()==ObjectId::from_bytes([7;16])).unwrap().payload(),payload.as_slice());prop_assert_eq!(history(&f,&paths),before);
    }
}
