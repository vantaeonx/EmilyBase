use super::file_policy_tests::{DENY, OWN, context, create};
use super::tests::PROJECT;
use super::*;
use std::fs::{self, File};
use std::path::Path;
const PHASES: [FilePolicyWritePhase; 4] = [
    FilePolicyWritePhase::BeforeOperation,
    FilePolicyWritePhase::BeforeCommit,
    FilePolicyWritePhase::Committed,
    FilePolicyWritePhase::Verified,
];
fn prepared(path: &Path, action: &str, compact: bool) -> AccountStore {
    let mut store = create(path, if action == "migrate" { 5 } else { 6 }, compact);
    if action == "replace" {
        store.install_file_policy(context(), 0, OWN).unwrap();
    }
    store
}
fn perform(
    store: &mut AccountStore,
    action: &str,
    guard: impl FnMut(FilePolicyWritePhase) -> Result<()>,
) -> Result<Option<FilePolicyReceipt>> {
    if action == "migrate" {
        store.enable_file_policy_catalog_guarded(guard)?;
        Ok(None)
    } else {
        let expected = store
            .file_policy_receipts()?
            .first()
            .map_or(0, |p| p.revision);
        Ok(Some(store.install_file_policy_guarded(
            context(),
            expected,
            DENY,
            guard,
        )?))
    }
}
fn committed(phase: FilePolicyWritePhase) -> bool {
    matches!(
        phase,
        FilePolicyWritePhase::Committed | FilePolicyWritePhase::Verified
    )
}
fn assert_new(store: &AccountStore, action: &str) {
    assert_eq!(store.private_schema_version().unwrap(), 6);
    let current = store.file_policy_receipts().unwrap();
    if action == "migrate" {
        assert!(current.is_empty());
    } else {
        assert_eq!(current.len(), 1);
        let row = crate::file_policy::records::encode(
            context(),
            current[0].revision,
            current[0].previous,
            DENY,
        )
        .unwrap();
        assert_eq!(
            *crate::file_policy::records::inspect(context(), row.row())
                .unwrap()
                .sha256(),
            current[0].sha256
        );
    }
}
#[test]
fn guarded_actual_phases_hold_original_private_owner_and_span_the_real_commit_without_extra_unguarded_io()
 {
    let _io = TEST_IO.lock().unwrap();
    for compact in [false, true] {
        for action in ["migrate", "install", "replace"] {
            let t = tempfile::tempdir().unwrap();
            let path = t.path().join("private");
            let mut store = prepared(&path, action, compact);
            let before = store.database.committed_wal().unwrap();
            let mut seen = Vec::new();
            perform(&mut store, action, |phase| {
                seen.push(phase);
                assert!(AccountStore::open(&path, PROJECT, PasswordPool::new(1).unwrap()).is_err());
                assert!(matches!(
                    File::open(&path).unwrap().try_lock(),
                    Err(std::fs::TryLockError::WouldBlock)
                ));
                let actual = fs::read(path.join("redo.wal")).unwrap();
                if committed(phase) {
                    assert_ne!(actual, before);
                } else {
                    assert_eq!(actual, before);
                }
                Ok(())
            })
            .unwrap();
            assert_eq!(seen, PHASES);
            assert_new(&store, action);
            let before = store.database.committed_wal().unwrap();
            let mut seen = Vec::new();
            perform(&mut store, action, |phase| {
                seen.push(phase);
                Ok(())
            })
            .unwrap();
            assert_eq!(seen, [PHASES[0], PHASES[3]]);
            assert_eq!(store.database.committed_wal().unwrap(), before);
        }
    }
}
#[test]
fn refusal_before_commit_keeps_healthy_original_state_and_after_commit_withholds_receipt_without_rollback()
 {
    let _io = TEST_IO.lock().unwrap();
    for compact in [false, true] {
        for action in ["migrate", "install", "replace"] {
            for rejected in PHASES {
                let t = tempfile::tempdir().unwrap();
                let path = t.path().join("private");
                let mut store = prepared(&path, action, compact);
                let before = store.database.committed_wal().unwrap();
                let mut seen = Vec::new();
                let result = perform(&mut store, action, |phase| {
                    seen.push(phase);
                    if phase == rejected {
                        Err(Error::FilePolicyGuardRejected)
                    } else {
                        Ok(())
                    }
                });
                assert!(result.is_err());
                if committed(rejected) {
                    assert!(
                        matches!(result,Err(Error::FilePolicyWriteUnknown(ref cause)) if matches!(**cause,Error::FilePolicyGuardRejected))
                    );
                    assert_new(&store, action);
                    assert_ne!(store.database.committed_wal().unwrap(), before);
                } else {
                    assert!(matches!(result, Err(Error::FilePolicyGuardRejected)));
                    assert_eq!(store.database.committed_wal().unwrap(), before);
                }
                assert_eq!(
                    seen,
                    PHASES[..=PHASES.iter().position(|p| *p == rejected).unwrap()]
                );
                let image = store.backup_image().unwrap();
                assert!(inspect_private_account_backup_bytes(&image, PROJECT).is_ok());
                drop(store);
                let mut fresh =
                    AccountStore::open(&path, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
                let before = fresh.database.last_transaction();
                fresh.advance_session_clock(101).unwrap();
                assert_eq!(fresh.database.last_transaction(), before + 1);
                assert!(fresh.backup_image().is_ok());
            }
        }
    }
}
#[test]
fn noops_conflicts_bad_inputs_and_old_schema_never_invent_a_commit_or_postcommit_uncertainty() {
    let _io = TEST_IO.lock().unwrap();
    let t = tempfile::tempdir().unwrap();
    let mut store = create(&t.path().join("private"), 6, false);
    let installed = store.install_file_policy(context(), 0, DENY).unwrap();
    let before = store.database.committed_wal().unwrap();
    let mut seen = Vec::new();
    let result = store.install_file_policy_guarded(context(), installed.revision, DENY, |phase| {
        seen.push(phase);
        if phase == FilePolicyWritePhase::Verified {
            Err(Error::FilePolicyGuardRejected)
        } else {
            Ok(())
        }
    });
    assert!(matches!(result, Err(Error::FilePolicyGuardRejected)));
    assert_eq!(seen, [PHASES[0], PHASES[3]]);
    assert_eq!(store.database.committed_wal().unwrap(), before);
    for (expected, document) in [(u64::MAX, OWN), (installed.revision, b"{}".as_slice())] {
        let mut seen = Vec::new();
        assert!(
            store
                .install_file_policy_guarded(context(), expected, document, |phase| {
                    seen.push(phase);
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(seen, [PHASES[0]]);
        assert_eq!(store.database.committed_wal().unwrap(), before);
    }
    let mut old = create(&t.path().join("old"), 4, false);
    let before = old.database.committed_wal().unwrap();
    let mut seen = Vec::new();
    assert!(matches!(
        old.enable_file_policy_catalog_guarded(|phase| {
            seen.push(phase);
            Ok(())
        }),
        Err(Error::FilePolicySchema)
    ));
    assert_eq!(seen, [PHASES[0]]);
    assert_eq!(old.database.committed_wal().unwrap(), before);
}
#[test]
fn trusted_callback_unwind_discards_only_unconfirmed_stages_and_preserves_completed_commits() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let _io = TEST_IO.lock().unwrap();
    for compact in [false, true] {
        for action in ["migrate", "install", "replace"] {
            for rejected in PHASES {
                let t = tempfile::tempdir().unwrap();
                let path = t.path().join("private");
                let mut store = prepared(&path, action, compact);
                let before = store.database.committed_wal().unwrap();
                let result = catch_unwind(AssertUnwindSafe(|| {
                    perform(&mut store, action, |phase| {
                        if phase == rejected {
                            panic!("synthetic trusted callback unwind");
                        }
                        Ok(())
                    })
                }));
                assert!(result.is_err());
                drop(store);
                let mut fresh =
                    AccountStore::open(&path, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
                if committed(rejected) {
                    assert_new(&fresh, action);
                    assert_ne!(fresh.database.committed_wal().unwrap(), before);
                } else {
                    assert_eq!(fresh.database.committed_wal().unwrap(), before);
                }
            }
        }
    }
}
#[cfg(target_os = "linux")]
#[test]
fn actual_guard_boundaries_and_caller_return_recover_after_signal9_and_allow_a_new_independent_write()
 {
    let _io = TEST_IO.lock().unwrap();
    for compact in [false, true] {
        for action in ["migrate", "install", "replace"] {
            for phase in ["before", "prepare", "commit", "verify", "returned"] {
                let t = tempfile::tempdir().unwrap();
                let path = t.path().join("private");
                let mut store = prepared(&path, action, compact);
                let before = store.database.committed_wal().unwrap();
                drop(store);
                super::recovery_tests::kill_worker_at(
                    &path,
                    &format!("file-policy-guarded-{action}-{phase}"),
                );
                let mut fresh =
                    AccountStore::open(&path, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
                if matches!(phase, "before" | "prepare") {
                    assert_eq!(fresh.database.committed_wal().unwrap(), before);
                } else {
                    assert_new(&fresh, action);
                }
                let last = fresh.database.last_transaction();
                fresh.advance_session_clock(101).unwrap();
                assert_eq!(fresh.database.last_transaction(), last + 1);
                assert!(fresh.backup_image().is_ok());
            }
        }
    }
}
#[cfg(target_os = "linux")]
pub(super) fn worker(store: &mut AccountStore, mode: &str) -> ! {
    use std::io::Write;
    let rest = mode.strip_prefix("file-policy-guarded-").unwrap();
    let (action, target) = rest.split_once('-').unwrap();
    let barrier = || {
        println!("EMILYBASE_ACCOUNT_READY");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    };
    perform(store, action, |phase| {
        let name = match phase {
            FilePolicyWritePhase::BeforeOperation => "before",
            FilePolicyWritePhase::BeforeCommit => "prepare",
            FilePolicyWritePhase::Committed => "commit",
            FilePolicyWritePhase::Verified => "verify",
        };
        if name == target {
            barrier();
        }
        Ok(())
    })
    .unwrap();
    barrier()
}
