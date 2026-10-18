use super::*;
use crate::{
    backup_account_file_components, inspect_account_file_archive, restore_account_file_archive,
};

fn backup(source: &Source, target: &Path) -> crate::Result<crate::AccountFileArchiveReport> {
    let mut root = AccountRoot::open(&source.root.root, pool()).unwrap();
    let mut files = open(&source.paths);
    backup_account_file_components(&mut root, &mut files, target)
}
#[test]
fn coordinated_backup_matches_immutable_images_preserves_sessions_and_restores_independent_copies()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (1, false), (4, true)] {
        let t = tempfile::tempdir().unwrap();
        let s = source(t.path(), count, compact);
        let out = output(t.path());
        let target = out.join("общий архив 界");
        let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
        let report = backup(&s, &target).unwrap();
        assert_eq!(report, inspect_account_file_archive(&target).unwrap());
        assert_eq!(fs::read(&target).unwrap(), s.bytes);
        assert_eq!(fds(std::process::id(), &target), 0);
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
        verify_source(&s);
        let copied = out.join("copy");
        let restored = restore_account_file_archive(&target, &copied, pool(), 40).unwrap();
        verify_copy(&s, &copied, &restored);
        verify_source(&s);
    }
}
#[test]
fn actual_source_owners_and_selected_archive_span_each_final_proof_without_invented_payload_leases()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for point in [
        "joined_archive_selected",
        "root_capture_coordinated",
        "joined_backup_confirming",
    ] {
        let t = tempfile::tempdir().unwrap();
        let s = source(t.path(), 1, true);
        let out = output(t.path());
        let target = out.join("archive");
        let root = s.root.root.clone();
        let files = s.paths.clone();
        let selected = target.clone();
        let id = s.root.credentials[0].0.clone();
        let _hook = durability::on_boundary(point, move || {
            assert!(File::open(&root).unwrap().try_lock().is_err());
            assert!(ProjectStore::open_existing(root.join("registry")).is_err());
            assert!(AccountStore::open(root.join("private").join(&id), &id, pool()).is_err());
            assert_eq!(
                Database::open(root.join("registry").join(&id).join("data")).is_err(),
                point != "joined_backup_confirming",
                "{point}"
            );
            assert!(NativeFileRoot::open(&files[0], id.parse().unwrap()).is_err());
            assert!(fds(std::process::id(), &selected) > 0);
            for orphan in [false, true] {
                assert_eq!(
                    fds(std::process::id(), &object(&files[0], orphan)) > 0,
                    point == "joined_archive_selected"
                );
            }
        });
        backup(&s, &target).unwrap();
        verify_source(&s);
    }
}
#[test]
fn contained_destinations_refuse_before_staging_and_leave_original_sources_usable() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let s = source(t.path(), 1, false);
    let mut root = AccountRoot::open(&s.root.root, pool()).unwrap();
    let mut files = open(&s.paths);
    let id = &s.root.credentials[0].0;
    for directory in [
        s.root.root.clone(),
        s.root.root.join("private"),
        s.root.root.join("private").join(id),
        s.root.root.join("registry").join(id).join("data"),
        s.paths[0].clone(),
        s.paths[0].join("metadata"),
        s.paths[0].join("objects"),
    ] {
        let before = fs::read_dir(&directory).unwrap().count();
        assert!(
            backup_account_file_components(&mut root, &mut files, directory.join("archive"))
                .is_err()
        );
        assert_eq!(fs::read_dir(directory).unwrap().count(), before);
        assert_eq!(
            encode(&capture_account_file_components(&mut root, &mut files).unwrap()),
            s.bytes
        );
    }
    drop(files);
    drop(root);
    verify_source(&s);
}
#[test]
fn late_source_changes_discard_selected_receipts_and_preserve_archives_and_external_bytes() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for point in [
            "joined_archive_selected",
            "joined_archive_readback",
            "joined_archive_acked",
        ] {
            for combined in [false, true] {
                let t = tempfile::tempdir().unwrap();
                let s = source(t.path(), 1, compact);
                let out = output(t.path());
                let target = out.join("archive");
                let changed = if combined {
                    s.root
                        .root
                        .join("private")
                        .join(&s.root.credentials[0].0)
                        .join("redo.wal")
                } else {
                    object(&s.paths[0], true)
                };
                let original = fs::read(&changed).unwrap();
                let external = changed.clone();
                let selected = target.clone();
                let _hook = durability::on_boundary(point, move || {
                    File::options()
                        .append(true)
                        .open(&external)
                        .unwrap()
                        .write_all(b"external tail")
                        .unwrap();
                    if combined {
                        fs::set_permissions(selected, fs::Permissions::from_mode(0o644)).unwrap();
                    }
                });
                assert!(matches!(
                    backup(&s, &target),
                    Err(Error::ComponentPublicationUnknown(_))
                ));
                assert_eq!(fs::read(&target).unwrap(), s.bytes);
                let mut expected = original;
                expected.extend_from_slice(b"external tail");
                assert_eq!(fs::read(changed).unwrap(), expected);
            }
        }
    }
}
#[test]
fn selected_archive_is_not_rebound_after_inner_capture_and_source_error_cannot_hide_publication() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for point in ["root_capture_coordinated", "joined_backup_confirming"] {
        for kind in 0..3 {
            let t = tempfile::tempdir().unwrap();
            let s = source(t.path(), 1, false);
            let out = output(t.path());
            let target = out.join("archive");
            let selected = target.clone();
            let saved = out.join("original");
            let _hook = durability::on_boundary(point, move || match kind {
                0 => {
                    fs::rename(&selected, &saved).unwrap();
                    fs::copy(&saved, &selected).unwrap();
                    assert!(fds(std::process::id(), &saved) > 0);
                    assert_eq!(fds(std::process::id(), &selected), 0);
                }
                1 => File::options()
                    .append(true)
                    .open(selected)
                    .unwrap()
                    .write_all(b"external tail")
                    .unwrap(),
                _ => fs::set_permissions(selected, fs::Permissions::from_mode(0o644)).unwrap(),
            });
            assert!(matches!(
                backup(&s, &target),
                Err(Error::ComponentPublicationUnknown(_))
            ));
            verify_source(&s);
            assert!(target.exists());
        }
    }
}
#[test]
fn actual_backup_kills_keep_complete_selected_archives_unchanged_sources_and_released_descriptors()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (1, true)] {
        for point in [
            "joined_archive_prepared",
            "joined_archive_copying",
            "joined_archive_selected",
            "joined_archive_acked",
            "root_capture_coordinated",
            "joined_backup_ack",
        ] {
            let t = tempfile::tempdir().unwrap();
            let s = source(t.path(), count, compact);
            let out = output(t.path());
            let target = out.join("archive");
            let worker = Worker::start(&s.root.root, &target, "", "joined-backup", point);
            worker.reach(point);
            assert!(File::open(&s.root.root).unwrap().try_lock().is_err());
            let selected = !matches!(point, "joined_archive_prepared" | "joined_archive_copying");
            assert_eq!(target.exists(), selected);
            if selected {
                assert_eq!(fs::read(&target).unwrap(), s.bytes);
                assert_eq!(fds(worker.pid(), &target) > 0, point != "joined_backup_ack");
            }
            for path in &s.paths {
                for orphan in [false, true] {
                    assert_eq!(
                        fds(worker.pid(), &object(path, orphan)) > 0,
                        !matches!(point, "root_capture_coordinated" | "joined_backup_ack")
                    );
                }
            }
            worker.kill();
            verify_source(&s);
            if selected {
                inspect_account_file_archive(&target).unwrap();
            }
            backup(&s, &out.join("retry")).unwrap();
            verify_source(&s);
        }
    }
}
#[test]
fn original_native_source_parents_can_move_without_adopting_foreign_old_paths() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let work = t.path().join("work");
    fs::DirBuilder::new().mode(0o700).create(&work).unwrap();
    let mut s = source(&work, 1, true);
    let out = output(t.path());
    let target = out.join("archive");
    let moved = t.path().join("moved");
    let mut root = AccountRoot::open_at(&File::open(&work).unwrap(), "restored", pool()).unwrap();
    let file_parent = File::open(work.join("native-files")).unwrap();
    let id = s.root.credentials[0].0.clone();
    let mut files = vec![NativeFileRoot::open_at(&file_parent, &id, id.parse().unwrap()).unwrap()];
    drop(file_parent);
    let old = work.clone();
    let original = moved.clone();
    let _hook = durability::on_boundary("joined_archive_selected", move || {
        fs::rename(&old, &original).unwrap();
        super::super::super::super::at::copy_directory(&original, &old);
    });
    backup_account_file_components(&mut root, &mut files, &target).unwrap();
    drop(files);
    drop(root);
    assert_eq!(history(&s.root, &s.paths), s.before);
    s.root.root = moved.join("restored");
    s.paths = vec![moved.join("native-files").join(id)];
    verify_source(&s);
    assert_eq!(fs::read(target).unwrap(), s.bytes);
}
#[test]
fn publication_sync_failures_remain_uncertain_through_source_unwind_and_inspect_without_repair() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let s = source(t.path(), 1, true);
    let out = output(t.path());
    for point in [
        "joined_archive_file_ack_sync",
        "joined_archive_file_ack_sync_after",
        "joined_archive_parent_ack_sync",
        "joined_archive_parent_ack_sync_after",
    ] {
        let target = out.join(point);
        durability::inject(point);
        assert!(matches!(
            backup(&s, &target),
            Err(Error::ComponentPublicationUnknown(_))
        ));
        assert_eq!(fs::read(&target).unwrap(), s.bytes);
        inspect_account_file_archive(&target).unwrap();
        verify_source(&s);
    }
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_explicit_file_subsets_preserve_account_roster_without_discovery(count in 0usize..3,compact in any::<bool>(),include_files in any::<bool>()) {
        let _serial=durability::PROCESS_TESTS.blocking_lock();let t=tempfile::tempdir().unwrap();let s=source(t.path(),count,compact);let out=output(t.path());let target=out.join("archive");let mut root=AccountRoot::open(&s.root.root,pool()).unwrap();let mut files=if include_files{open(&s.paths)}else{Vec::new()};let expected=encode(&capture_account_file_components(&mut root,&mut files).unwrap());let report=backup_account_file_components(&mut root,&mut files,&target).unwrap();prop_assert_eq!(fs::read(&target).unwrap(),expected);prop_assert_eq!(report.manifest().private_projects().len(),count);prop_assert_eq!(report.manifest().file_stores().len(),if include_files{count}else{0});drop(files);drop(root);verify_source(&s);
    }
}
