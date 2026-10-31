//! Real current-user publication: ownership, quotas, failure and durable recovery.
use super::*;
use crate::AccountFileUpload;
use emilybase_auth::{
    accounts::IssuedSession,
    file_policy::{CreateRule, Definition, Rule},
};
use std::os::unix::fs::MetadataExt;

const FRESH: FileId = FileId::from_bytes([91; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([92; 16]);
const BYTES: &[u8] = b"user-upload-payload";
const POINTS: [&str; 8] = [
    "account_user_file_upload_before_operation",
    "account_user_file_upload_before_blob",
    "account_user_file_upload_blob_admitted",
    "account_user_file_upload_before_commit",
    "account_user_file_upload_committed",
    "account_user_file_upload_verified",
    "account_user_file_upload_native_returned",
    "account_user_file_result",
];
fn upload(payload: &[u8]) -> AccountFileUpload<'_> {
    AccountFileUpload {
        id: FRESH,
        object: OBJECT,
        name: "user-upload",
        payload,
    }
}
fn policy(root: &mut AccountRoot, id: &str, key: &str, permit: bool, read: Rule) {
    let expected = root.project_file_policy(id, key).unwrap().unwrap().revision;
    let bytes = serde_json::to_vec(&Definition {
        version: 1,
        read,
        create: if permit {
            CreateRule::SelfOwned {}
        } else {
            CreateRule::Deny {}
        },
        rename: Rule::Deny {},
        remove: Rule::Deny {},
    })
    .unwrap();
    root.install_project_file_policy(id, key, expected, &bytes)
        .unwrap();
}
fn activate(root: &mut AccountRoot, id: &str, key: &str) -> IssuedSession {
    let session = super::user_files::activate(root, id, key, Rule::Deny {});
    policy(root, id, key, true, Rule::Deny {});
    session
}
fn identity(root: &mut AccountRoot, id: &str, key: &str, session: &IssuedSession) -> [u8; 16] {
    root.with_access(id, key, session.access.expose(), 101, |p| p.account().id)
        .unwrap()
}
fn committed(point: &str) -> bool {
    matches!(
        point,
        "account_user_file_upload_committed"
            | "account_user_file_upload_verified"
            | "account_user_file_upload_native_returned"
            | "account_user_file_result"
            | "account_user_file_upload_returned"
    )
}
fn has_blob(point: &str) -> bool {
    committed(point) || point == "account_user_file_upload_before_commit"
}
fn last(path: &Path) -> u64 {
    emilybase_transactions::recover_image(&fs::read(path).unwrap(), None)
        .unwrap()
        .last_transaction
}
fn assert_state(root: &mut AccountRoot, id: &str, key: &str, blob: bool, commit: bool) {
    let state = root.inspect_project_files(id, key).unwrap();
    assert_eq!(state.revision(), 3 + u64::from(commit));
    assert_eq!(state.usage().physical_objects, 2 + usize::from(blob));
    assert_eq!(state.usage().references, 1 + usize::from(commit));
    assert_eq!(state.usage().orphans, 1 + usize::from(blob && !commit));
    let entry = root.project_file_info(id, key, FRESH).unwrap();
    assert_eq!(entry.is_some(), commit);
    if let Some(entry) = entry {
        assert_eq!(entry.object(), OBJECT);
        assert_eq!(entry.name(), "user-upload");
        assert_eq!(entry.revision(), 4);
        assert_eq!(
            root.read_project_file(id, key, FRESH, 32)
                .unwrap()
                .payload(),
            BYTES
        );
    }
    assert_eq!(
        root.project_file_info(id, key, FILE)
            .unwrap()
            .unwrap()
            .revision(),
        3
    );
}

#[test]
fn actual_session_owns_new_files_with_independent_read_policy_and_project_isolation() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 2, compact);
        let (id, key) = &s.root.credentials[0];
        let (other, other_key) = &s.root.credentials[1];
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let alice = activate(&mut root, id, key);
        root.create_user(id, key, "synthetic_bob", PASSWORD)
            .unwrap();
        let bob = root
            .sign_in(id, key, "synthetic_bob", PASSWORD, 101)
            .unwrap();
        let a = identity(&mut root, id, key, &alice);
        let b = identity(&mut root, id, key, &bob);
        assert_ne!(a, b);
        let foreign = activate(&mut root, other, other_key);
        let before = copied_history(&s, &path);
        for token in [
            key.as_str(),
            alice.refresh.expose(),
            foreign.access.expose(),
        ] {
            assert!(
                root.public_publish_file(id, token, 101, upload(BYTES))
                    .is_err()
            );
        }
        assert!(
            root.public_publish_file(other, alice.access.expose(), 101, upload(BYTES))
                .is_err()
        );
        assert_eq!(copied_history(&s, &path), before);
        let other_before = root.inspect_project_files(other, other_key).unwrap();
        let receipt = root
            .public_publish_file(id, alice.access.expose(), 101, upload(BYTES))
            .unwrap();
        assert_eq!(receipt.id(), FRESH);
        assert_eq!(receipt.previous_revision(), 0);
        assert_eq!(receipt.revision(), 4);
        let stored = root.project_file_info(id, key, FRESH).unwrap().unwrap();
        assert_eq!(stored.owner(), &a);
        assert_eq!(stored.object(), OBJECT);
        assert!(
            root.public_file_info(id, alice.access.expose(), 101, FRESH)
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            root.public_read_file(id, alice.access.expose(), 101, FRESH, 32),
            Err(Error::Denied)
        ));
        assert_eq!(
            root.inspect_project_files(other, other_key).unwrap(),
            other_before
        );
        let rotated = root.rotate_project_key(id).unwrap();
        let second = FileId::from_bytes([93; 16]);
        let receipt2 = root
            .public_publish_file(
                id,
                bob.access.expose(),
                101,
                AccountFileUpload {
                    id: second,
                    object: ObjectId::from_bytes([94; 16]),
                    ..upload(b"bob")
                },
            )
            .unwrap();
        assert_eq!(receipt2.revision(), 5);
        assert_eq!(
            root.project_file_info(id, &rotated.api_key, second)
                .unwrap()
                .unwrap()
                .owner(),
            &b
        );
        policy(&mut root, id, &rotated.api_key, true, Rule::Owner {});
        assert_eq!(
            root.public_read_file(id, alice.access.expose(), 101, FRESH, 32)
                .unwrap()
                .payload(),
            BYTES
        );
        assert!(matches!(
            root.public_read_file(id, bob.access.expose(), 101, FRESH, 32),
            Err(Error::Denied)
        ));
        for text in [
            FRESH.to_string(),
            OBJECT.to_string(),
            "user-upload".into(),
            "user-upload-payload".into(),
        ] {
            assert!(!format!("{receipt:?}").contains(&text));
            assert!(!format!("{:?}", upload(BYTES)).contains(&text));
        }
        drop(root);
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        assert_eq!(
            root.project_file_info(id, &rotated.api_key, FRESH)
                .unwrap()
                .unwrap(),
            stored
        );
        drop(root);
        verify_source(&s);
    }
}

#[test]
fn denied_create_precedes_validation_collision_and_quota_without_entering_native_body() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, compact);
        let (id, key) = &s.root.credentials[0];
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let alice = activate(&mut root, id, key);
        let info = root.project_file_info(id, key, FILE).unwrap().unwrap();
        policy(&mut root, id, key, false, Rule::Authenticated {});
        let state = root.project_file_quota(id, key).unwrap();
        let bytes = root
            .inspect_project_files(id, key)
            .unwrap()
            .usage()
            .payload_bytes;
        root.set_project_file_quota(id, key, state, FileQuota::new(2, bytes).unwrap())
            .unwrap();
        let before = copied_history(&s, &path);
        let _hook =
            durability::on_boundary(POINTS[0], || panic!("denied create entered native write"));
        for request in [
            upload(BYTES),
            AccountFileUpload {
                id: FILE,
                ..upload(BYTES)
            },
            AccountFileUpload {
                object: info.object(),
                ..upload(BYTES)
            },
            AccountFileUpload {
                name: "../forbidden",
                ..upload(BYTES)
            },
        ] {
            assert!(matches!(
                root.public_publish_file(id, alice.access.expose(), 101, request),
                Err(Error::Denied)
            ));
        }
        drop(_hook);
        assert_eq!(copied_history(&s, &path), before);
        assert!(root.projects().is_ok());
        policy(&mut root, id, key, true, Rule::Deny {});
        let before = copied_history(&s, &path);
        assert!(
            root.public_publish_file(id, alice.access.expose(), 101, upload(BYTES))
                .is_err()
        );
        assert_eq!(copied_history(&s, &path), before);
        drop(root);
        verify_source(&s);
    }
}

#[test]
fn ordinary_duplicates_and_full_quota_preserve_original_objects_and_leave_root_usable() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, compact);
        let (id, key) = &s.root.credentials[0];
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let alice = activate(&mut root, id, key);
        let original = root.project_file_info(id, key, FILE).unwrap().unwrap();
        let before = copied_history(&s, &path);
        assert!(matches!(
            root.public_publish_file(
                id,
                alice.access.expose(),
                101,
                AccountFileUpload {
                    id: FILE,
                    ..upload(BYTES)
                }
            ),
            Err(Error::Files(emilybase_files::Error::Exists))
        ));
        for object in [original.object(), ObjectId::from_bytes([5; 16])] {
            assert!(matches!(
                root.public_publish_file(
                    id,
                    alice.access.expose(),
                    101,
                    AccountFileUpload {
                        object,
                        ..upload(BYTES)
                    }
                ),
                Err(Error::Files(emilybase_files::Error::Objects(
                    emilybase_object_storage::Error::Exists
                )))
            ));
        }
        assert_eq!(copied_history(&s, &path), before);
        let usage = root.inspect_project_files(id, key).unwrap().usage();
        let state = root.project_file_quota(id, key).unwrap();
        root.set_project_file_quota(
            id,
            key,
            state,
            FileQuota::new(2, usage.payload_bytes).unwrap(),
        )
        .unwrap();
        let before = copied_history(&s, &path);
        for payload in [b"".as_slice(), BYTES] {
            assert!(matches!(
                root.public_publish_file(id, alice.access.expose(), 101, upload(payload)),
                Err(Error::Files(emilybase_files::Error::Objects(
                    emilybase_object_storage::Error::Limit
                )))
            ));
            assert!(root.projects().is_ok());
        }
        assert_eq!(copied_history(&s, &path), before);
        let state = root.project_file_quota(id, key).unwrap();
        root.set_project_file_quota(
            id,
            key,
            state,
            FileQuota::new(3, usage.payload_bytes + BYTES.len() as u64).unwrap(),
        )
        .unwrap();
        let revision = root.inspect_project_files(id, key).unwrap().revision();
        let receipt = root
            .public_publish_file(id, alice.access.expose(), 101, upload(BYTES))
            .unwrap();
        assert_eq!(receipt.revision(), revision + 1);
        assert_eq!(
            root.project_file_info(id, key, FILE).unwrap().unwrap(),
            original
        );
        assert_eq!(
            root.inspect_project_files(id, key)
                .unwrap()
                .usage()
                .physical_objects,
            3
        );
        // Removal changes visibility, not physical capacity. Reusing the logical
        // ID requires a fresh immutable object and still reports creation absence.
        root.remove_project_file(id, key, FRESH, receipt.revision())
            .unwrap();
        let state = root.project_file_quota(id, key).unwrap();
        root.set_project_file_quota(id, key, state, FileQuota::new(4, 65536).unwrap())
            .unwrap();
        let recreated = root
            .public_publish_file(
                id,
                alice.access.expose(),
                101,
                AccountFileUpload {
                    object: ObjectId::from_bytes([94; 16]),
                    ..upload(b"recreated")
                },
            )
            .unwrap();
        assert_eq!(recreated.previous_revision(), 0);
        let usage = root.inspect_project_files(id, key).unwrap().usage();
        assert_eq!(usage.physical_objects, 4);
        assert_eq!(usage.references, 2);
        assert_eq!(usage.orphans, 2);
        drop(root);
        verify_source(&s);
    }
}

#[test]
fn bounded_names_payloads_and_empty_files_have_typed_failures_and_exact_receipts() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, true);
    let (id, key) = &s.root.credentials[0];
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    let session = activate(&mut root, id, key);
    let long = "x".repeat(emilybase_files::MAX_FILE_NAME_BYTES + 1);
    let payload = vec![0; emilybase_object_storage::MAX_PAYLOAD_BYTES + 1];
    let before = copied_history(&s, &path);
    for name in ["", "bad\0name", "bad\nname", &long] {
        assert!(
            root.public_publish_file(
                id,
                session.access.expose(),
                101,
                AccountFileUpload {
                    name,
                    ..upload(BYTES)
                }
            )
            .is_err()
        );
    }
    assert!(matches!(
        root.public_publish_file(id, session.access.expose(), 101, upload(&payload)),
        Err(Error::Files(emilybase_files::Error::Objects(
            emilybase_object_storage::Error::Limit
        )))
    ));
    assert_eq!(copied_history(&s, &path), before);
    let name = "ф".repeat(emilybase_files::MAX_FILE_NAME_BYTES / 2);
    let receipt = root
        .public_publish_file(
            id,
            session.access.expose(),
            101,
            AccountFileUpload {
                name: &name,
                ..upload(b"")
            },
        )
        .unwrap();
    assert_eq!(receipt.revision(), 4);
    let info = root.project_file_info(id, key, FRESH).unwrap().unwrap();
    assert_eq!(info.name(), name);
    assert_eq!(info.report().payload_bytes, 0);
    assert!(
        root.read_project_file(id, key, FRESH, 0)
            .unwrap()
            .payload()
            .is_empty()
    );
    // Display names never choose paths; typed object IDs choose canonical names.
    let display = "../../../escape";
    let second = FileId::from_bytes([93; 16]);
    root.public_publish_file(
        id,
        session.access.expose(),
        101,
        AccountFileUpload {
            id: second,
            object: ObjectId::from_bytes([94; 16]),
            name: display,
            ..upload(b"data")
        },
    )
    .unwrap();
    assert_eq!(
        root.project_file_info(id, key, second)
            .unwrap()
            .unwrap()
            .name(),
        display
    );
    assert!(!path.join("escape").exists());
    assert!(!path.join("files").join(id).join("escape").exists());
    drop(root);
    verify_source(&s);
}

#[test]
fn closed_admission_and_current_revocation_refuse_before_any_object_publication() {
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
                    root.change_password(id, key, LOGIN, PASSWORD, b"synthetic-changed-password")
                        .unwrap();
                }
                _ => now = session.metadata.access_until,
            }
            let native = fs::read(path.join("files").join(id).join("metadata/redo.wal")).unwrap();
            let _hook = durability::on_boundary(POINTS[0], || {
                panic!("revoked session entered native publication")
            });
            assert!(
                root.public_publish_file(id, session.access.expose(), now, upload(BYTES))
                    .is_err()
            );
            drop(_hook);
            drop(root);
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            assert!(
                root.public_publish_file(id, session.access.expose(), now, upload(BYTES))
                    .is_err()
            );
            assert_state(&mut root, id, key, false, false);
            assert_eq!(
                fs::read(path.join("files").join(id).join("metadata/redo.wal")).unwrap(),
                native
            );
            drop(root);
            verify_source(&s);
        }
    }
}

#[test]
fn trusted_forward_clock_commits_once_even_when_create_is_denied_without_native_write() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, compact);
        let (id, key) = &s.root.credentials[0];
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let session = activate(&mut root, id, key);
        policy(&mut root, id, key, false, Rule::Deny {});
        let private = path.join("private").join(id).join("redo.wal");
        let native = path.join("files").join(id).join("metadata/redo.wal");
        let clock = last(&private);
        let bytes = fs::read(&native).unwrap();
        for _ in 0..2 {
            assert!(matches!(
                root.public_publish_file(id, session.access.expose(), 102, upload(BYTES)),
                Err(Error::Denied)
            ));
            assert_eq!(last(&private), clock + 1);
            assert_eq!(fs::read(&native).unwrap(), bytes);
        }
        policy(&mut root, id, key, true, Rule::Deny {});
        let clock = last(&private);
        let result = root
            .public_publish_file(id, session.access.expose(), 103, upload(BYTES))
            .unwrap();
        assert_eq!(last(&private), clock + 1);
        assert_eq!(result.revision(), 4);
        drop(root);
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let before = copied_history(&s, &path);
        assert!(
            root.public_publish_file(id, session.access.expose(), 102, upload(b"retry"))
                .is_err()
        );
        assert_eq!(copied_history(&s, &path), before);
        assert_state(&mut root, id, key, true, true);
        drop(root);
        verify_source(&s);
    }
}

#[test]
fn real_source_substitutions_before_blob_after_blob_and_final_result_withhold_receipt() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for point in [POINTS[1], POINTS[3], POINTS[7]] {
            for target in 0..4 {
                let t = tempfile::tempdir().unwrap();
                let (s, path) = copy(t.path(), 1, compact);
                let (id, key) = &s.root.credentials[0];
                let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                let session = activate(&mut root, id, key);
                let before = copied_history(&s, &path);
                let selected = match target {
                    0 => path.join("root.json"),
                    1 => path.join("registry").join(id).join("project.json"),
                    2 => path.join("private").join(id).join("redo.wal"),
                    _ => path.join("files").join(id).join("metadata/redo.wal"),
                };
                let saved = t.path().join("original");
                let changed = selected.clone();
                let retained = saved.clone();
                let _hook = durability::on_boundary(point, move || {
                    fs::rename(&changed, &retained).unwrap();
                    fs::copy(&retained, &changed).unwrap();
                });
                let result =
                    root.public_publish_file(id, session.access.expose(), 101, upload(BYTES));
                drop(_hook);
                assert!(result.is_err());
                assert_eq!(
                    matches!(result, Err(Error::AccountFileWriteUnknown(_))),
                    has_blob(point)
                );
                fs::remove_file(&selected).unwrap();
                fs::rename(&saved, &selected).unwrap();
                if has_blob(point) {
                    assert!(matches!(
                        root.projects(),
                        Err(Error::AccountFileWritePoisoned)
                    ));
                }
                drop(root);
                if !has_blob(point) {
                    assert_eq!(copied_history(&s, &path), before);
                }
                let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                assert_state(&mut root, id, key, has_blob(point), committed(point));
                root.execute(id, key, "UPDATE t SET v=201 WHERE id=1", &[])
                    .unwrap();
                drop(root);
                verify_source(&s);
            }
        }
    }
}

#[test]
fn whole_frame_unwind_closes_root_after_admitted_blob_attempt_and_keeps_possible_data() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for point in POINTS {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), 1, compact);
            let (id, key) = &s.root.credentials[0];
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            let session = activate(&mut root, id, key);
            let before = copied_history(&s, &path);
            let _hook =
                durability::on_boundary(point, || panic!("synthetic trusted upload unwind"));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                root.public_publish_file(id, session.access.expose(), 101, upload(BYTES))
            }));
            drop(_hook);
            assert!(result.is_err());
            if has_blob(point) || point == POINTS[2] {
                assert!(matches!(
                    root.projects(),
                    Err(Error::AccountFileWritePoisoned)
                ));
            } else {
                assert!(!matches!(
                    root.projects(),
                    Err(Error::AccountFileWritePoisoned)
                ));
            }
            drop(root);
            if !has_blob(point) {
                assert_eq!(copied_history(&s, &path), before);
            }
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            assert_state(&mut root, id, key, has_blob(point), committed(point));
            root.execute(id, key, "UPDATE t SET v=202 WHERE id=1", &[])
                .unwrap();
            drop(root);
            verify_source(&s);
        }
    }
}

#[test]
fn actual_signal9_recovery_preserves_confirmed_uploads_and_charges_unreferenced_blobs() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for point in [
            POINTS[1],
            POINTS[2],
            POINTS[3],
            POINTS[4],
            POINTS[5],
            POINTS[7],
            "account_user_file_upload_returned",
        ] {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), 1, compact);
            let (id, key) = &s.root.credentials[0];
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            let session = activate(&mut root, id, key);
            let owner = identity(&mut root, id, key, &session);
            drop(root);
            let token = t.path().join("access");
            fs::write(&token, session.access.expose()).unwrap();
            fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
            let before = copied_history(&s, &path);
            let worker = Worker::start(&path, &token, id, "root-v2-user-file-upload", point);
            worker.reach(point);
            assert!(AccountStore::open(path.join("private").join(id), id, pool()).is_err());
            worker.kill();
            if !has_blob(point) {
                assert_eq!(copied_history(&s, &path), before);
            }
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            assert_state(&mut root, id, key, has_blob(point), committed(point));
            if committed(point) {
                assert_eq!(
                    root.project_file_info(id, key, FRESH)
                        .unwrap()
                        .unwrap()
                        .owner(),
                    &owner
                );
            }
            if has_blob(point) {
                let before = copied_history(&s, &path);
                assert!(
                    root.public_publish_file(id, session.access.expose(), 101, upload(BYTES))
                        .is_err()
                );
                assert_eq!(copied_history(&s, &path), before);
            }
            root.execute(id, key, "UPDATE t SET v=203 WHERE id=1", &[])
                .unwrap();
            let next = root
                .public_publish_file(
                    id,
                    session.access.expose(),
                    101,
                    AccountFileUpload {
                        id: FileId::from_bytes([95; 16]),
                        object: ObjectId::from_bytes([96; 16]),
                        ..upload(b"independent")
                    },
                )
                .unwrap();
            assert_eq!(next.revision(), if committed(point) { 5 } else { 4 });
            drop(root);
            verify_source(&s);
        }
    }
}

#[test]
fn quota_refusal_followed_by_final_guard_failure_does_not_invent_a_published_blob() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, false);
    let (id, key) = &s.root.credentials[0];
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    let session = activate(&mut root, id, key);
    let usage = root.inspect_project_files(id, key).unwrap().usage();
    let state = root.project_file_quota(id, key).unwrap();
    root.set_project_file_quota(
        id,
        key,
        state,
        FileQuota::new(2, usage.payload_bytes).unwrap(),
    )
    .unwrap();
    let before = copied_history(&s, &path);
    let selected = path.join("root.json");
    let changed = selected.clone();
    let saved = t.path().join("original");
    let retained = saved.clone();
    let _hook = durability::on_boundary("account_user_file_result", move || {
        fs::rename(&changed, &retained).unwrap();
        fs::copy(&retained, &changed).unwrap();
    });
    let result = root.public_publish_file(id, session.access.expose(), 101, upload(BYTES));
    drop(_hook);
    assert!(result.is_err());
    assert!(!matches!(result, Err(Error::AccountFileWriteUnknown(_))));
    fs::remove_file(&selected).unwrap();
    fs::rename(&saved, &selected).unwrap();
    assert!(root.projects().is_ok());
    assert_eq!(copied_history(&s, &path), before);
    let state = root.project_file_quota(id, key).unwrap();
    root.set_project_file_quota(id, key, state, FileQuota::new(3, 65536).unwrap())
        .unwrap();
    root.public_publish_file(id, session.access.expose(), 101, upload(BYTES))
        .unwrap();
    drop(root);
    verify_source(&s);
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
fn original_common_private_native_and_selected_blob_descriptors_span_publication_checks() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for point in [POINTS[3], POINTS[4], POINTS[5], POINTS[7]] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, true);
        let (id, key) = &s.root.credentials[0];
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let session = activate(&mut root, id, key);
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
            if point != "account_user_file_result" {
                assert!(
                    fds(&selected
                        .join("files")
                        .join(&project)
                        .join("objects")
                        .join(format!("{OBJECT}.object")))
                        > 0
                );
            }
            assert!(
                AccountStore::open(selected.join("private").join(&project), &project, pool())
                    .is_err()
            );
            assert!(Database::open(selected.join("registry").join(&project).join("data")).is_ok());
        });
        root.public_publish_file(id, session.access.expose(), 101, upload(BYTES))
            .unwrap();
        drop(_hook);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        drop(root);
        verify_source(&s);
    }
}

#[test]
fn independent_restore_preserves_uploaded_owner_and_policy_but_revokes_copied_sessions() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, compact);
        let (id, key) = &s.root.credentials[0];
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let session = activate(&mut root, id, key);
        let owner = identity(&mut root, id, key, &session);
        root.public_publish_file(id, session.access.expose(), 101, upload(BYTES))
            .unwrap();
        let before = copied_history(&s, &path);
        let bytes = encode(&root.capture_v2().unwrap());
        crate::restore_account_file_archive_bytes_at(
            &bytes,
            &File::open(t.path()).unwrap(),
            "user-upload-copy",
            pool(),
            50,
        )
        .unwrap();
        let mut restored = AccountRoot::open_v2(t.path().join("user-upload-copy"), pool()).unwrap();
        assert_state(&mut restored, id, key, true, true);
        assert_eq!(
            restored
                .project_file_info(id, key, FRESH)
                .unwrap()
                .unwrap()
                .owner(),
            &owner
        );
        assert_eq!(
            restored.project_file_policy(id, key).unwrap(),
            root.project_file_policy(id, key).unwrap()
        );
        let admission = restored.public_admission(id, key).unwrap();
        assert!(!admission.enabled);
        assert!(
            restored
                .public_publish_file(id, session.access.expose(), 101, upload(b"old-session"))
                .is_err()
        );
        restored
            .set_public_admission(id, key, admission.revision, true)
            .unwrap();
        // Copied sessions remain revoked even after explicit reopening.
        assert!(
            restored
                .public_publish_file(id, session.access.expose(), 50, upload(b"old-session"))
                .is_err()
        );
        let fresh = restored.public_sign_in(id, LOGIN, PASSWORD, 50).unwrap();
        let next = FileId::from_bytes([95; 16]);
        restored
            .public_publish_file(
                id,
                fresh.access.expose(),
                50,
                AccountFileUpload {
                    id: next,
                    object: ObjectId::from_bytes([96; 16]),
                    ..upload(b"restored-only")
                },
            )
            .unwrap();
        assert_eq!(
            restored
                .project_file_info(id, key, next)
                .unwrap()
                .unwrap()
                .owner(),
            &owner
        );
        assert!(root.project_file_info(id, key, next).unwrap().is_none());
        assert_eq!(copied_history(&s, &path), before);
        drop(restored);
        drop(root);
        verify_source(&s);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]
    #[test]
    fn generated_create_rule_collision_and_capacity_decisions_match_independent_model(
        compact in any::<bool>(), permit in any::<bool>(), collision in 0u8..3,
        full in any::<bool>(), payload in proptest::collection::vec(any::<u8>(), 0..32)
    ) {
        let _serial = durability::PROCESS_TESTS.blocking_lock();
        let t = tempfile::tempdir().unwrap();
        let (s,path) = copy(t.path(), 1, compact);
        let (id,key) = &s.root.credentials[0];
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let session = activate(&mut root, id, key);
        let owner = identity(&mut root, id, key, &session);
        policy(&mut root, id, key, permit, Rule::Deny {});
        let usage = root.inspect_project_files(id,key).unwrap().usage();
        let quota = root.project_file_quota(id,key).unwrap();
        root.set_project_file_quota(id,key,quota,FileQuota::new(if full {2} else {3},65536).unwrap()).unwrap();
        let before = copied_history(&s,&path);
        let original = root.project_file_info(id,key,FILE).unwrap().unwrap();
        let request = AccountFileUpload {
            id: if collision == 1 {FILE} else {FRESH},
            object: if collision == 2 {original.object()} else {OBJECT},
            ..upload(&payload)
        };
        let result = root.public_publish_file(id, session.access.expose(), 101, request);
        if !permit { prop_assert!(matches!(result, Err(Error::Denied))); }
        else if collision == 1 { prop_assert!(matches!(result,Err(Error::Files(emilybase_files::Error::Exists)))); }
        else if collision == 2 { prop_assert!(matches!(result,Err(Error::Files(emilybase_files::Error::Objects(emilybase_object_storage::Error::Exists))))); }
        else if full { prop_assert!(matches!(result,Err(Error::Files(emilybase_files::Error::Objects(emilybase_object_storage::Error::Limit))))); }
        else {
            prop_assert!(result.is_ok());
            let info = root.project_file_info(id,key,FRESH).unwrap().unwrap();
            prop_assert_eq!(info.owner(), &owner);
            let read = root.read_project_file(id,key,FRESH,32).unwrap();
            prop_assert_eq!(read.payload(), payload.as_slice());
            prop_assert_eq!(root.inspect_project_files(id,key).unwrap().usage().physical_objects, usage.physical_objects+1);
        }
        if !permit || collision != 0 || full { prop_assert_eq!(copied_history(&s,&path),before); }
        prop_assert!(root.projects().is_ok());
        drop(root);
        verify_source(&s);
    }
}
