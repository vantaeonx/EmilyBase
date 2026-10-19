use super::*;
use std::os::unix::fs::MetadataExt;
const FILE: FileId = FileId::from_bytes([2; 16]);
type Phase = NativeWritePhase;
fn run(root: &mut NativeFileRoot, kind: u8, guard: impl FnMut(Phase) -> Result<()>) -> Result<()> {
    run_with_payload(root, kind, b"payload", guard)
}
pub(super) fn run_with_payload(
    root: &mut NativeFileRoot,
    kind: u8,
    payload: &[u8],
    guard: impl FnMut(Phase) -> Result<()>,
) -> Result<()> {
    match kind {
        0 => root
            .publish_guarded(FRESH, OBJECT, [8; 16], "fresh", payload, guard)
            .map(drop),
        1 => root.rename_guarded(FILE, 3, "renamed", guard).map(drop),
        2 => root.remove_guarded(FILE, 3, guard).map(drop),
        _ => {
            let expected = root.quota_state()?;
            root.set_quota_guarded(expected, FileQuota::new(4, 16384)?, guard)
                .map(drop)
        }
    }
}
pub(super) fn label(phase: Phase) -> &'static str {
    match phase {
        Phase::BeforeOperation => "operation",
        Phase::BeforeBlob => "blob",
        Phase::BeforeCommit => "commit",
        Phase::Committed => "committed",
        Phase::Verified => "verified",
    }
}
fn phases(kind: u8) -> Vec<Phase> {
    let mut phases = vec![Phase::BeforeOperation];
    if kind == 0 {
        phases.push(Phase::BeforeBlob);
    }
    phases.extend([Phase::BeforeCommit, Phase::Committed, Phase::Verified]);
    phases
}
fn assert_state(root: &mut NativeFileRoot, kind: u8, committed: bool, blob: bool) {
    let state = root.inspect().unwrap();
    assert_eq!(state.revision(), if committed { 4 } else { 3 });
    assert_eq!(state.usage().physical_objects, 2 + usize::from(blob));
    assert_eq!(
        state.usage().references,
        if committed && kind == 0 {
            2
        } else {
            usize::from(!(committed && kind == 2))
        }
    );
    let original = root.file_info(FILE).unwrap();
    if committed && kind == 2 {
        assert!(original.is_none());
    } else {
        assert_eq!(
            original.unwrap().name(),
            if committed && kind == 1 {
                "renamed"
            } else {
                "existing"
            }
        );
    }
    assert_eq!(
        root.file_info(FRESH).unwrap().is_some(),
        committed && kind == 0
    );
    assert_eq!(
        state.quota(),
        FileQuota::new(
            if committed && kind == 3 { 4 } else { 8 },
            if committed && kind == 3 { 16384 } else { 32768 }
        )
        .unwrap()
    );
}
fn fds(path: &Path) -> usize {
    let Some(expected) = fs::metadata(path).ok() else {
        return 0;
    };
    fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|e| fs::metadata(e.ok()?.path()).ok())
        .filter(|m| (m.dev(), m.ino()) == (expected.dev(), expected.ino()))
        .count()
}
#[test]
fn real_guard_phases_match_durable_state_and_original_owners_remain_held() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for kind in 0..4 {
            let t = tempfile::tempdir().unwrap();
            let path = fixture(t.path(), compacted);
            let wal = path.join("metadata/redo.wal");
            let before = fs::read(&wal).unwrap();
            let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
            let mut seen = Vec::new();
            run(&mut root, kind, |phase| {
                seen.push(phase);
                // Native common selection retains its original directory; its
                // two component owners, rather than a new directory flock,
                // provide the existing exclusive writer contract.
                assert!(fds(&path) > 0);
                assert!(Database::open(path.join("metadata")).is_err());
                assert!(ProjectDirectory::open(path.join("objects"), PROJECT).is_err());
                if matches!(phase, Phase::Committed | Phase::Verified) {
                    assert_ne!(fs::read(&wal).unwrap(), before);
                } else {
                    assert_eq!(fs::read(&wal).unwrap(), before);
                }
                if matches!(
                    phase,
                    Phase::BeforeCommit | Phase::Committed | Phase::Verified
                ) {
                    let object = if kind == 0 {
                        OBJECT
                    } else {
                        ObjectId::from_bytes([3; 16])
                    };
                    assert!(fds(&path.join("objects").join(format!("{object}.object"))) > 0);
                    if kind == 3 {
                        assert!(
                            fds(&path
                                .join("objects")
                                .join(format!("{}.object", ObjectId::from_bytes([5; 16]))))
                                > 0
                        );
                    }
                }
                Ok(())
            })
            .unwrap();
            assert_eq!(seen, phases(kind));
            assert_state(&mut root, kind, true, kind == 0);
            let snapshot = root.capture().unwrap();
            let mut bytes = Vec::new();
            FileArchiveReader::from_snapshot(&snapshot)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            restore_file_archive(&bytes, PROJECT, t.path().join("copy")).unwrap();
            let mut copy = NativeFileRoot::open(t.path().join("copy"), PROJECT).unwrap();
            assert_state(&mut copy, kind, true, kind == 0);
        }
    }
}
#[test]
fn rejection_at_each_actual_boundary_preserves_confirmed_data_or_private_orphan_without_fake_rollback()
 {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for kind in 0..4 {
            for rejected in phases(kind) {
                let t = tempfile::tempdir().unwrap();
                let path = fixture(t.path(), compacted);
                let wal = path.join("metadata/redo.wal");
                let before = fs::read(&wal).unwrap();
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                let mut seen = Vec::new();
                let result = run(&mut root, kind, |phase| {
                    seen.push(phase);
                    if phase == rejected {
                        Err(Error::GuardRejected)
                    } else {
                        Ok(())
                    }
                });
                assert_eq!(seen.last(), Some(&rejected));
                let committed = matches!(rejected, Phase::Committed | Phase::Verified);
                let blob = kind == 0
                    && matches!(
                        rejected,
                        Phase::BeforeCommit | Phase::Committed | Phase::Verified
                    );
                let uncertain = committed || blob;
                if uncertain {
                    assert!(matches!(result, Err(Error::OutcomeUnknown(_))));
                } else {
                    assert!(matches!(result, Err(Error::GuardRejected)));
                }
                let healthy = matches!(rejected, Phase::BeforeOperation | Phase::BeforeBlob);
                assert_eq!(root.inspect().is_ok(), healthy);
                if !committed {
                    assert_eq!(fs::read(&wal).unwrap(), before);
                }
                drop(root);
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                assert_state(&mut root, kind, committed, blob);
                if blob {
                    assert_eq!(
                        root.inspect().unwrap().usage().orphans,
                        if committed { 1 } else { 2 }
                    );
                }
            }
        }
    }
}
#[test]
fn noops_recheck_guard_without_inventing_a_commit_and_preflight_denial_preserves_healthy_owner() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let path = fixture(t.path(), compacted);
        let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
        let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
        for kind in 0..2 {
            let mut seen = Vec::new();
            let mut guard = |phase| {
                seen.push(phase);
                Ok(())
            };
            if kind == 0 {
                root.rename_guarded(FILE, 3, "existing", &mut guard)
                    .unwrap();
            } else {
                let expected = root.quota_state().unwrap();
                root.set_quota_guarded(expected, expected.quota(), &mut guard)
                    .unwrap();
            }
            assert_eq!(seen, vec![Phase::BeforeOperation, Phase::Verified]);
            assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
            let reject = |phase| {
                if phase == Phase::Verified {
                    Err(Error::GuardRejected)
                } else {
                    Ok(())
                }
            };
            if kind == 0 {
                assert!(matches!(
                    root.rename_guarded(FILE, 3, "existing", reject),
                    Err(Error::GuardRejected)
                ));
            } else {
                let expected = root.quota_state().unwrap();
                assert!(matches!(
                    root.set_quota_guarded(expected, expected.quota(), reject),
                    Err(Error::GuardRejected)
                ));
            }
            assert_state(&mut root, 1, false, false);
        }
    }
}
#[test]
fn callback_unwind_poisoning_prevents_reusing_an_owner_after_a_partially_observed_write() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for kind in 0..4 {
            for failed in phases(kind) {
                let t = tempfile::tempdir().unwrap();
                let path = fixture(t.path(), compacted);
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&mut root, kind, |phase| {
                        assert_ne!(phase, failed, "synthetic guard unwind");
                        Ok(())
                    })
                }));
                assert!(result.is_err());
                assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                drop(root);
                let committed = matches!(failed, Phase::Committed | Phase::Verified);
                let blob = kind == 0
                    && matches!(
                        failed,
                        Phase::BeforeCommit | Phase::Committed | Phase::Verified
                    );
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                assert_state(&mut root, kind, committed, blob);
            }
        }
    }
}
#[test]
fn successful_outer_callback_cannot_hide_a_changed_native_namespace_before_persistence() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for kind in 0..4 {
            for changed_at in phases(kind) {
                let t = tempfile::tempdir().unwrap();
                let path = fixture(t.path(), compacted);
                let wal = path.join("metadata/redo.wal");
                let before = fs::read(&wal).unwrap();
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                let result = run(&mut root, kind, |phase| {
                    if phase == changed_at {
                        fs::write(path.join("unexpected"), b"protected").unwrap();
                    }
                    Ok(())
                });
                let committed = matches!(changed_at, Phase::Committed | Phase::Verified);
                let blob = kind == 0
                    && matches!(
                        changed_at,
                        Phase::BeforeCommit | Phase::Committed | Phase::Verified
                    );
                if committed || blob {
                    assert!(matches!(result, Err(Error::OutcomeUnknown(_))));
                } else {
                    assert!(result.is_err());
                }
                assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                drop(root);
                assert_eq!(fs::read(path.join("unexpected")).unwrap(), b"protected");
                if !committed {
                    assert_eq!(fs::read(&wal).unwrap(), before);
                }
                // The native namespace remains invalid; inspect the unchanged
                // actual component graph without adopting the foreign entry.
                let store = components(&path);
                assert_eq!(
                    store.database.last_transaction(),
                    if committed { 4 } else { 3 }
                );
                assert_eq!(store.info(FRESH).unwrap().is_some(), kind == 0 && committed);
                assert_eq!(
                    store.usage().unwrap().physical_objects,
                    2 + usize::from(blob)
                );
            }
        }
    }
}
#[test]
fn invalid_inputs_and_cas_conflicts_do_not_publish_a_blob_or_commit_metadata() {
    let _serial = TEST_IO.lock().unwrap();
    let t = tempfile::tempdir().unwrap();
    let path = fixture(t.path(), false);
    let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
    let before = root.inspect().unwrap();
    let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
    let mut called = false;
    assert!(matches!(
        root.publish_guarded(FRESH, OBJECT, [8; 16], "\n", b"payload", |_| {
            called = true;
            Ok(())
        }),
        Err(Error::Name)
    ));
    assert!(!called);
    assert!(matches!(
        root.rename_guarded(FILE, 2, "renamed", |_| Ok(())),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        root.remove_guarded(FileId::from_bytes([99; 16]), 3, |_| Ok(())),
        Err(Error::Missing)
    ));
    let expected = root.quota_state().unwrap();
    assert!(
        root.set_quota_guarded(expected, FileQuota::new(1, 9).unwrap(), |_| Ok(()))
            .is_err()
    );
    assert_eq!(root.inspect().unwrap(), before);
    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_guard_decisions_match_revision_and_physical_orphan_model(compacted in any::<bool>(),kind in 0u8..4,decision in 0usize..7) {
        let _serial=TEST_IO.lock().unwrap();let t=tempfile::tempdir().unwrap();let path=fixture(t.path(),compacted);let available=phases(kind);let denied=available.get(decision).copied();let mut root=NativeFileRoot::open(&path,PROJECT).unwrap();
        let result=run(&mut root,kind,|phase|if Some(phase)==denied{Err(Error::GuardRejected)}else{Ok(())});prop_assert_eq!(result.is_ok(),denied.is_none());drop(root);
        let committed=denied.is_none()||matches!(denied,Some(Phase::Committed|Phase::Verified));let blob=kind==0&&(committed||denied==Some(Phase::BeforeCommit));let mut root=NativeFileRoot::open(&path,PROJECT).unwrap();assert_state(&mut root,kind,committed,blob);
    }
}
#[test]
fn actual_guard_boundary_kills_recover_confirmed_metadata_and_charge_only_selected_physical_objects()
 {
    use std::os::unix::process::ExitStatusExt;
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for kind in 0..4 {
            for empty in [false, true] {
                if kind != 0 && empty {
                    continue;
                }
                let mut boundaries = phases(kind).into_iter().map(label).collect::<Vec<_>>();
                boundaries.push("returned");
                for phase in boundaries {
                    let t = tempfile::tempdir().unwrap();
                    let path = fixture(t.path(), compacted);
                    let payload = if empty { vec![] } else { vec![0x53; 8193] };
                    fs::write(t.path().join("payload"), &payload).unwrap();
                    let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
                    let mut child = Command::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            "native_root::publication_tests::worker",
                            "--ignored",
                            "--nocapture",
                        ])
                        .env("EMILYBASE_NATIVE_PUBLISH_TEST_PARENT", t.path())
                        .env("EMILYBASE_NATIVE_PUBLISH_TEST_PHASE", "guard")
                        .env("EMILYBASE_NATIVE_GUARD_PHASE", phase)
                        .env("EMILYBASE_NATIVE_GUARD_KIND", kind.to_string())
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()
                        .unwrap();
                    let stdout = child.stdout.take().unwrap();
                    let (send, receive) = std::sync::mpsc::channel();
                    let thread = std::thread::spawn(move || {
                        for line in BufReader::new(stdout).lines() {
                            if line.unwrap() == "EB_NATIVE_FILE_ROOT_PUBLISH_READY" {
                                let _ = send.send(());
                                return;
                            }
                        }
                    });
                    let ready = receive.recv_timeout(std::time::Duration::from_secs(15));
                    child.kill().unwrap();
                    let status = child.wait().unwrap();
                    thread.join().unwrap();
                    assert!(ready.is_ok(), "{kind} {phase}");
                    assert_eq!(status.signal(), Some(9));
                    let committed = matches!(phase, "committed" | "verified" | "returned");
                    let blob = kind == 0 && !matches!(phase, "operation" | "blob");
                    if !committed {
                        assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
                    }
                    let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                    assert_state(&mut root, kind, committed, blob);
                    if blob {
                        let snapshot = root.capture().unwrap();
                        assert_eq!(
                            snapshot
                                .objects()
                                .objects()
                                .iter()
                                .find(|o| o.object() == OBJECT)
                                .unwrap()
                                .payload(),
                            payload
                        );
                    }
                    let current = root.quota_state().unwrap();
                    root.set_quota_guarded(current, FileQuota::new(8, 32768).unwrap(), |_| Ok(()))
                        .unwrap();
                    root.publish_guarded(
                        FileId::from_bytes([10; 16]),
                        ObjectId::from_bytes([11; 16]),
                        [8; 16],
                        "next",
                        b"independent",
                        |_| Ok(()),
                    )
                    .unwrap();
                    assert!(
                        root.file_info(FileId::from_bytes([10; 16]))
                            .unwrap()
                            .is_some()
                    );
                }
            }
        }
    }
}
