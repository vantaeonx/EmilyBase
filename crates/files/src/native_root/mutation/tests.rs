use super::*;
use crate::native_root::publication_tests::{changed, components, copy_directory, fixture};
use crate::{FileArchiveReader, TEST_IO, restore_file_archive};
use proptest::prelude::*;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FILE: FileId = FileId::from_bytes([2; 16]);
fn operation(root: &mut NativeFileRoot, kind: u8) -> Operation<'static> {
    match kind {
        0 => Operation::Rename(FILE, 3, "renamed-private"),
        1 => Operation::Remove(FILE, 3),
        _ => Operation::Quota(root.quota_state().unwrap(), FileQuota::new(2, 9).unwrap()),
    }
}
fn assert_state(root: &mut NativeFileRoot, kind: u8, committed: bool) {
    let state = root.inspect().unwrap();
    assert_eq!(state.revision(), if committed { 4 } else { 3 });
    assert_eq!(state.usage().physical_objects, 2);
    assert_eq!(state.usage().payload_bytes, 9);
    assert_eq!(
        state.usage().references,
        usize::from(!(kind == 1 && committed))
    );
    assert_eq!(
        state.usage().orphans,
        if kind == 1 && committed { 2 } else { 1 }
    );
    assert_eq!(
        state.quota(),
        FileQuota::new(
            if kind == 2 && committed { 2 } else { 8 },
            if kind == 2 && committed { 9 } else { 32768 }
        )
        .unwrap()
    );
    if let Some(info) = root.file_info(FILE).unwrap() {
        assert_eq!(
            info.name(),
            if kind == 0 && committed {
                "renamed-private"
            } else {
                "existing"
            }
        );
        assert_eq!(info.revision(), if kind == 0 && committed { 4 } else { 3 });
    } else {
        assert!(kind == 1 && committed);
    }
}

#[test]
fn native_metadata_cas_noops_and_orphan_charge_survive_reopen_and_paired_restore() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let path = fixture(temp.path(), compacted);
        let descriptors = fs::read_dir("/proc/self/fd").unwrap().count();
        let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
        let old = root.quota_state().unwrap();
        let original = root.file_info(FILE).unwrap().unwrap();
        let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
        assert_eq!(root.rename(FILE, 3, "existing").unwrap(), original);
        assert_eq!(root.set_quota(old, old.quota()).unwrap(), old);
        assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
        let renamed = root.rename(FILE, 3, "renamed-private").unwrap();
        assert_eq!(renamed.revision(), 4);
        assert_eq!(renamed.owner(), original.owner());
        assert_eq!(renamed.report(), original.report());
        assert_eq!(renamed.object(), original.object());
        for error in [
            root.rename(FILE, 3, "renamed-private").map(|_| ()),
            root.remove(FILE, 3).map(|_| ()),
            root.set_quota(old, old.quota()).map(|_| ()),
        ] {
            assert!(matches!(error, Err(Error::Conflict)));
        }
        let removed = root.remove(FILE, 4).unwrap();
        assert_eq!(removed.removed(), &renamed);
        assert_eq!(removed.revision(), 5);
        assert!(matches!(root.remove(FILE, 4), Err(Error::Missing)));
        let expected = root.quota_state().unwrap();
        for quota in [FileQuota::new(1, 9).unwrap(), FileQuota::new(2, 8).unwrap()] {
            assert!(root.set_quota(expected, quota).is_err());
            assert_eq!(root.quota_state().unwrap(), expected);
        }
        let quota = root
            .set_quota(expected, FileQuota::new(2, 9).unwrap())
            .unwrap();
        assert_eq!(quota.revision(), 6);
        assert_eq!(root.inspect().unwrap().usage().orphans, 2);
        assert!(matches!(
            root.publish(FILE, ObjectId::from_bytes([8; 16]), [4; 16], "reuse", &[]),
            Err(Error::Objects(emilybase_object_storage::Error::Limit))
        ));
        let snapshot = root.capture().unwrap();
        assert_eq!(
            snapshot.metadata_report().wal_version,
            if compacted { 2 } else { 1 }
        );
        assert_eq!(
            snapshot
                .objects()
                .objects()
                .iter()
                .map(|o| o.payload().len())
                .sum::<usize>(),
            9
        );
        let info = root.inspect().unwrap();
        let mut bytes = Vec::new();
        FileArchiveReader::from_snapshot(&snapshot)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        drop(root);
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), descriptors);
        assert_eq!(
            NativeFileRoot::open(&path, PROJECT)
                .unwrap()
                .inspect()
                .unwrap(),
            info
        );
        restore_file_archive(&bytes, PROJECT, temp.path().join("copy")).unwrap();
        assert_eq!(
            NativeFileRoot::open(temp.path().join("copy"), PROJECT)
                .unwrap()
                .inspect()
                .unwrap(),
            info
        );
    }
}

#[test]
fn foreign_database_cas_invalid_names_and_missing_ids_preserve_usable_exact_source() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = fixture(temp.path(), false);
    let other = tempfile::tempdir().unwrap();
    let foreign = NativeFileRoot::open(fixture(other.path(), false), PROJECT)
        .unwrap()
        .quota_state()
        .unwrap();
    let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
    let before = root.inspect().unwrap();
    let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
    assert!(matches!(
        root.set_quota(foreign, foreign.quota()),
        Err(Error::Scope)
    ));
    for name in ["", "\n", &"x".repeat(crate::MAX_FILE_NAME_BYTES + 1)] {
        assert!(matches!(root.rename(FILE, 3, name), Err(Error::Name)));
    }
    assert!(matches!(
        root.rename(FileId::from_bytes([9; 16]), 3, "absent"),
        Err(Error::Missing)
    ));
    assert_eq!(root.file_info(FileId::from_bytes([9; 16])).unwrap(), None);
    assert_eq!(root.inspect().unwrap(), before);
    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
}

#[test]
fn selected_source_substitutions_abort_staged_metadata_and_preserve_later_commits() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for kind in 0..3 {
            for phase in [
                MetadataBoundary::Staged,
                MetadataBoundary::Committed,
                MetadataBoundary::Verified,
            ] {
                for change in 0..7 {
                    let temp = tempfile::tempdir().unwrap();
                    let path = fixture(temp.path(), compacted);
                    let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                    let op = operation(&mut root, kind);
                    let result = root.edit_with(op, |at| {
                        if at == phase {
                            changed(&path, &temp.path().join("saved"), change)
                        }
                    });
                    assert!(result.is_err());
                    assert_eq!(
                        matches!(result, Err(Error::OutcomeUnknown(_))),
                        phase != MetadataBoundary::Staged
                    );
                    assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                    drop(root);
                    let mut store = components(&path);
                    let snapshot = store.capture().unwrap();
                    assert_eq!(
                        snapshot.metadata_report().last_transaction,
                        if phase == MetadataBoundary::Staged {
                            3
                        } else {
                            4
                        }
                    );
                    assert_eq!(snapshot.objects().objects().len(), 2);
                    let committed = phase != MetadataBoundary::Staged;
                    let info = store.info(FILE).unwrap();
                    assert_eq!(info.is_none(), kind == 1 && committed);
                    if let Some(info) = info {
                        assert_eq!(
                            info.name(),
                            if kind == 0 && committed {
                                "renamed-private"
                            } else {
                                "existing"
                            }
                        );
                    }
                    assert_eq!(
                        store.quota_state().unwrap().quota(),
                        FileQuota::new(
                            if kind == 2 && committed { 2 } else { 8 },
                            if kind == 2 && committed { 9 } else { 32768 }
                        )
                        .unwrap()
                    );
                }
            }
        }
    }
}

#[test]
fn moved_parent_is_rejected_and_never_redirects_metadata_writes() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for kind in 0..3 {
            for phase in [
                MetadataBoundary::Staged,
                MetadataBoundary::Committed,
                MetadataBoundary::Verified,
            ] {
                let temp = tempfile::tempdir().unwrap();
                let parent = temp.path().join("parent");
                fs::create_dir(&parent).unwrap();
                let path = fixture(&parent, compacted);
                let moved = temp.path().join("moved");
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                let op = operation(&mut root, kind);
                let result = root.edit_with(op, |at| {
                    if at == phase {
                        fs::rename(&parent, &moved).unwrap();
                        fs::create_dir(&parent).unwrap();
                        copy_directory(&moved.join("root"), &path);
                    }
                });
                assert_eq!(
                    matches!(result, Err(Error::OutcomeUnknown(_))),
                    phase != MetadataBoundary::Staged
                );
                assert!(result.is_err());
                drop(root);
                for selected in [&path, &moved.join("root")] {
                    assert_state(
                        &mut NativeFileRoot::open(selected, PROJECT).unwrap(),
                        kind,
                        phase != MetadataBoundary::Staged,
                    );
                }
            }
        }
    }
}

#[test]
fn inplace_wal_changes_before_commit_and_after_ack_are_preserved_and_poison_owner() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for kind in 0..3 {
            for phase in [MetadataBoundary::Staged, MetadataBoundary::Committed] {
                for change in 0..4 {
                    let temp = tempfile::tempdir().unwrap();
                    let path = fixture(temp.path(), compacted);
                    let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                    let op = operation(&mut root, kind);
                    let wal = path.join("metadata/redo.wal");
                    let mut bytes = Vec::new();
                    let result = root.edit_with(op, |at| {
                        if at == phase {
                            bytes = fs::read(&wal).unwrap();
                            match change {
                                0 => fs::write(&wal, &bytes).unwrap(),
                                1 => {
                                    *bytes.last_mut().unwrap() ^= 1;
                                    fs::write(&wal, &bytes).unwrap();
                                }
                                2 => fs::set_permissions(&wal, fs::Permissions::from_mode(0o644))
                                    .unwrap(),
                                _ => {
                                    bytes.extend_from_slice(b"foreign tail");
                                    fs::write(&wal, &bytes).unwrap();
                                }
                            }
                        }
                    });
                    assert!(result.is_err());
                    assert_eq!(
                        matches!(result, Err(Error::OutcomeUnknown(_))),
                        phase != MetadataBoundary::Staged
                    );
                    assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                    drop(root);
                    assert_eq!(fs::read(&wal).unwrap(), bytes);
                }
            }
        }
    }
}

#[test]
#[ignore = "native guarded metadata helper invoked by parent kill test"]
fn worker() {
    let Some(parent) = std::env::var_os("EMILYBASE_NATIVE_METADATA_TEST_PARENT") else {
        return;
    };
    let phase = std::env::var("EMILYBASE_NATIVE_METADATA_TEST_PHASE").unwrap();
    let kind = std::env::var("EMILYBASE_NATIVE_METADATA_TEST_KIND")
        .unwrap()
        .parse()
        .unwrap();
    let ready = || -> ! {
        println!("EB_NATIVE_FILE_ROOT_METADATA_READY");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    };
    let mut root = NativeFileRoot::open(PathBuf::from(parent).join("root"), PROJECT).unwrap();
    let op = operation(&mut root, kind);
    root.edit_with(op, |at| {
        if matches!(
            (phase.as_str(), at),
            ("staged", MetadataBoundary::Staged)
                | ("committed", MetadataBoundary::Committed)
                | ("verified", MetadataBoundary::Verified)
        ) {
            ready();
        }
    })
    .unwrap();
    assert_eq!(phase, "ack");
    ready();
}

#[test]
fn killed_metadata_writers_recover_only_committed_changes_and_keep_physical_objects() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for kind in 0..3 {
            for phase in ["staged", "committed", "verified", "ack"] {
                let temp = tempfile::tempdir().unwrap();
                let path = fixture(temp.path(), compacted);
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "native_root::mutation::tests::worker",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("EMILYBASE_NATIVE_METADATA_TEST_PARENT", temp.path())
                    .env("EMILYBASE_NATIVE_METADATA_TEST_PHASE", phase)
                    .env("EMILYBASE_NATIVE_METADATA_TEST_KIND", kind.to_string())
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let stdout = child.stdout.take().unwrap();
                let (send, receive) = std::sync::mpsc::channel();
                let thread = std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines() {
                        if line.unwrap() == "EB_NATIVE_FILE_ROOT_METADATA_READY" {
                            send.send(()).unwrap();
                            return;
                        }
                    }
                });
                let ready = receive.recv_timeout(std::time::Duration::from_secs(15));
                child.kill().unwrap();
                let status = child.wait().unwrap();
                thread.join().unwrap();
                assert!(ready.is_ok());
                assert_eq!(status.signal(), Some(9));
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                assert_state(&mut root, kind, phase != "staged");
                let state = root.quota_state().unwrap();
                let next = root
                    .set_quota(state, FileQuota::new(3, 10).unwrap())
                    .unwrap();
                assert_eq!(next.revision(), if phase == "staged" { 4 } else { 5 });
                assert_eq!(root.capture().unwrap().objects().objects().len(), 2);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_metadata_history_matches_independent_model_after_reopen_and_restore(compacted in any::<bool>(), steps in prop::collection::vec(0u8..6, 1..17)) {
        let _serial=TEST_IO.lock().unwrap(); let temp=tempfile::tempdir().unwrap(); let path=fixture(temp.path(),compacted); let mut root=NativeFileRoot::open(&path,PROJECT).unwrap();
        let mut name="existing";let mut visible=true;let mut file_revision=3;let mut revision=3;let mut quota=FileQuota::new(8,32768).unwrap();
        for step in steps {
            match step {
                0|1 => {let desired=if step==0 {"first"} else {"second"};let result=root.rename(FILE,file_revision,desired);prop_assert_eq!(result.is_ok(),visible);if visible && desired!=name {revision+=1;file_revision=revision;name=desired;}},
                2 => {let result=root.remove(FILE,file_revision);prop_assert_eq!(result.is_ok(),visible);if visible {visible=false;revision+=1;}},
                3|4 => {let expected=root.quota_state().unwrap();let desired=if step==3 {FileQuota::new(2,9).unwrap()} else {FileQuota::new(8,32768).unwrap()};root.set_quota(expected,desired).unwrap();if desired!=quota {revision+=1;quota=desired;}},
                _ => {let expected=root.quota_state().unwrap();prop_assert!(root.set_quota(expected,FileQuota::new(1,9).unwrap()).is_err());}
            }
            let state=root.inspect().unwrap();prop_assert_eq!(state.revision(),revision);prop_assert_eq!(state.quota(),quota);prop_assert_eq!(state.usage().physical_objects,2);prop_assert_eq!(state.usage().payload_bytes,9);prop_assert_eq!(state.usage().references,usize::from(visible));
            if visible {let info=root.file_info(FILE).unwrap().unwrap();prop_assert_eq!(info.name(),name);prop_assert_eq!(info.revision(),file_revision);}
            drop(root);root=NativeFileRoot::open(&path,PROJECT).unwrap();prop_assert_eq!(root.inspect().unwrap(),state);
        }
        let state=root.inspect().unwrap();let snapshot=root.capture().unwrap();let mut bytes=Vec::new();FileArchiveReader::from_snapshot(&snapshot).unwrap().read_to_end(&mut bytes).unwrap();restore_file_archive(&bytes,PROJECT,temp.path().join("copy")).unwrap();prop_assert_eq!(NativeFileRoot::open(temp.path().join("copy"),PROJECT).unwrap().inspect().unwrap(),state);
    }
}
