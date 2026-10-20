//! Actual current-key metadata adapter; user-file routes remain separate.
use super::*;
use crate::Result;
use emilybase_auth::accounts::Error as AccountError;
use std::panic::{AssertUnwindSafe, catch_unwind};

const OWN: &[u8] = br#"{"version":1,"read":{"kind":"owner"},"create":{"kind":"self_owned"},"rename":{"kind":"owner"},"remove":{"kind":"owner"}}"#;
const DENY: &[u8] = br#"{"version":1,"read":{"kind":"deny"},"create":{"kind":"deny"},"rename":{"kind":"deny"},"remove":{"kind":"deny"}}"#;
const POINTS: [&str; 6] = [
    "account_file_policy_context_acquired",
    "account_file_policy_before_operation",
    "account_file_policy_before_commit",
    "account_file_policy_committed",
    "account_file_policy_verified",
    "account_file_policy_native_returned",
];
fn committed(point: &str) -> bool {
    matches!(
        point,
        "account_file_policy_committed"
            | "account_file_policy_verified"
            | "account_file_policy_native_returned"
            | "account_file_policy_returned"
    )
}
fn prepare(root: &mut AccountRoot, id: &str, key: &str, kind: u8) -> u64 {
    root.enable_row_policy_catalog(id, key).unwrap();
    root.enable_public_admission_catalog(id, key).unwrap();
    if kind == 0 {
        return 0;
    }
    root.enable_file_policy_catalog(id, key).unwrap();
    if kind == 2 {
        root.install_project_file_policy(id, key, 0, OWN)
            .unwrap()
            .revision
    } else {
        0
    }
}
fn run(root: &mut AccountRoot, id: &str, key: &str, kind: u8, expected: u64) -> Result<()> {
    if kind == 0 {
        root.enable_file_policy_catalog(id, key)
    } else {
        root.install_project_file_policy(id, key, expected, DENY)
            .map(drop)
    }
}
fn private_wal(path: &Path, id: &str) -> Vec<u8> {
    fs::read(path.join("private").join(id).join("redo.wal")).unwrap()
}
fn unrelated(path: &Path, id: &str) -> Vec<Vec<u8>> {
    [
        "root.json".into(),
        format!("registry/{id}/data/redo.wal"),
        format!("registry/{id}/project.json"),
        format!("files/{id}/metadata/redo.wal"),
        format!("files/{id}/objects/{BLOB}.object"),
        format!("files/{id}/objects/{ORPHAN}.object"),
    ]
    .iter()
    .map(|p| fs::read(path.join(p)).unwrap())
    .collect()
}
fn assert_state(root: &mut AccountRoot, id: &str, key: &str, kind: u8, new: bool, old: u64) {
    assert_eq!(
        root.private_schema_version(id, key).unwrap(),
        if kind == 0 && !new { 5 } else { 6 }
    );
    if kind == 0 {
        if new {
            assert!(root.project_file_policy(id, key).unwrap().is_none());
        }
    } else {
        let receipt = root.project_file_policy(id, key).unwrap();
        if new {
            let receipt = receipt.unwrap();
            assert!(receipt.revision > old);
            assert_eq!(receipt.previous, old);
            let context = emilybase_auth::file_policy::StoreContext {
                project: id,
                database: receipt.database,
            };
            let encoded = emilybase_auth::file_policy::records::encode(
                context,
                receipt.revision,
                receipt.previous,
                DENY,
            )
            .unwrap();
            assert_eq!(
                receipt.sha256,
                *emilybase_auth::file_policy::records::inspect(context, encoded.row())
                    .unwrap()
                    .sha256()
            );
        } else if kind == 2 {
            assert_eq!(receipt.unwrap().revision, old);
        } else {
            assert!(receipt.is_none());
        }
    }
}
fn assert_poisoned(root: &mut AccountRoot, id: &str, key: &str) {
    assert!(matches!(
        root.projects(),
        Err(Error::AccountFileWritePoisoned)
    ));
    assert!(matches!(
        root.project_file_policy(id, key),
        Err(Error::AccountFileWritePoisoned)
    ));
    assert!(matches!(
        root.enable_file_policy_catalog(id, key),
        Err(Error::AccountFileWritePoisoned)
    ));
    assert!(matches!(
        root.private_schema_version(id, key),
        Err(Error::AccountFileWritePoisoned)
    ));
    assert!(matches!(
        root.execute(id, key, "SELECT * FROM t", &[]),
        Err(Error::AccountFileWritePoisoned)
    ));
    assert!(matches!(
        root.read_project_file(id, key, FILE, 32),
        Err(Error::AccountFileWritePoisoned)
    ));
    assert!(matches!(
        root.capture_v2(),
        Err(Error::AccountFileWritePoisoned)
    ));
    assert!(matches!(
        root.rotate_project_key(id),
        Err(Error::AccountFileWritePoisoned)
    ));
}

#[test]
fn real_native_binding_exact_cas_current_key_isolation_and_retained_parent_survive_independent_restore()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, mut path) = copy(t.path(), 2, compact);
        let mut root = if compact {
            AccountRoot::open_v2_at(&File::open(path.parent().unwrap()).unwrap(), "v2", pool())
                .unwrap()
        } else {
            AccountRoot::open_v2(&path, pool()).unwrap()
        };
        if compact {
            let original = path.parent().unwrap().to_path_buf();
            let moved = t.path().join("moved-policy-owner");
            fs::rename(&original, &moved).unwrap();
            fs::DirBuilder::new().mode(0o700).create(&original).unwrap();
            fs::write(original.join("keep"), b"foreign parent").unwrap();
            path = moved.join("v2");
        }
        let (id, key) = &s.root.credentials[0];
        let (other, other_key) = &s.root.credentials[1];
        let expected = prepare(&mut root, id, key, 1);
        assert_eq!(expected, 0);
        let other_before = private_wal(&path, other);
        let before = unrelated(&path, id);
        let native = *root.inspect_project_files(id, key).unwrap().database_id();
        assert!(root.project_file_policy(id, key).unwrap().is_none());
        let installed = root.install_project_file_policy(id, key, 0, OWN).unwrap();
        assert_eq!(installed.database, native);
        assert!(!root.public_admission(id, key).unwrap().enabled);
        assert_eq!(private_wal(&path, other), other_before);
        assert_eq!(unrelated(&path, id), before);
        let wal = private_wal(&path, id);
        assert_eq!(
            root.install_project_file_policy(id, key, 0, OWN).unwrap(),
            installed
        );
        assert_eq!(
            root.install_project_file_policy(id, key, installed.revision, OWN)
                .unwrap(),
            installed
        );
        assert_eq!(private_wal(&path, id), wal);
        assert!(matches!(
            root.install_project_file_policy(id, key, 0, DENY),
            Err(Error::Accounts(AccountError::FilePolicyConflict))
        ));
        assert!(matches!(
            root.project_file_policy(id, other_key),
            Err(Error::Denied)
        ));
        assert!(matches!(
            root.project_file_policy(other, key),
            Err(Error::Denied)
        ));
        let session = root.sign_in(id, key, LOGIN, PASSWORD, 101).unwrap();
        assert!(matches!(
            root.install_project_file_policy(id, session.access.expose(), installed.revision, DENY),
            Err(Error::Denied)
        ));
        let rotated = root.rotate_project_key(id).unwrap();
        assert!(matches!(
            root.project_file_policy(id, key),
            Err(Error::Denied)
        ));
        assert_eq!(
            root.project_file_policy(id, &rotated.api_key).unwrap(),
            Some(installed.clone())
        );
        let new = root
            .install_project_file_policy(id, &rotated.api_key, installed.revision, DENY)
            .unwrap();
        assert_eq!(new.previous, installed.revision);
        assert!(new.revision > installed.revision);
        let image = encode(&root.capture_v2().unwrap());
        let source_wal = private_wal(&path, id);
        let target = t.path().join("independent-policy-copy");
        crate::restore_account_file_archive_bytes_at(
            &image,
            &File::open(t.path()).unwrap(),
            "independent-policy-copy",
            pool(),
            50,
        )
        .unwrap();
        let mut restored = AccountRoot::open_v2(&target, pool()).unwrap();
        assert_eq!(
            restored.project_file_policy(id, &rotated.api_key).unwrap(),
            Some(new)
        );
        assert!(
            !restored
                .public_admission(id, &rotated.api_key)
                .unwrap()
                .enabled
        );
        assert!(
            restored
                .with_access(id, &rotated.api_key, session.access.expose(), 50, |_| ())
                .is_err()
        );
        root.with_access(id, &rotated.api_key, session.access.expose(), 101, |_| ())
            .unwrap();
        assert_eq!(private_wal(&path, id), source_wal);
        assert_eq!(private_wal(&path, other), other_before);
        drop(restored);
        drop(root);
        verify_source(&s);
    }
}

#[test]
fn original_common_private_native_and_current_key_descriptors_span_every_actual_write_phase() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for kind in 0..3 {
        for point in POINTS {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), 1, kind % 2 == 0);
            let (id, key) = &s.root.credentials[0];
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            let expected = prepare(&mut root, id, key, kind);
            let before = private_wal(&path, id);
            let original = path.clone();
            let project = id.clone();
            let wal = before.clone();
            let _hook = durability::on_boundary(point, move || {
                assert!(File::open(&original).unwrap().try_lock().is_err());
                assert!(ProjectStore::open_existing(original.join("registry")).is_err());
                assert!(
                    AccountStore::open(original.join("private").join(&project), &project, pool())
                        .is_err()
                );
                assert!(
                    NativeFileRoot::open(
                        original.join("files").join(&project),
                        project.parse().unwrap()
                    )
                    .is_err()
                );
                assert!(
                    Database::open(original.join("registry").join(&project).join("data")).is_ok()
                );
                assert!(
                    fds(
                        std::process::id(),
                        &original.join("private").join(&project).join("redo.wal")
                    ) >= 2
                );

                assert!(
                    fds(
                        std::process::id(),
                        &original
                            .join("registry")
                            .join(&project)
                            .join("project.json")
                    ) > 0
                );
                assert_eq!(private_wal(&original, &project) == wal, !committed(point));
            });
            run(&mut root, id, key, kind, expected).unwrap();
            assert_state(&mut root, id, key, kind, true, expected);
            drop(root);
            verify_source(&s);
        }
    }
}

#[test]
fn common_key_or_native_identity_refusal_preserves_commit_and_closes_the_entire_live_root_only_after_write()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for kind in 0..3 {
            for point in POINTS {
                for target in 0..4 {
                    let t = tempfile::tempdir().unwrap();
                    let (s, path) = copy(t.path(), 1, compact);
                    let (id, key) = &s.root.credentials[0];
                    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                    let expected = prepare(&mut root, id, key, kind);
                    let before = private_wal(&path, id);
                    let selected = match target {
                        0 => path.join("root.json"),
                        1 => path.join("registry").join(id).join("project.json"),
                        2 => path.join("files").join(id).join("metadata/redo.wal"),
                        _ => path.join("private").join(id).join("redo.wal"),
                    };
                    let changed = selected.clone();
                    let saved = t.path().join("original");
                    let retained = saved.clone();
                    let _hook = durability::on_boundary(point, move || {
                        fs::rename(&changed, &retained).unwrap();
                        fs::copy(retained, changed).unwrap();
                    });
                    let result = run(&mut root, id, key, kind, expected);
                    assert!(result.is_err(), "{kind} {point} {target}");
                    drop(_hook);
                    fs::rename(saved, selected).unwrap();
                    assert_eq!(
                        matches!(result, Err(Error::AccountFilePolicyWriteUnknown(_))),
                        committed(point),
                        "{kind} {point}"
                    );
                    if committed(point) {
                        assert_poisoned(&mut root, id, key);
                    } else {
                        if target == 2 {
                            // Native original WAL stamps include ctime. Restoring
                            // the inode cannot restore its original kernel stamp.
                            assert!(matches!(root.projects(), Err(Error::Files(_))));
                        } else {
                            assert_eq!(root.projects().unwrap().len(), 1);
                        }
                        assert_eq!(private_wal(&path, id), before);
                    }
                    drop(root);
                    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                    assert_state(&mut root, id, key, kind, committed(point), expected);
                    root.execute(id, key, "UPDATE t SET v=99 WHERE id=1", &[])
                        .unwrap();
                    assert!(root.capture_v2().is_ok());
                    drop(root);
                    verify_source(&s);
                }
            }
        }
    }
}

#[test]
fn noops_conflicts_invalid_documents_and_old_private_versions_do_not_publish_or_poison() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, false);
    let (id, key) = &s.root.credentials[0];
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    let before = private_wal(&path, id);
    assert!(matches!(
        root.project_file_policy(id, key),
        Err(Error::Accounts(AccountError::FilePolicySchema))
    ));
    assert_eq!(private_wal(&path, id), before);
    let expected = prepare(&mut root, id, key, 2);
    let before = private_wal(&path, id);
    for (revision, document) in [(expected, b"{}".as_slice()), (0, DENY)] {
        let _hook = durability::on_boundary("account_file_policy_before_commit", || {
            panic!("invalid input attempted commit")
        });
        assert!(
            root.install_project_file_policy(id, key, revision, document)
                .is_err()
        );
        assert_eq!(private_wal(&path, id), before);
        assert_eq!(root.projects().unwrap().len(), 1);
    }
    for migrate in [false, true] {
        let selected = path.join("root.json");
        let changed = selected.clone();
        let saved = t.path().join("original");
        let retained = saved.clone();
        let _hook = durability::on_boundary("account_file_policy_native_returned", move || {
            fs::rename(&changed, &retained).unwrap();
            fs::copy(retained, changed).unwrap();
        });
        let result = if migrate {
            root.enable_file_policy_catalog(id, key)
        } else {
            root.install_project_file_policy(id, key, expected, OWN)
                .map(drop)
        };
        assert!(result.is_err());
        assert!(!matches!(
            result,
            Err(Error::AccountFilePolicyWriteUnknown(_))
        ));
        drop(_hook);
        fs::rename(saved, selected).unwrap();
        assert_eq!(root.projects().unwrap().len(), 1);
        assert_eq!(private_wal(&path, id), before);
    }
    drop(root);
    verify_source(&s);
}

#[test]
fn caught_trusted_unwind_cannot_reuse_a_root_after_possible_publication_and_does_not_hide_commits()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for kind in 0..3 {
            for point in POINTS {
                let t = tempfile::tempdir().unwrap();
                let (s, path) = copy(t.path(), 1, compact);
                let (id, key) = &s.root.credentials[0];
                let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                let expected = prepare(&mut root, id, key, kind);
                let before = private_wal(&path, id);
                let _hook =
                    durability::on_boundary(point, || panic!("synthetic coordination unwind"));
                assert!(
                    catch_unwind(AssertUnwindSafe(|| run(&mut root, id, key, kind, expected)))
                        .is_err()
                );
                drop(_hook);
                if committed(point) {
                    assert_poisoned(&mut root, id, key);
                } else {
                    assert_eq!(private_wal(&path, id), before);
                    assert_eq!(root.projects().unwrap().len(), 1);
                }
                drop(root);
                let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                assert_state(&mut root, id, key, kind, committed(point), expected);
                assert!(root.capture_v2().is_ok());
                drop(root);
                verify_source(&s);
            }
        }
    }
}

#[test]
fn real_common_guard_and_native_return_signal9_kills_recover_only_confirmed_policy_and_allow_a_new_write()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for kind in 0..3 {
            for point in POINTS.into_iter().chain([
                "account_file_policy_commit_admitted",
                "account_file_policy_returned",
            ]) {
                let t = tempfile::tempdir().unwrap();
                let (s, path) = copy(t.path(), 1, compact);
                let (id, key) = &s.root.credentials[0];
                let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                let expected = prepare(&mut root, id, key, kind);
                drop(root);
                let before = private_wal(&path, id);
                let others = unrelated(&path, id);
                let key_file = t.path().join("service-key");
                fs::write(&key_file, key).unwrap();
                fs::set_permissions(&key_file, fs::Permissions::from_mode(0o600)).unwrap();
                let worker = Worker::start(
                    &path,
                    &key_file,
                    id,
                    &format!("root-v2-file-policy-{kind}-{expected}"),
                    point,
                );
                worker.reach(point);
                assert!(File::open(&path).unwrap().try_lock().is_err());
                assert!(AccountStore::open(path.join("private").join(id), id, pool()).is_err());
                assert!(
                    NativeFileRoot::open(path.join("files").join(id), id.parse().unwrap()).is_err()
                );
                worker.kill();
                assert_eq!(unrelated(&path, id), others);
                if !committed(point) {
                    assert_eq!(private_wal(&path, id), before);
                }
                let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                assert_state(&mut root, id, key, kind, committed(point), expected);
                root.execute(id, key, "UPDATE t SET v=98 WHERE id=1", &[])
                    .unwrap();
                assert!(root.capture_v2().is_ok());
                drop(root);
                verify_source(&s);
            }
        }
    }
}

#[test]
fn unwind_after_accepting_before_commit_poisons_root_even_when_staged_transaction_drops_unconfirmed()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for kind in 0..3 {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), 1, compact);
            let (id, key) = &s.root.credentials[0];
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            let expected = prepare(&mut root, id, key, kind);
            let before = private_wal(&path, id);
            let _hook = durability::on_boundary("account_file_policy_commit_admitted", || {
                panic!("synthetic unwind after accepting last outer check")
            });
            assert!(
                catch_unwind(AssertUnwindSafe(|| run(&mut root, id, key, kind, expected))).is_err()
            );
            drop(_hook);
            assert_poisoned(&mut root, id, key);
            assert_eq!(private_wal(&path, id), before);
            drop(root);
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            assert_state(&mut root, id, key, kind, false, expected);
            root.execute(id, key, "UPDATE t SET v=97 WHERE id=1", &[])
                .unwrap();
            assert!(root.capture_v2().is_ok());
            drop(root);
            verify_source(&s);
        }
    }
}

#[test]
fn undeclared_file_components_and_v1_refuse_policy_admin_without_implicit_provisioning() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, compact);
        let (id, key) = &s.root.credentials[0];
        let m = manifest(&path);
        // Existing v2 format requires private membership for every file binding.
        assert!(AccountFileRootManifest::new(Vec::new(), m.file_stores().to_vec(), 40).is_err());
        let files = path.join("files").join(id);
        let saved = t.path().join("retained-files");
        fs::rename(&files, &saved).unwrap();
        fs::write(
            path.join("root.json"),
            AccountFileRootManifest::new(m.private_projects().to_vec(), Vec::new(), 40)
                .unwrap()
                .to_bytes()
                .unwrap(),
        )
        .unwrap();
        let wal = private_wal(&path, id);
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        assert!(matches!(
            root.list_project_files(id, key),
            Err(Error::Denied)
        ));
        assert!(matches!(
            root.project_file_policy(id, key),
            Err(Error::Denied)
        ));
        assert!(matches!(
            root.enable_file_policy_catalog(id, key),
            Err(Error::Denied)
        ));
        assert!(matches!(
            root.install_project_file_policy(id, key, 0, OWN),
            Err(Error::Denied)
        ));
        assert_eq!(root.projects().unwrap().len(), 1);
        assert!(!files.exists());
        assert_eq!(private_wal(&path, id), wal);
        assert!(saved.join("metadata/redo.wal").is_file());
        drop(root);
        let mut v1 = AccountRoot::open(&s.root.root, pool()).unwrap();
        assert!(matches!(
            v1.project_file_policy(id, key),
            Err(Error::Denied)
        ));
        assert!(matches!(
            v1.enable_file_policy_catalog(id, key),
            Err(Error::Denied)
        ));
        assert!(matches!(
            v1.install_project_file_policy(id, key, 0, OWN),
            Err(Error::Denied)
        ));
        drop(v1);
        verify_source(&s);
    }
}
