//! User edits exercise real sessions, installed policies and original owners.
use super::*;
use emilybase_auth::{
    accounts::IssuedSession,
    file_policy::{CreateRule, Definition, Rule},
};
use std::os::unix::fs::MetadataExt;

const ALICE: FileId = FileId::from_bytes([81; 16]);
const BOB_FILE: FileId = FileId::from_bytes([83; 16]);
const MISSING: FileId = FileId::from_bytes([87; 16]);
const POINTS: [&str; 7] = [
    "account_user_file_write_selected",
    "account_user_file_write_before_commit",
    "account_user_file_write_commit_admitted",
    "account_user_file_write_committed",
    "account_user_file_write_verified",
    "account_user_file_result",
    "account_user_file_write_returned",
];
fn policy(root: &mut AccountRoot, id: &str, key: &str, read: Rule, rename: Rule, remove: Rule) {
    let expected = root
        .project_file_policy(id, key)
        .unwrap()
        .map_or(0, |p| p.revision);
    let bytes = serde_json::to_vec(&Definition {
        version: 1,
        read,
        create: CreateRule::Deny {},
        rename,
        remove,
    })
    .unwrap();
    root.install_project_file_policy(id, key, expected, &bytes)
        .unwrap();
}
fn activate(root: &mut AccountRoot, id: &str, key: &str) -> IssuedSession {
    let session = super::user_files::activate(root, id, key, Rule::Deny {});
    policy(
        root,
        id,
        key,
        Rule::Deny {},
        Rule::Authenticated {},
        Rule::Authenticated {},
    );
    session
}
fn run(
    root: &mut AccountRoot,
    id: &str,
    access: &str,
    now: u64,
    file: FileId,
    expected: u64,
    remove: bool,
) -> crate::Result<crate::AccountFileWrite> {
    if remove {
        root.public_remove_file(id, access, now, file, expected)
    } else {
        root.public_rename_file(id, access, now, file, expected, "user-renamed")
    }
}
fn committed(point: &str) -> bool {
    matches!(
        point,
        "account_user_file_write_committed"
            | "account_user_file_write_verified"
            | "account_user_file_write_native_returned"
            | "account_user_file_result"
            | "account_user_file_write_returned"
    )
}
fn assert_native(
    root: &mut AccountRoot,
    id: &str,
    key: &str,
    file: FileId,
    remove: bool,
    changed: bool,
    revision: u64,
) {
    let info = root.project_file_info(id, key, file).unwrap();
    if remove && changed {
        assert!(info.is_none());
    } else {
        let info = info.unwrap();
        assert_eq!(info.revision(), revision + u64::from(changed));
        assert_eq!(
            info.name(),
            if changed {
                "user-renamed"
            } else {
                "synthetic-name"
            }
        );
    }
    let state = root.inspect_project_files(id, key).unwrap();
    assert_eq!(state.revision(), revision + u64::from(changed));
    assert_eq!(state.usage().physical_objects, 2);
    assert_eq!(state.usage().references, usize::from(!(remove && changed)));
    assert_eq!(state.usage().orphans, 1 + usize::from(remove && changed));
}

#[test]
fn current_action_rules_apply_to_actual_owners_without_read_or_service_key_authority() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 2, compact);
        let (id, key) = &s.root.credentials[0];
        let (other, other_key) = &s.root.credentials[1];
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let (alice, bob, alice_id, _) = super::user_files::setup(&mut root, id, key);
        let foreign = activate(&mut root, other, other_key);
        policy(
            &mut root,
            id,
            key,
            Rule::Deny {},
            Rule::Owner {},
            Rule::Owner {},
        );
        let original = root.project_file_info(id, key, ALICE).unwrap().unwrap();
        let usage = root.inspect_project_files(id, key).unwrap().usage();
        let before = copied_history(&s, &path);
        for remove in [false, true] {
            for file in [ALICE, MISSING] {
                assert!(matches!(
                    run(&mut root, id, bob.access.expose(), 101, file, 0, remove),
                    Err(Error::Denied)
                ));
            }
            for token in [
                key.as_str(),
                alice.refresh.expose(),
                foreign.access.expose(),
            ] {
                assert!(
                    run(
                        &mut root,
                        id,
                        token,
                        101,
                        ALICE,
                        original.revision(),
                        remove
                    )
                    .is_err()
                );
            }
            assert!(
                run(
                    &mut root,
                    other,
                    alice.access.expose(),
                    101,
                    ALICE,
                    original.revision(),
                    remove
                )
                .is_err()
            );
        }
        assert_eq!(copied_history(&s, &path), before);
        let receipt = run(
            &mut root,
            id,
            alice.access.expose(),
            101,
            ALICE,
            original.revision(),
            false,
        )
        .unwrap();
        assert_eq!(receipt.id(), ALICE);
        assert_eq!(receipt.previous_revision(), original.revision());
        let changed = root.project_file_info(id, key, ALICE).unwrap().unwrap();
        assert_eq!(changed.revision(), receipt.revision());
        assert_eq!(changed.owner(), &alice_id);
        assert_eq!(changed.object(), original.object());
        assert_eq!(changed.report(), original.report());
        assert_eq!(root.inspect_project_files(id, key).unwrap().usage(), usage);
        assert!(
            root.public_file_info(id, alice.access.expose(), 101, ALICE)
                .unwrap()
                .is_none()
        );
        assert!(
            root.public_list_files(id, alice.access.expose(), 101)
                .unwrap()
                .is_empty()
        );
        for text in [
            ALICE.to_string(),
            original.object().to_string(),
            "user-renamed".into(),
            "alice-payload".into(),
        ] {
            assert!(!format!("{receipt:?}").contains(&text));
        }
        policy(
            &mut root,
            id,
            key,
            Rule::Deny {},
            Rule::Authenticated {},
            Rule::Deny {},
        );
        let b = root.project_file_info(id, key, BOB_FILE).unwrap().unwrap();
        let renamed = run(
            &mut root,
            id,
            alice.access.expose(),
            101,
            BOB_FILE,
            b.revision(),
            false,
        )
        .unwrap();
        assert!(matches!(
            run(
                &mut root,
                id,
                alice.access.expose(),
                101,
                BOB_FILE,
                renamed.revision(),
                true
            ),
            Err(Error::Denied)
        ));
        let key = root.rotate_project_key(id).unwrap().api_key;
        policy(
            &mut root,
            id,
            &key,
            Rule::Deny {},
            Rule::Deny {},
            Rule::Authenticated {},
        );
        let removed = run(
            &mut root,
            id,
            bob.access.expose(),
            101,
            ALICE,
            receipt.revision(),
            true,
        )
        .unwrap();
        assert_eq!(removed.previous_revision(), receipt.revision());
        assert!(root.project_file_info(id, &key, ALICE).unwrap().is_none());
        let after = root.inspect_project_files(id, &key).unwrap().usage();
        assert_eq!(after.physical_objects, usage.physical_objects);
        assert_eq!(after.orphans, usage.orphans + 1);
        drop(root);
        verify_source(&s);
    }
}

#[test]
fn authorized_cas_noops_invalid_names_and_missing_references_preserve_exact_histories() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, compact);
        let (id, key) = &s.root.credentials[0];
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let session = activate(&mut root, id, key);
        let token = session.access.expose();
        let info = root.project_file_info(id, key, FILE).unwrap().unwrap();
        let before = copied_history(&s, &path);
        let noop = root
            .public_rename_file(id, token, 101, FILE, info.revision(), info.name())
            .unwrap();
        assert_eq!(noop.revision(), noop.previous_revision());
        assert_eq!(noop.id(), FILE);
        for remove in [false, true] {
            assert!(matches!(
                run(&mut root, id, token, 101, FILE, 0, remove),
                Err(Error::Files(emilybase_files::Error::Conflict))
            ));
            assert!(matches!(
                run(&mut root, id, token, 101, MISSING, info.revision(), remove),
                Err(Error::Denied)
            ));
        }
        for name in [
            "",
            "\n",
            &"x".repeat(emilybase_files::MAX_FILE_NAME_BYTES + 1),
        ] {
            assert!(matches!(
                root.public_rename_file(id, token, 101, FILE, info.revision(), name),
                Err(Error::Files(emilybase_files::Error::Name))
            ));
        }
        assert_eq!(copied_history(&s, &path), before);
        let receipt = run(&mut root, id, token, 101, FILE, info.revision(), false).unwrap();
        drop(root);
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let removed = run(&mut root, id, token, 101, FILE, receipt.revision(), true).unwrap();
        assert!(removed.revision() > receipt.revision());
        assert!(matches!(
            run(&mut root, id, token, 101, FILE, receipt.revision(), true),
            Err(Error::Denied)
        ));
        drop(root);
        verify_source(&s);
    }
}

#[test]
fn reused_identity_authorizes_the_new_native_owner_before_disclosing_stale_cas() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, compact);
        let (id, key) = &s.root.credentials[0];
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let (alice, bob, _, bob_id) = super::user_files::setup(&mut root, id, key);
        policy(
            &mut root,
            id,
            key,
            Rule::Deny {},
            Rule::Owner {},
            Rule::Owner {},
        );
        let old = root.project_file_info(id, key, ALICE).unwrap().unwrap();
        root.remove_project_file(id, key, ALICE, old.revision())
            .unwrap();
        let current = root
            .publish_project_file(
                id,
                key,
                crate::ProjectFileUpload {
                    id: ALICE,
                    object: ObjectId::from_bytes([90; 16]),
                    owner: bob_id,
                    name: "new-owner",
                    payload: b"new-owner-payload",
                },
            )
            .unwrap();
        let before = copied_history(&s, &path);
        for remove in [false, true] {
            for revision in [0, old.revision(), current.revision()] {
                assert!(matches!(
                    run(
                        &mut root,
                        id,
                        alice.access.expose(),
                        101,
                        ALICE,
                        revision,
                        remove
                    ),
                    Err(Error::Denied)
                ));
            }
            assert!(matches!(
                run(
                    &mut root,
                    id,
                    bob.access.expose(),
                    101,
                    ALICE,
                    old.revision(),
                    remove
                ),
                Err(Error::Files(emilybase_files::Error::Conflict))
            ));
        }
        assert_eq!(copied_history(&s, &path), before);
        let receipt = run(
            &mut root,
            id,
            bob.access.expose(),
            101,
            ALICE,
            current.revision(),
            false,
        )
        .unwrap();
        assert_eq!(receipt.previous_revision(), current.revision());
        assert_eq!(
            root.project_file_info(id, key, ALICE)
                .unwrap()
                .unwrap()
                .owner(),
            &bob_id
        );
        drop(root);
        verify_source(&s);
    }
}

#[test]
fn forward_denied_and_allowed_edits_advance_one_private_clock_without_inventing_file_commits() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, compact);
        let (id, key) = &s.root.credentials[0];
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let (alice, bob, _, _) = super::user_files::setup(&mut root, id, key);
        policy(
            &mut root,
            id,
            key,
            Rule::Deny {},
            Rule::Owner {},
            Rule::Owner {},
        );
        let private = path.join("private").join(id).join("redo.wal");
        let native = path.join("files").join(id).join("metadata/redo.wal");
        let last = |path: &Path| {
            emilybase_transactions::recover_image(&fs::read(path).unwrap(), None)
                .unwrap()
                .last_transaction
        };
        let clock = last(&private);
        let native_lsn = last(&native);
        let bytes = fs::read(&native).unwrap();
        let old = root.project_file_info(id, key, ALICE).unwrap().unwrap();
        for _ in 0..2 {
            assert!(matches!(
                run(
                    &mut root,
                    id,
                    bob.access.expose(),
                    102,
                    ALICE,
                    old.revision(),
                    false
                ),
                Err(Error::Denied)
            ));
            assert_eq!(last(&private), clock + 1);
            assert_eq!(fs::read(&native).unwrap(), bytes);
            assert!(root.projects().is_ok());
        }
        let receipt = run(
            &mut root,
            id,
            alice.access.expose(),
            103,
            ALICE,
            old.revision(),
            false,
        )
        .unwrap();
        assert_eq!(last(&private), clock + 2);
        assert_eq!(receipt.revision(), native_lsn + 1);
        assert_eq!(receipt.previous_revision(), old.revision());
        drop(root);
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let before = copied_history(&s, &path);
        assert!(
            run(
                &mut root,
                id,
                alice.access.expose(),
                102,
                ALICE,
                receipt.revision(),
                true
            )
            .is_err()
        );
        assert_eq!(copied_history(&s, &path), before);
        drop(root);
        verify_source(&s);
    }
}

#[test]
fn closed_admission_and_current_session_revocation_refuse_before_mutation_admission() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for change in 0..6 {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), 1, compact);
            let (id, key) = &s.root.credentials[0];
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            let session = activate(&mut root, id, key);
            let mut now = 101;
            match change {
                0 => {
                    let a = root.public_admission(id, key).unwrap();
                    root.set_public_admission(id, key, a.revision, false)
                        .unwrap();
                }
                1 => root
                    .logout_session(id, key, session.refresh.expose(), 101)
                    .unwrap(),
                2 => {
                    root.refresh_session(id, key, session.refresh.expose(), 101)
                        .unwrap();
                }
                3 => {
                    root.set_disabled(id, key, LOGIN, true).unwrap();
                    root.set_disabled(id, key, LOGIN, false).unwrap();
                }
                4 => {
                    root.change_password(id, key, LOGIN, PASSWORD, b"synthetic-new-password")
                        .unwrap();
                }
                _ => now = u64::MAX - 1,
            }
            let native = fs::read(path.join("files").join(id).join("metadata/redo.wal")).unwrap();
            let _hook = durability::on_boundary("account_user_file_write_before_operation", || {
                panic!("revoked session reached mutation")
            });
            for remove in [false, true] {
                assert!(run(&mut root, id, session.access.expose(), now, FILE, 3, remove).is_err());
            }
            drop(_hook);
            drop(root);
            assert_eq!(
                fs::read(path.join("files").join(id).join("metadata/redo.wal")).unwrap(),
                native
            );
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            for remove in [false, true] {
                assert!(run(&mut root, id, session.access.expose(), now, FILE, 3, remove).is_err());
            }
            drop(root);
            verify_source(&s);
        }
    }
}

#[test]
fn real_selected_and_final_metadata_substitutions_refuse_and_close_whole_root_after_possible_commit()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for remove in [false, true] {
            for point in [
                "account_user_file_write_selected",
                "account_user_file_result",
            ] {
                for target in 0..4 {
                    let t = tempfile::tempdir().unwrap();
                    let (s, path) = copy(t.path(), 1, compact);
                    let (id, key) = &s.root.credentials[0];
                    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                    let session = activate(&mut root, id, key);
                    let wal = path.join("files").join(id).join("metadata/redo.wal");
                    let before = fs::read(&wal).unwrap();
                    let selected = match target {
                        0 => path.join("root.json"),
                        1 => path.join("registry").join(id).join("project.json"),
                        2 => path.join("private").join(id).join("redo.wal"),
                        _ => wal.clone(),
                    };
                    let saved = t.path().join("original");
                    let changed = selected.clone();
                    let retained = saved.clone();
                    let _hook = durability::on_boundary(point, move || {
                        fs::rename(&changed, &retained).unwrap();
                        fs::copy(&retained, &changed).unwrap();
                    });
                    let result = run(&mut root, id, session.access.expose(), 101, FILE, 3, remove);
                    drop(_hook);
                    assert!(result.is_err());
                    assert_eq!(
                        matches!(result, Err(Error::AccountFileWriteUnknown(_))),
                        committed(point)
                    );
                    fs::remove_file(&selected).unwrap();
                    fs::rename(&saved, &selected).unwrap();
                    if committed(point) {
                        assert!(matches!(
                            root.projects(),
                            Err(Error::AccountFileWritePoisoned)
                        ));
                    }
                    drop(root);
                    if !committed(point) {
                        assert_eq!(fs::read(&wal).unwrap(), before);
                    }
                    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                    assert_native(&mut root, id, key, FILE, remove, committed(point), 3);
                    root.execute(id, key, "UPDATE t SET v=98 WHERE id=1", &[])
                        .unwrap();
                    drop(root);
                    verify_source(&s);
                }
            }
        }
    }
}

#[test]
fn unwind_before_and_after_commit_admission_preserves_data_and_blocks_whole_root_after_possible_write()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for remove in [false, true] {
            for point in POINTS[..6].iter().copied().chain([
                "account_user_file_write_before_operation",
                "account_user_file_write_native_returned",
            ]) {
                let t = tempfile::tempdir().unwrap();
                let (s, path) = copy(t.path(), 1, compact);
                let (id, key) = &s.root.credentials[0];
                let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                let session = activate(&mut root, id, key);
                let _hook =
                    durability::on_boundary(point, || panic!("trusted synthetic write unwind"));
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&mut root, id, session.access.expose(), 101, FILE, 3, remove)
                }));
                drop(_hook);
                assert!(result.is_err());
                if committed(point) || point == "account_user_file_write_commit_admitted" {
                    assert!(matches!(
                        root.projects(),
                        Err(Error::AccountFileWritePoisoned)
                    ));
                }
                drop(root);
                let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                assert_native(&mut root, id, key, FILE, remove, committed(point), 3);
                root.execute(id, key, "UPDATE t SET v=99 WHERE id=1", &[])
                    .unwrap();
                drop(root);
                verify_source(&s);
            }
        }
    }
}

#[test]
fn actual_signal9_user_edits_recover_confirmed_changes_release_owners_and_allow_independent_writes()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for remove in [false, true] {
            for point in POINTS {
                let t = tempfile::tempdir().unwrap();
                let (s, path) = copy(t.path(), 1, compact);
                let (id, key) = &s.root.credentials[0];
                let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                let session = activate(&mut root, id, key);
                drop(root);
                let token = t.path().join("access");
                fs::write(&token, session.access.expose()).unwrap();
                fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
                let before = copied_history(&s, &path);
                let action = if remove {
                    "root-v2-user-file-edit-remove"
                } else {
                    "root-v2-user-file-edit-rename"
                };
                let worker = Worker::start(&path, &token, id, action, point);
                worker.reach(point);
                assert!(AccountStore::open(path.join("private").join(id), id, pool()).is_err());
                worker.kill();
                if !committed(point) {
                    assert_eq!(copied_history(&s, &path), before);
                }
                let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                assert_native(&mut root, id, key, FILE, remove, committed(point), 3);
                root.execute(id, key, "UPDATE t SET v=97 WHERE id=1", &[])
                    .unwrap();
                let next = root
                    .publish_project_file(
                        id,
                        key,
                        crate::ProjectFileUpload {
                            id: MISSING,
                            object: ObjectId::from_bytes([88; 16]),
                            owner: [9; 16],
                            name: "independent",
                            payload: b"new",
                        },
                    )
                    .unwrap();
                assert_eq!(next.revision(), if committed(point) { 5 } else { 4 });
                drop(root);
                verify_source(&s);
            }
        }
    }
}

#[test]
fn refused_verified_noop_does_not_claim_a_file_write_or_close_an_unchanged_root() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, false);
    let (id, key) = &s.root.credentials[0];
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    let session = activate(&mut root, id, key);
    let info = root.project_file_info(id, key, FILE).unwrap().unwrap();
    let selected = path.join("root.json");
    let original = selected.clone();
    let saved = t.path().join("saved");
    let retained = saved.clone();
    let before = copied_history(&s, &path);
    let _hook = durability::on_boundary("account_user_file_result", move || {
        fs::rename(&selected, &retained).unwrap();
        fs::copy(&retained, &selected).unwrap();
    });
    let result = root.public_rename_file(
        id,
        session.access.expose(),
        101,
        FILE,
        info.revision(),
        info.name(),
    );
    drop(_hook);
    assert!(result.is_err());
    assert!(!matches!(result, Err(Error::AccountFileWriteUnknown(_))));
    fs::remove_file(&original).unwrap();
    fs::rename(&saved, &original).unwrap();
    assert_eq!(copied_history(&s, &path), before);
    assert!(root.projects().is_ok());
    run(
        &mut root,
        id,
        session.access.expose(),
        101,
        FILE,
        info.revision(),
        false,
    )
    .unwrap();
}

fn fds(path: &Path) -> usize {
    let expected = fs::metadata(path).unwrap();
    fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|e| fs::metadata(e.ok()?.path()).ok())
        .filter(|m| (m.dev(), m.ino()) == (expected.dev(), expected.ino()))
        .count()
}
#[test]
fn original_common_private_project_native_and_payload_owners_span_actual_user_edit_phases() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for remove in [false, true] {
        for point in [
            "account_user_file_write_selected",
            "account_user_file_write_before_commit",
            "account_user_file_write_committed",
            "account_user_file_write_verified",
            "account_user_file_result",
        ] {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), 1, true);
            let (id, key) = &s.root.credentials[0];
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            let session = activate(&mut root, id, key);
            let info = root.project_file_info(id, key, FILE).unwrap().unwrap();
            let object = info.object();
            let selected = path.clone();
            let project = id.clone();
            let calls = Arc::new(AtomicUsize::new(0));
            let observed = calls.clone();
            let _hook = durability::on_boundary(point, move || {
                observed.fetch_add(1, Ordering::SeqCst);
                for file in [
                    selected.join("root.json"),
                    selected
                        .join("registry")
                        .join(&project)
                        .join("project.json"),
                    selected.join("private").join(&project).join("redo.wal"),
                    selected
                        .join("files")
                        .join(&project)
                        .join("metadata/redo.wal"),
                ] {
                    assert!(fds(&file) > 0);
                }
                if matches!(
                    point,
                    "account_user_file_write_before_commit"
                        | "account_user_file_write_committed"
                        | "account_user_file_write_verified"
                ) {
                    assert!(
                        fds(&selected
                            .join("files")
                            .join(&project)
                            .join("objects")
                            .join(format!("{object}.object")))
                            > 0
                    );
                }
                assert!(
                    AccountStore::open(selected.join("private").join(&project), &project, pool())
                        .is_err()
                );
                assert!(
                    Database::open(selected.join("registry").join(&project).join("data")).is_ok()
                );
            });
            let receipt =
                run(&mut root, id, session.access.expose(), 101, FILE, 3, remove).unwrap();
            drop(_hook);
            assert_eq!(receipt.revision(), 4);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }
}

#[test]
fn independent_restore_keeps_owner_policy_and_new_user_edits_after_explicit_admission() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, compact);
        let (id, key) = &s.root.credentials[0];
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let (alice, _, _, _) = super::user_files::setup(&mut root, id, key);
        policy(
            &mut root,
            id,
            key,
            Rule::Deny {},
            Rule::Owner {},
            Rule::Owner {},
        );
        let old = root.project_file_info(id, key, ALICE).unwrap().unwrap();
        let receipt = run(
            &mut root,
            id,
            alice.access.expose(),
            101,
            ALICE,
            old.revision(),
            false,
        )
        .unwrap();
        let before = copied_history(&s, &path);
        let bytes = encode(&root.capture_v2().unwrap());
        crate::restore_account_file_archive_bytes_at(
            &bytes,
            &File::open(t.path()).unwrap(),
            "user-edit-copy",
            pool(),
            50,
        )
        .unwrap();
        let mut copy = AccountRoot::open_v2(t.path().join("user-edit-copy"), pool()).unwrap();
        assert!(
            run(
                &mut copy,
                id,
                alice.access.expose(),
                101,
                ALICE,
                receipt.revision(),
                true
            )
            .is_err()
        );
        let a = copy.public_admission(id, key).unwrap();
        assert!(!a.enabled);
        copy.set_public_admission(id, key, a.revision, true)
            .unwrap();
        let fresh = copy.public_sign_in(id, LOGIN, PASSWORD, 50).unwrap();
        let removed = run(
            &mut copy,
            id,
            fresh.access.expose(),
            50,
            ALICE,
            receipt.revision(),
            true,
        )
        .unwrap();
        assert_eq!(removed.previous_revision(), receipt.revision());
        assert_eq!(
            root.project_file_info(id, key, ALICE)
                .unwrap()
                .unwrap()
                .revision(),
            receipt.revision()
        );
        assert_eq!(copied_history(&s, &path), before);
        drop(copy);
        drop(root);
        verify_source(&s);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]
    #[test]
    fn generated_current_action_rule_and_owner_decisions_match_independent_model(
        compact in any::<bool>(), bob in any::<bool>(), rule in 0u8..3, remove in any::<bool>(), stale in any::<bool>()
    ) {
        let _serial = durability::PROCESS_TESTS.blocking_lock(); let t = tempfile::tempdir().unwrap(); let (s,path)=copy(t.path(),1,compact);
        let(id,key)=&s.root.credentials[0];let mut root=AccountRoot::open_v2(&path,pool()).unwrap();
        let(a,b,_,_)=super::user_files::setup(&mut root,id,key);let access=if bob { b.access.expose() } else { a.access.expose() };
        let selected=match rule {0=>Rule::Deny{},1=>Rule::Owner{},_=>Rule::Authenticated{}};
        policy(&mut root,id,key,Rule::Deny{},selected,selected);let old=root.project_file_info(id,key,ALICE).unwrap().unwrap();let before=copied_history(&s,&path);
        let permitted=rule==2||(rule==1&&!bob);let result=run(&mut root,id,access,101,ALICE,if stale {0}else{old.revision()},remove);
        if !permitted {prop_assert!(matches!(result,Err(Error::Denied)));}
        else if stale {prop_assert!(matches!(result,Err(Error::Files(emilybase_files::Error::Conflict))));}
        else {prop_assert!(result.is_ok());}
        if !permitted||stale {prop_assert_eq!(copied_history(&s,&path),before);}
        else {let current=root.project_file_info(id,key,ALICE).unwrap();prop_assert_eq!(current.is_none(),remove);if let Some(current)=current {prop_assert_eq!(current.owner(),old.owner());prop_assert_eq!(current.object(),old.object());prop_assert_eq!(current.name(),"user-renamed");}}
        drop(root);verify_source(&s);
    }
}
