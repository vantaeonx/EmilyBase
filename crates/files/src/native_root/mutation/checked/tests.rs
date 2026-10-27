use super::*;
use crate::native_root::publication_tests::{changed, components, fixture};
use crate::{FileArchiveReader, TEST_IO, restore_file_archive};
use proptest::prelude::*;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FILE: FileId = FileId::from_bytes([2; 16]);
const MISSING: FileId = FileId::from_bytes([99; 16]);
type Phase = NativeWritePhase;

fn label(event: &NativeMutationCheck<'_>) -> &'static str {
    match event {
        NativeMutationCheck::Selected(_) => "selected",
        NativeMutationCheck::Phase(Phase::BeforeOperation) => "operation",
        NativeMutationCheck::Phase(Phase::BeforeCommit) => "commit",
        NativeMutationCheck::Phase(Phase::Committed) => "committed",
        NativeMutationCheck::Phase(Phase::Verified) => "verified",
        NativeMutationCheck::Phase(Phase::BeforeBlob) => panic!("metadata edit publishes no blob"),
    }
}
fn run(
    root: &mut NativeFileRoot,
    remove: bool,
    revision: u64,
    name: &str,
    check: impl for<'info> FnMut(NativeMutationCheck<'info>) -> Result<()>,
) -> Result<u64> {
    if remove {
        root.remove_with_checks(FILE, revision, check)
            .map(|receipt| receipt.revision())
    } else {
        root.rename_with_checks(FILE, revision, name, check)
            .map(|info| info.revision())
    }
}
fn fds(path: &Path) -> usize {
    let metadata = fs::metadata(path).unwrap();
    fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|entry| fs::metadata(entry.ok()?.path()).ok())
        .filter(|m| (m.dev(), m.ino()) == (metadata.dev(), metadata.ino()))
        .count()
}
fn assert_state(root: &mut NativeFileRoot, remove: bool, committed: bool) {
    let state = root.inspect().unwrap();
    assert_eq!(state.revision(), if committed { 4 } else { 3 });
    assert_eq!(state.quota(), FileQuota::new(8, 32768).unwrap());
    assert_eq!(state.usage().physical_objects, 2);
    assert_eq!(state.usage().payload_bytes, 9);
    assert_eq!(
        state.usage().references,
        usize::from(!(remove && committed))
    );
    assert_eq!(state.usage().orphans, 1 + usize::from(remove && committed));
    match root.file_info(FILE).unwrap() {
        Some(info) => {
            assert!(!(remove && committed));
            assert_eq!(info.owner(), &[4; 16]);
            assert_eq!(info.object(), ObjectId::from_bytes([3; 16]));
            assert_eq!(info.report().payload_bytes, 3);
            assert_eq!(info.name(), if committed { "renamed" } else { "existing" });
            assert_eq!(info.revision(), if committed { 4 } else { 3 });
        }
        None => assert!(remove && committed),
    }
}

#[test]
fn selected_denial_precedes_stale_cas_noop_and_staging_without_poisoning() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let path = fixture(temp.path(), compacted);
        let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
        let original = root.file_info(FILE).unwrap().unwrap();
        let before = root.inspect().unwrap();
        let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
        for remove in [false, true] {
            for revision in [0, 3, u64::MAX] {
                for name in ["existing", "renamed"] {
                    let mut events = Vec::new();
                    let result = run(&mut root, remove, revision, name, |event| {
                        events.push(label(&event));
                        if let NativeMutationCheck::Selected(info) = event {
                            assert_eq!(info, &original);
                            return Err(Error::GuardRejected);
                        }
                        Ok(())
                    });
                    assert!(matches!(result, Err(Error::GuardRejected)));
                    assert_eq!(events, ["operation", "selected"]);
                    assert_eq!(root.inspect().unwrap(), before);
                    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
                }
            }
        }
        assert_eq!(run(&mut root, false, 3, "renamed", |_| Ok(())).unwrap(), 4);
        assert_eq!(run(&mut root, true, 4, "unused", |_| Ok(())).unwrap(), 5);
        assert_eq!(root.inspect().unwrap().usage().orphans, 2);
    }
}

#[test]
fn selection_is_exactly_once_before_noop_or_commit_and_retains_original_owners() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for kind in 0..3 {
            let temp = tempfile::tempdir().unwrap();
            let path = fixture(temp.path(), compacted);
            let before_fds = fs::read_dir("/proc/self/fd").unwrap().count();
            let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
            let original = root.file_info(FILE).unwrap().unwrap();
            let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
            let mut events = Vec::new();
            let revision = run(
                &mut root,
                kind == 2,
                3,
                if kind == 0 { "existing" } else { "renamed" },
                |event| {
                    let at = label(&event);
                    events.push(at);
                    for selected in ["", "metadata", "objects", "metadata/redo.wal"] {
                        assert!(fds(&path.join(selected)) > 0, "{at}: {selected}");
                    }
                    if let NativeMutationCheck::Selected(info) = event {
                        assert_eq!(info, &original);
                        assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
                    }
                    if matches!(at, "commit" | "committed" | "verified") && kind != 0 {
                        assert!(
                            fds(&path
                                .join("objects")
                                .join(format!("{}.object", original.object())))
                                > 0
                        );
                    }
                    Ok(())
                },
            )
            .unwrap();
            if kind == 0 {
                assert_eq!(events, ["operation", "selected", "verified"]);
                assert_eq!(revision, 3);
                assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
            } else {
                assert_eq!(
                    events,
                    ["operation", "selected", "commit", "committed", "verified"]
                );
                assert_eq!(revision, 4);
            }
            drop(root);
            assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), before_fds);
        }
    }
}

#[test]
fn missing_invalid_and_authorized_conflict_have_no_staging_or_metadata_commit() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let path = fixture(temp.path(), compacted);
        let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
        let before = root.inspect().unwrap();
        let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
        for remove in [false, true] {
            let mut events = Vec::new();
            let mut check = |event: NativeMutationCheck<'_>| {
                events.push(label(&event));
                Ok(())
            };
            let result = if remove {
                root.remove_with_checks(MISSING, 3, &mut check).map(drop)
            } else {
                root.rename_with_checks(MISSING, 3, "renamed", &mut check)
                    .map(drop)
            };
            assert!(matches!(result, Err(Error::Missing)));
            assert_eq!(events, ["operation"]);
            events.clear();
            assert!(matches!(
                run(&mut root, remove, 2, "renamed", |event| {
                    events.push(label(&event));
                    Ok(())
                }),
                Err(Error::Conflict)
            ));
            assert_eq!(events, ["operation", "selected"]);
        }
        for name in ["", "\n", &"x".repeat(crate::MAX_FILE_NAME_BYTES + 1)] {
            assert!(matches!(
                root.rename_with_checks(FILE, 3, name, |_| panic!("invalid name")),
                Err(Error::Name)
            ));
        }
        assert_eq!(root.inspect().unwrap(), before);
        assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
    }
}

#[test]
fn reused_logical_identity_selects_new_owner_before_comparing_an_old_revision() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for remove in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let path = fixture(temp.path(), compacted);
            let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
            let copied = root.file_info(FILE).unwrap().unwrap();
            root.remove(FILE, copied.revision()).unwrap();
            let current = root
                .publish(
                    FILE,
                    ObjectId::from_bytes([7; 16]),
                    [8; 16],
                    "new-owner",
                    b"new",
                )
                .unwrap();
            let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
            let result = run(&mut root, remove, copied.revision(), "renamed", |event| {
                if let NativeMutationCheck::Selected(info) = event {
                    assert_eq!(info, &current);
                    assert_ne!(info.owner(), copied.owner());
                    if info.owner() != copied.owner() {
                        return Err(Error::GuardRejected);
                    }
                }
                Ok(())
            });
            assert!(matches!(result, Err(Error::GuardRejected)));
            assert!(matches!(
                run(&mut root, remove, copied.revision(), "renamed", |_| Ok(())),
                Err(Error::Conflict)
            ));
            assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
            assert_eq!(root.file_info(FILE).unwrap(), Some(current.clone()));
            assert_eq!(
                run(&mut root, remove, current.revision(), "renamed", |_| Ok(())).unwrap(),
                current.revision() + 1
            );
            assert_eq!(root.inspect().unwrap().usage().physical_objects, 3);
        }
    }
}

#[test]
fn callback_refusals_preserve_precommit_history_and_withhold_committed_outcomes() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for remove in [false, true] {
            for denied in ["operation", "selected", "commit", "committed", "verified"] {
                let temp = tempfile::tempdir().unwrap();
                let path = fixture(temp.path(), compacted);
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
                let result = run(&mut root, remove, 3, "renamed", |event| {
                    if label(&event) == denied {
                        Err(Error::GuardRejected)
                    } else {
                        Ok(())
                    }
                });
                let committed = matches!(denied, "committed" | "verified");
                assert!(result.is_err());
                assert_eq!(matches!(result, Err(Error::OutcomeUnknown(_))), committed);
                if matches!(denied, "operation" | "selected") {
                    assert_state(&mut root, remove, false);
                } else {
                    assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                }
                drop(root);
                if !committed {
                    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
                }
                assert_state(
                    &mut NativeFileRoot::open(&path, PROJECT).unwrap(),
                    remove,
                    committed,
                );
            }
        }
    }
}

#[test]
fn denied_verified_noop_does_not_invent_a_commit_or_poison_a_healthy_root() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let path = fixture(temp.path(), compacted);
        let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
        let before = root.inspect().unwrap();
        let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
        assert!(matches!(
            root.rename_with_checks(FILE, 3, "existing", |event| {
                if label(&event) == "verified" {
                    Err(Error::GuardRejected)
                } else {
                    Ok(())
                }
            }),
            Err(Error::GuardRejected)
        ));
        assert_eq!(root.inspect().unwrap(), before);
        assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
        root.rename_with_checks(FILE, 3, "renamed", |_| Ok(()))
            .unwrap();
    }
}

#[test]
fn unwind_at_every_actual_check_closes_owner_and_preserves_only_committed_changes() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for remove in [false, true] {
            for failed in ["operation", "selected", "commit", "committed", "verified"] {
                let temp = tempfile::tempdir().unwrap();
                let path = fixture(temp.path(), compacted);
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&mut root, remove, 3, "renamed", |event| {
                        if label(&event) == failed {
                            panic!("trusted check unwind")
                        }
                        Ok(())
                    })
                }));
                assert!(result.is_err());
                assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                drop(root);
                assert_state(
                    &mut NativeFileRoot::open(&path, PROJECT).unwrap(),
                    remove,
                    matches!(failed, "committed" | "verified"),
                );
            }
        }
    }
}

#[test]
fn selected_callback_cannot_hide_namespace_replacement_even_by_refusing() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for remove in [false, true] {
            for change in 0..7 {
                for refuse in [false, true] {
                    let temp = tempfile::tempdir().unwrap();
                    let path = fixture(temp.path(), compacted);
                    let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                    let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
                    let result = run(&mut root, remove, 3, "renamed", |event| {
                        if let NativeMutationCheck::Selected(_) = event {
                            changed(&path, &temp.path().join("saved"), change);
                            if refuse {
                                return Err(Error::GuardRejected);
                            }
                        }
                        Ok(())
                    });
                    assert!(result.is_err());
                    assert!(!matches!(result, Err(Error::OutcomeUnknown(_))));
                    assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                    drop(root);
                    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
                    let store = components(&path);
                    assert_eq!(store.database.last_transaction(), 3);
                    assert_eq!(store.info(FILE).unwrap().unwrap().name(), "existing");
                    assert_eq!(store.usage().unwrap().physical_objects, 2);
                    if change == 6 {
                        assert_eq!(fs::read(path.join("unexpected")).unwrap(), b"protected");
                    }
                }
            }
        }
    }
}

#[test]
fn selected_callback_wal_tampering_never_rewrites_or_repairs_external_bytes() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for remove in [false, true] {
            for change in 0..4 {
                let temp = tempfile::tempdir().unwrap();
                let path = fixture(temp.path(), compacted);
                let wal_path = path.join("metadata/redo.wal");
                let mut bytes = fs::read(&wal_path).unwrap();
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                let result = run(&mut root, remove, 3, "renamed", |event| {
                    if let NativeMutationCheck::Selected(_) = event {
                        match change {
                            0 => fs::write(&wal_path, &bytes).unwrap(),
                            1 => {
                                *bytes.last_mut().unwrap() ^= 1;
                                fs::write(&wal_path, &bytes).unwrap();
                            }
                            2 => {
                                bytes.extend_from_slice(b"foreign tail");
                                fs::write(&wal_path, &bytes).unwrap();
                            }
                            _ => fs::set_permissions(&wal_path, fs::Permissions::from_mode(0o644))
                                .unwrap(),
                        }
                    }
                    Ok(())
                });
                assert!(result.is_err());
                assert!(!matches!(result, Err(Error::OutcomeUnknown(_))));
                assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                drop(root);
                assert_eq!(fs::read(&wal_path).unwrap(), bytes);
            }
        }
    }
}

#[test]
fn successful_checked_edits_restore_without_touching_original_or_orphan_payloads() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for remove in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let path = fixture(temp.path(), compacted);
            let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
            run(&mut root, remove, 3, "renamed", |_| Ok(())).unwrap();
            let state = root.inspect().unwrap();
            let snapshot = root.capture().unwrap();
            assert_eq!(
                snapshot.metadata_report().wal_version,
                if compacted { 2 } else { 1 }
            );
            let mut archive = Vec::new();
            FileArchiveReader::from_snapshot(&snapshot)
                .unwrap()
                .read_to_end(&mut archive)
                .unwrap();
            restore_file_archive(&archive, PROJECT, temp.path().join("copy")).unwrap();
            let mut copy = NativeFileRoot::open(temp.path().join("copy"), PROJECT).unwrap();
            assert_eq!(copy.inspect().unwrap(), state);
            let restored = copy.capture().unwrap();
            assert_eq!(restored.files(), snapshot.files());
            assert_eq!(restored.metadata_bytes(), snapshot.metadata_bytes());
            assert_eq!(
                restored.objects().inventory(),
                snapshot.objects().inventory()
            );
            for (left, right) in restored
                .objects()
                .objects()
                .iter()
                .zip(snapshot.objects().objects())
            {
                assert_eq!(left.object(), right.object());
                assert_eq!(left.payload(), right.payload());
            }
            copy.publish(
                MISSING,
                ObjectId::from_bytes([10; 16]),
                [11; 16],
                "independent",
                b"copy",
            )
            .unwrap();
            assert_eq!(root.inspect().unwrap(), state);
            assert_eq!(root.file_info(MISSING).unwrap(), None);
            drop(root);
            assert_state(
                &mut NativeFileRoot::open(&path, PROJECT).unwrap(),
                remove,
                true,
            );
        }
    }
}

#[test]
#[ignore = "selection-aware mutation worker invoked only by its parent"]
fn worker() {
    let Some(parent) = std::env::var_os("EMILYBASE_SELECTED_EDIT_PARENT") else {
        return;
    };
    let phase = std::env::var("EMILYBASE_SELECTED_EDIT_PHASE").unwrap();
    let remove = std::env::var("EMILYBASE_SELECTED_EDIT_REMOVE").unwrap() == "true";
    let ready = || -> ! {
        println!("EB_SELECTED_EDIT_READY");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    };
    let mut root = NativeFileRoot::open(PathBuf::from(parent).join("root"), PROJECT).unwrap();
    run(&mut root, remove, 3, "renamed", |event| {
        if label(&event) == phase {
            ready();
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(phase, "returned");
    ready();
}

#[test]
fn actual_killed_checked_writers_recover_only_commits_and_allow_independent_writes() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for remove in [false, true] {
            for phase in [
                "operation",
                "selected",
                "commit",
                "committed",
                "verified",
                "returned",
            ] {
                let temp = tempfile::tempdir().unwrap();
                let path = fixture(temp.path(), compacted);
                let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "native_root::mutation::checked::tests::worker",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("EMILYBASE_SELECTED_EDIT_PARENT", temp.path())
                    .env("EMILYBASE_SELECTED_EDIT_PHASE", phase)
                    .env("EMILYBASE_SELECTED_EDIT_REMOVE", remove.to_string())
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let stdout = child.stdout.take().unwrap();
                let (send, receive) = std::sync::mpsc::channel();
                let reader = std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines() {
                        if line.unwrap() == "EB_SELECTED_EDIT_READY" {
                            let _ = send.send(());
                            return;
                        }
                    }
                });
                let ready = receive.recv_timeout(std::time::Duration::from_secs(15));
                child.kill().unwrap();
                let status = child.wait().unwrap();
                reader.join().unwrap();
                assert!(ready.is_ok(), "{compacted} {remove} {phase}");
                assert_eq!(status.signal(), Some(9));
                let committed = matches!(phase, "committed" | "verified" | "returned");
                if !committed {
                    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
                }
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                assert_state(&mut root, remove, committed);
                let next = root
                    .publish(
                        MISSING,
                        ObjectId::from_bytes([10; 16]),
                        [11; 16],
                        "independent",
                        b"next",
                    )
                    .unwrap();
                assert_eq!(next.revision(), if committed { 5 } else { 4 });
                assert_eq!(root.inspect().unwrap().usage().physical_objects, 3);
                drop(root);
                assert_eq!(
                    NativeFileRoot::open(&path, PROJECT)
                        .unwrap()
                        .file_info(MISSING)
                        .unwrap(),
                    Some(next)
                );
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn generated_checked_history_matches_owner_visibility_revision_and_quota_model(
        compacted in any::<bool>(),
        steps in prop::collection::vec((0u8..5, any::<bool>()), 1..13)
    ) {
        let _serial = TEST_IO.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let path = fixture(temp.path(), compacted);
        let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
        let mut visible = true;
        let mut revision = 3;
        let mut file_revision = 3;
        let mut owner = [4; 16];
        let mut name = "existing";
        let mut objects = 2;
        let mut bytes = 9;
        for (operation, permitted) in steps {
            let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
            if operation == 4 && !visible && objects < 8 {
                owner = if owner == [4; 16] { [8; 16] } else { [4; 16] };
                let info = root.publish(
                    FILE, ObjectId::from_bytes([20 + objects; 16]), owner, "existing", b"new"
                ).unwrap();
                revision += 1;
                file_revision = revision;
                prop_assert_eq!(info.revision(), revision);
                name = "existing";
                visible = true;
                objects += 1;
                bytes += 3;
            } else {
                let remove = operation == 3;
                let desired = if operation % 2 == 0 { "first" } else { "second" };
                let stale = operation == 2;
                let expected = if stale { 0 } else { file_revision };
                let mut selected = 0;
                let result = run(&mut root, remove, expected, desired, |event| {
                    if let NativeMutationCheck::Selected(info) = event {
                        selected += 1;
                        prop_assert_native_owner(info, owner, file_revision, name);
                        if !permitted { return Err(Error::GuardRejected); }
                    }
                    Ok(())
                });
                prop_assert_eq!(selected, usize::from(visible));
                if !visible {
                    prop_assert!(matches!(result, Err(Error::Missing)));
                } else if !permitted {
                    prop_assert!(matches!(result, Err(Error::GuardRejected)));
                } else if stale {
                    prop_assert!(matches!(result, Err(Error::Conflict)));
                } else {
                    prop_assert!(result.is_ok());
                    if remove || desired != name {
                        revision += 1;
                        file_revision = revision;
                        name = desired;
                    }
                    visible = !remove;
                }
                if result.is_err() {
                    prop_assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
                }
            }
            let state = root.inspect().unwrap();
            prop_assert_eq!(state.revision(), revision);
            prop_assert_eq!(state.usage().physical_objects, objects as usize);
            prop_assert_eq!(state.usage().references, usize::from(visible));
            prop_assert_eq!(state.usage().orphans, objects as usize - usize::from(visible));
            prop_assert_eq!(state.usage().payload_bytes, bytes);
            if let Some(info) = root.file_info(FILE).unwrap() {
                prop_assert!(visible);
                prop_assert_native_owner(&info, owner, file_revision, name);
            } else {
                prop_assert!(!visible);
            }
            drop(root);
            root = NativeFileRoot::open(&path, PROJECT).unwrap();
            prop_assert_eq!(root.inspect().unwrap(), state);
        }
    }
}
fn prop_assert_native_owner(info: &FileInfo, owner: [u8; 16], revision: u64, name: &str) {
    assert_eq!(info.id(), FILE);
    assert_eq!(info.owner(), &owner);
    assert_eq!(info.revision(), revision);
    assert_eq!(info.name(), name);
}
