//! Genuine user-only installed file reads, not copied service observations.
use super::*;
use emilybase_auth::{
    accounts::IssuedSession,
    file_policy::{CreateRule, Definition, Rule},
};
const ALICE_FILE: FileId = FileId::from_bytes([81; 16]);
const BOB_FILE: FileId = FileId::from_bytes([83; 16]);
const MISSING: FileId = FileId::from_bytes([87; 16]);
const BOB: &str = "synthetic_bob";
const POINTS: [&str; 6] = [
    "account_user_file_authenticated",
    "account_user_file_principal",
    "account_file_read_retained",
    "account_file_read_copied",
    "account_file_read_verified",
    "account_user_file_result",
];

fn policy(root: &mut AccountRoot, id: &str, key: &str, rule: Rule) {
    let expected = root
        .project_file_policy(id, key)
        .unwrap()
        .map_or(0, |p| p.revision);
    let bytes = serde_json::to_vec(&Definition {
        version: 1,
        read: rule,
        create: CreateRule::Deny {},
        rename: Rule::Deny {},
        remove: Rule::Deny {},
    })
    .unwrap();
    root.install_project_file_policy(id, key, expected, &bytes)
        .unwrap();
}
fn activate(root: &mut AccountRoot, id: &str, key: &str, rule: Rule) -> IssuedSession {
    root.enable_file_policy_catalog(id, key).unwrap();
    policy(root, id, key, rule);
    let admission = root.public_admission(id, key).unwrap();
    root.set_public_admission(id, key, admission.revision, true)
        .unwrap();
    root.sign_in(id, key, LOGIN, PASSWORD, 101).unwrap()
}
fn identity(root: &mut AccountRoot, id: &str, key: &str, session: &IssuedSession) -> [u8; 16] {
    root.with_access(id, key, session.access.expose(), 101, |p| p.account().id)
        .unwrap()
}
fn publish(
    root: &mut AccountRoot,
    id: &str,
    key: &str,
    file: FileId,
    object: u8,
    owner: [u8; 16],
    payload: &[u8],
) {
    root.publish_project_file(
        id,
        key,
        crate::ProjectFileUpload {
            id: file,
            object: ObjectId::from_bytes([object; 16]),
            owner,
            name: "synthetic-user-file",
            payload,
        },
    )
    .unwrap();
}
fn setup(
    root: &mut AccountRoot,
    id: &str,
    key: &str,
) -> (IssuedSession, IssuedSession, [u8; 16], [u8; 16]) {
    let alice = activate(root, id, key, Rule::Owner {});
    root.create_user(id, key, BOB, PASSWORD).unwrap();
    let bob = root.sign_in(id, key, BOB, PASSWORD, 101).unwrap();
    let a = identity(root, id, key, &alice);
    let b = identity(root, id, key, &bob);
    assert_ne!(a, b);
    publish(root, id, key, ALICE_FILE, 82, a, b"alice-payload");
    publish(root, id, key, BOB_FILE, 84, b, b"bob-payload");
    (alice, bob, a, b)
}
fn denies(root: &mut AccountRoot, id: &str, token: &str, now: u64) {
    assert!(root.public_list_files(id, token, now).is_err());
    assert!(root.public_file_info(id, token, now, MISSING).is_err());
    assert!(
        root.public_read_file(id, token, now, ALICE_FILE, 32)
            .is_err()
    );
}

#[test]
fn installed_owner_authenticated_and_deny_rules_use_actual_users_native_owners_and_project_scope() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 2, compact);
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let (id, key) = &s.root.credentials[0];
        let (other, other_key) = &s.root.credentials[1];
        let (alice, bob, _, _) = setup(&mut root, id, key);
        let foreign = activate(&mut root, other, other_key, Rule::Authenticated {});
        let before = copied_history(&s, &path);
        for (session, file, bytes) in [
            (&alice, ALICE_FILE, b"alice-payload".as_slice()),
            (&bob, BOB_FILE, b"bob-payload".as_slice()),
        ] {
            let token = session.access.expose();
            let list = root.public_list_files(id, token, 101).unwrap();
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].id(), file);
            assert_eq!(
                root.public_file_info(id, token, 101, file).unwrap(),
                Some(list[0].clone())
            );
            assert_eq!(
                root.public_read_file(id, token, 101, file, 32)
                    .unwrap()
                    .payload(),
                bytes
            );
        }
        assert!(
            root.public_file_info(id, alice.access.expose(), 101, BOB_FILE)
                .unwrap()
                .is_none()
        );
        assert!(
            root.public_file_info(id, alice.access.expose(), 101, MISSING)
                .unwrap()
                .is_none()
        );
        for file in [BOB_FILE, MISSING, FILE] {
            assert!(matches!(
                root.public_read_file(id, alice.access.expose(), 101, file, 32),
                Err(Error::Denied)
            ));
        }
        denies(&mut root, id, foreign.access.expose(), 101);
        denies(&mut root, other, alice.access.expose(), 101);
        denies(&mut root, id, key, 101);
        denies(&mut root, id, alice.refresh.expose(), 101);
        assert_eq!(copied_history(&s, &path), before);
        policy(&mut root, id, key, Rule::Authenticated {});
        let before = copied_history(&s, &path);
        assert_eq!(
            root.public_list_files(id, alice.access.expose(), 101)
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            root.public_read_file(id, alice.access.expose(), 101, BOB_FILE, 32)
                .unwrap()
                .payload(),
            b"bob-payload"
        );
        assert_eq!(copied_history(&s, &path), before);
        policy(&mut root, id, key, Rule::Deny {});
        let before = copied_history(&s, &path);
        assert!(
            root.public_list_files(id, alice.access.expose(), 101)
                .unwrap()
                .is_empty()
        );
        assert!(
            root.public_file_info(id, alice.access.expose(), 101, ALICE_FILE)
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            root.public_read_file(id, alice.access.expose(), 101, ALICE_FILE, 32),
            Err(Error::Denied)
        ));
        assert_eq!(copied_history(&s, &path), before);
        // Service-key rotation is independent of current user-session authority.
        let rotated = root.rotate_project_key(id).unwrap();
        policy(&mut root, id, &rotated.api_key, Rule::Authenticated {});
        assert_eq!(
            root.public_read_file(id, alice.access.expose(), 101, BOB_FILE, 32)
                .unwrap()
                .payload(),
            b"bob-payload"
        );
        denies(&mut root, id, &rotated.api_key, 101);
        drop(root);
        verify_source(&s);
    }
}

#[test]
fn current_admission_revocation_refresh_disable_password_and_expiry_never_reuse_a_copied_grant() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for kind in 0..6 {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), 1, compact);
            let (id, key) = &s.root.credentials[0];
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            let session = activate(&mut root, id, key, Rule::Authenticated {});
            root.public_read_file(id, session.access.expose(), 101, FILE, 32)
                .unwrap();
            let now = if kind == 5 {
                session.metadata.access_until
            } else {
                101
            };
            match kind {
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
                    root.change_password(
                        id,
                        key,
                        LOGIN,
                        PASSWORD,
                        b"synthetic-replacement-password",
                    )
                    .unwrap();
                }
                _ => (),
            }
            denies(&mut root, id, session.access.expose(), now);
            let _hook = durability::on_boundary("account_file_key_admitted", || {
                panic!("denied session reached file binding")
            });
            denies(&mut root, id, session.access.expose(), now);
            drop(_hook);
            drop(root);
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            denies(&mut root, id, session.access.expose(), now);
            drop(root);
            verify_source(&s);
        }
    }
}

#[test]
fn absent_catalog_policy_closed_admission_and_invalid_tokens_refuse_before_empty_or_missing_lookup()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, false);
    let (id, key) = &s.root.credentials[0];
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    let session = root.sign_in(id, key, LOGIN, PASSWORD, 101).unwrap();
    let _hook = durability::on_boundary("account_file_key_admitted", || {
        panic!("closed admission reached binding")
    });
    denies(&mut root, id, session.access.expose(), 101);
    drop(_hook);
    let a = root.public_admission(id, key).unwrap();
    root.set_public_admission(id, key, a.revision, true)
        .unwrap();
    assert!(
        root.public_list_files(id, session.access.expose(), 101)
            .is_err()
    );
    assert!(
        root.public_file_info(id, session.access.expose(), 101, MISSING)
            .is_err()
    );
    root.enable_file_policy_catalog(id, key).unwrap();
    assert!(
        root.public_list_files(id, session.access.expose(), 101)
            .is_err()
    );
    assert!(
        root.public_file_info(id, session.access.expose(), 101, MISSING)
            .is_err()
    );
    policy(&mut root, id, key, Rule::Owner {});
    assert!(
        root.public_list_files(id, session.access.expose(), 101)
            .unwrap()
            .is_empty()
    );
    let _hook = durability::on_boundary("account_file_key_admitted", || {
        panic!("invalid token reached binding")
    });
    for access in ["malformed", key.as_str(), session.refresh.expose()] {
        denies(&mut root, id, access, 101);
    }
    drop(_hook);
    drop(root);
    verify_source(&s);
}

#[test]
fn actual_reader_authority_limits_missing_orphans_and_empty_payloads_preserve_exact_histories() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, true);
    let (id, key) = &s.root.credentials[0];
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    let session = activate(&mut root, id, key, Rule::Owner {});
    let user = identity(&mut root, id, key, &session);
    publish(&mut root, id, key, ALICE_FILE, 82, user, b"alice-payload");
    publish(&mut root, id, key, BOB_FILE, 84, user, b"");
    let before = copied_history(&s, &path);
    let token = session.access.expose();
    assert!(matches!(
        root.public_read_file(id, token, 101, ALICE_FILE, 3),
        Err(Error::Limit)
    ));
    assert!(matches!(
        root.public_read_file(id, token, 101, ALICE_FILE, usize::MAX),
        Err(Error::Limit)
    ));
    assert!(
        root.public_read_file(id, token, 101, BOB_FILE, 0)
            .unwrap()
            .payload()
            .is_empty()
    );
    assert!(matches!(
        root.public_read_file(id, token, 101, MISSING, 32),
        Err(Error::Denied)
    ));
    assert!(
        root.public_file_info(id, token, 101, MISSING)
            .unwrap()
            .is_none()
    );
    let orphan_id = FileId::from_bytes([5; 16]);
    assert!(
        root.public_file_info(id, token, 101, orphan_id)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        root.public_read_file(id, token, 101, orphan_id, 32),
        Err(Error::Denied)
    ));
    assert_eq!(root.public_list_files(id, token, 101).unwrap().len(), 2);
    assert_eq!(copied_history(&s, &path), before);
    assert_eq!(root.projects().unwrap().len(), 1);
    drop(root);
    verify_source(&s);
}

#[test]
fn original_private_native_project_metadata_and_payload_descriptors_span_user_read_checks() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for point in POINTS {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, true);
        let (id, key) = &s.root.credentials[0];
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let session = activate(&mut root, id, key, Rule::Authenticated {});
        let p = path.clone();
        let project = id.clone();
        let _hook = durability::on_boundary(point, move || {
            assert!(File::open(&p).unwrap().try_lock().is_err());
            assert!(ProjectStore::open_existing(p.join("registry")).is_err());
            assert!(
                AccountStore::open(p.join("private").join(&project), &project, pool()).is_err()
            );
            assert!(
                NativeFileRoot::open(p.join("files").join(&project), project.parse().unwrap())
                    .is_err()
            );
            assert!(Database::open(p.join("registry").join(&project).join("data")).is_ok());
            assert!(
                fds(
                    std::process::id(),
                    &p.join("private").join(&project).join("redo.wal")
                ) >= 2
            );
            if point != "account_user_file_authenticated" {
                assert!(
                    fds(
                        std::process::id(),
                        &p.join("registry").join(&project).join("project.json")
                    ) >= 1
                );
            }
            if matches!(
                point,
                "account_file_read_retained"
                    | "account_file_read_copied"
                    | "account_file_read_verified"
            ) {
                assert!(
                    fds(
                        std::process::id(),
                        &object(&p.join("files").join(&project), false)
                    ) >= 1
                );
            }
        });
        assert_eq!(
            root.public_read_file(id, session.access.expose(), 101, FILE, 32)
                .unwrap()
                .payload(),
            b"synthetic-payload"
        );
        drop(root);
        verify_source(&s);
    }
}

#[test]
fn late_original_selection_replacements_withhold_all_user_payload_output_without_adopting_copies() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for point in POINTS {
            for target in 0..4 {
                let t = tempfile::tempdir().unwrap();
                let (s, path) = copy(t.path(), 1, compact);
                let (id, key) = &s.root.credentials[0];
                let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                let session = activate(&mut root, id, key, Rule::Authenticated {});
                let selected = match target {
                    0 => path.join("root.json"),
                    1 => path.join("registry").join(id).join("project.json"),
                    2 => path.join("files").join(id).join("metadata/redo.wal"),
                    _ => path.join("private").join(id).join("redo.wal"),
                };
                let changed = selected.clone();
                let saved = t.path().join("original");
                let retained = saved.clone();
                let before = copied_history(&s, &path);
                let _hook = durability::on_boundary(point, move || {
                    fs::rename(&changed, &retained).unwrap();
                    fs::copy(retained, changed).unwrap();
                });
                assert!(
                    root.public_read_file(id, session.access.expose(), 101, FILE, 32)
                        .is_err(),
                    "{compact} {point} {target}"
                );
                drop(_hook);
                fs::rename(saved, selected).unwrap();
                drop(root);
                assert_eq!(copied_history(&s, &path), before);
                let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                assert_eq!(
                    root.public_read_file(id, session.access.expose(), 101, FILE, 32)
                        .unwrap()
                        .payload(),
                    b"synthetic-payload"
                );
                drop(root);
                verify_source(&s);
            }
        }
    }
}

#[test]
fn actual_user_reader_signal9_kills_release_original_owners_keep_histories_and_allow_a_new_write() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for point in POINTS.into_iter().chain(["account_user_file_returned"]) {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), 1, compact);
            let (id, key) = &s.root.credentials[0];
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            let session = activate(&mut root, id, key, Rule::Authenticated {});
            drop(root);
            let token = t.path().join("synthetic-access");
            fs::write(&token, session.access.expose()).unwrap();
            fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
            let before = copied_history(&s, &path);
            let worker = Worker::start(&path, &token, id, "root-v2-user-file-read", point);
            worker.reach(point);
            assert!(AccountStore::open(path.join("private").join(id), id, pool()).is_err());
            worker.kill();
            assert_eq!(copied_history(&s, &path), before);
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            assert_eq!(
                root.public_read_file(id, session.access.expose(), 101, FILE, 32)
                    .unwrap()
                    .payload(),
                b"synthetic-payload"
            );
            root.execute(id, key, "UPDATE t SET v=97 WHERE id=1", &[])
                .unwrap();
            drop(root);
            verify_source(&s);
        }
    }
}

#[test]
fn retained_parent_and_independent_restore_keep_native_policy_binding_without_copy_session_authority()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, true);
    let (id, key) = &s.root.credentials[0];
    let parent = path.parent().unwrap().to_path_buf();
    let mut root = AccountRoot::open_v2_at(&File::open(&parent).unwrap(), "v2", pool()).unwrap();
    let session = activate(&mut root, id, key, Rule::Owner {});
    let user = identity(&mut root, id, key, &session);
    publish(&mut root, id, key, ALICE_FILE, 82, user, b"alice-payload");
    let moved = t.path().join("moved-user-reader");
    fs::rename(&parent, &moved).unwrap();
    fs::DirBuilder::new().mode(0o700).create(&parent).unwrap();
    fs::write(parent.join("keep"), b"foreign old parent").unwrap();
    let before = copied_history(&s, &moved.join("v2"));
    assert_eq!(
        root.public_read_file(id, session.access.expose(), 101, ALICE_FILE, 32)
            .unwrap()
            .payload(),
        b"alice-payload"
    );
    let bytes = encode(&root.capture_v2().unwrap());
    crate::restore_account_file_archive_bytes_at(
        &bytes,
        &File::open(t.path()).unwrap(),
        "user-file-copy",
        pool(),
        50,
    )
    .unwrap();
    let mut copy = AccountRoot::open_v2(t.path().join("user-file-copy"), pool()).unwrap();
    denies(&mut copy, id, session.access.expose(), 50);
    let a = copy.public_admission(id, key).unwrap();
    assert!(!a.enabled);
    copy.set_public_admission(id, key, a.revision, true)
        .unwrap();
    let fresh = copy.public_sign_in(id, LOGIN, PASSWORD, 50).unwrap();
    assert_eq!(
        copy.public_read_file(id, fresh.access.expose(), 50, ALICE_FILE, 32)
            .unwrap()
            .payload(),
        b"alice-payload"
    );
    assert_eq!(copied_history(&s, &moved.join("v2")), before);
    assert_eq!(
        fs::read(parent.join("keep")).unwrap(),
        b"foreign old parent"
    );
    drop(copy);
    drop(root);
    verify_source(&s);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]
    #[test]
    fn generated_current_policy_and_user_visibility_match_independent_native_owner_inventory(rule in 0u8..3,bob in any::<bool>(),compact in any::<bool>()) {
        let _serial=durability::PROCESS_TESTS.blocking_lock();let t=tempfile::tempdir().unwrap();let(s,path)=copy(t.path(),1,compact);let(id,key)=&s.root.credentials[0];let mut root=AccountRoot::open_v2(&path,pool()).unwrap();let(a,b,alice_id,bob_id)=setup(&mut root,id,key);
        let selected=if bob{bob_id}else{alice_id};let token=if bob{b.access.expose()}else{a.access.expose()};
        policy(&mut root,id,key,match rule{0=>Rule::Deny{},1=>Rule::Owner{},_=>Rule::Authenticated{}});
        let before=copied_history(&s,&path);let inventory=root.list_project_files(id,key).unwrap();let expected=inventory.into_iter().filter(|f|rule==2||(rule==1&&*f.owner()==selected)).collect::<Vec<_>>();
        prop_assert_eq!(root.public_list_files(id,token,101).unwrap(),expected);prop_assert_eq!(copied_history(&s,&path),before);drop(root);verify_source(&s);
    }
}

#[test]
fn original_private_selection_allows_exact_forward_clock_commit_and_denied_attempt_never_reverses_time()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, compact);
        let (id, key) = &s.root.credentials[0];
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let session = activate(&mut root, id, key, Rule::Authenticated {});
        let file = path.join("private").join(id).join("redo.wal");
        let last = |bytes: &[u8]| {
            emilybase_transactions::recover_image(bytes, None)
                .unwrap()
                .last_transaction
        };
        let before = fs::read(&file).unwrap();
        let receipt = root.project_file_policy(id, key).unwrap();
        assert_eq!(
            root.public_read_file(id, session.access.expose(), 102, FILE, 32)
                .unwrap()
                .payload(),
            b"synthetic-payload"
        );
        let after = fs::read(&file).unwrap();
        assert_eq!(last(&after), last(&before) + 1);
        assert_eq!(root.project_file_policy(id, key).unwrap(), receipt);
        assert!(root.public_list_files(id, "malformed", 103).is_err());
        let denied = fs::read(&file).unwrap();
        assert_eq!(last(&denied), last(&after) + 1);
        assert!(
            root.public_list_files(id, session.access.expose(), 102)
                .is_err()
        );
        assert_eq!(fs::read(&file).unwrap(), denied);
        let a = root.public_admission(id, key).unwrap();
        root.set_public_admission(id, key, a.revision, false)
            .unwrap();
        let closed = fs::read(&file).unwrap();
        denies(&mut root, id, session.access.expose(), 999);
        assert_eq!(fs::read(&file).unwrap(), closed);
        let a = root.public_admission(id, key).unwrap();
        root.set_public_admission(id, key, a.revision, true)
            .unwrap();
        assert_eq!(
            root.public_read_file(id, session.access.expose(), 104, FILE, 32)
                .unwrap()
                .payload(),
            b"synthetic-payload"
        );
        drop(root);
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        assert!(
            root.public_list_files(id, session.access.expose(), 103)
                .is_err()
        );
        assert_eq!(
            root.public_read_file(id, session.access.expose(), 104, FILE, 32)
                .unwrap()
                .payload(),
            b"synthetic-payload"
        );
        drop(root);
        verify_source(&s);
    }
}

#[test]
fn same_byte_payload_replacement_while_actual_reader_is_held_never_releases_a_partial_user_copy() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for point in [
            "account_file_read_retained",
            "account_file_read_copied",
            "account_file_read_verified",
        ] {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), 1, compact);
            let (id, key) = &s.root.credentials[0];
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            let session = activate(&mut root, id, key, Rule::Authenticated {});
            let selected = object(&path.join("files").join(id), false);
            let changed = selected.clone();
            let saved = t.path().join("original-blob");
            let retained = saved.clone();
            let before = copied_history(&s, &path);
            let _hook = durability::on_boundary(point, move || {
                fs::rename(&changed, &retained).unwrap();
                fs::copy(retained, changed).unwrap();
            });
            assert!(
                root.public_read_file(id, session.access.expose(), 101, FILE, 32)
                    .is_err()
            );
            drop(_hook);
            fs::rename(saved, selected).unwrap();
            drop(root);
            assert_eq!(copied_history(&s, &path), before);
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            assert_eq!(
                root.public_read_file(id, session.access.expose(), 101, FILE, 32)
                    .unwrap()
                    .payload(),
                b"synthetic-payload"
            );
            drop(root);
            verify_source(&s);
        }
    }
}

#[test]
fn unsafe_metadata_replacements_refuse_in_bounded_actual_user_workers_and_preserve_foreign_entries()
{
    use std::os::unix::fs::FileTypeExt;
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for target in 0..3 {
        for kind in 0..4 {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), 1, target % 2 == 0);
            let (id, key) = &s.root.credentials[0];
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            let session = activate(&mut root, id, key, Rule::Authenticated {});
            drop(root);
            let before = copied_history(&s, &path);
            let token = t.path().join("synthetic-access");
            fs::write(&token, session.access.expose()).unwrap();
            fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
            let selected = match target {
                0 => path.join("registry").join(id).join("project.json"),
                1 => path.join("private").join(id).join("redo.wal"),
                _ => path.join("files").join(id).join("metadata/redo.wal"),
            };
            let saved = t.path().join("original");
            let bytes = fs::read(&selected).unwrap();
            let mut worker = Worker::start(
                &path,
                &token,
                id,
                "root-v2-user-file-read-refuse-race",
                "account_user_file_authenticated",
            );
            worker.reach("account_user_file_authenticated");
            fs::rename(&selected, &saved).unwrap();
            match kind {
                0 => rustix::fs::mkfifoat(
                    rustix::fs::CWD,
                    &selected,
                    rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
                )
                .unwrap(),
                1 => std::os::unix::fs::symlink(&saved, &selected).unwrap(),
                2 => fs::hard_link(&saved, &selected).unwrap(),
                _ => {
                    fs::write(&selected, &bytes).unwrap();
                    fs::set_permissions(&selected, fs::Permissions::from_mode(0o644)).unwrap();
                }
            }
            worker.release();
            let out = worker.finish();
            assert!(!out.contains(session.access.expose()));
            assert_eq!(fs::read(&saved).unwrap(), bytes);
            let md = fs::symlink_metadata(&selected).unwrap();
            match kind {
                0 => assert!(md.file_type().is_fifo()),
                1 => assert!(md.file_type().is_symlink()),
                2 => assert_eq!(md.nlink(), 2),
                _ => assert_eq!(md.mode() & 0o777, 0o644),
            }
            fs::remove_file(&selected).unwrap();
            fs::rename(&saved, &selected).unwrap();
            assert_eq!(copied_history(&s, &path), before);
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            assert_eq!(
                root.public_read_file(id, session.access.expose(), 101, FILE, 32)
                    .unwrap()
                    .payload(),
                b"synthetic-payload"
            );
            drop(root);
            verify_source(&s);
        }
    }
}

#[test]
fn v1_unbound_and_invalid_project_inputs_refuse_without_creating_files_or_upgrading_private_catalogs()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let s = source(t.path(), 1, true);
    let (id, key) = &s.root.credentials[0];
    let mut root = AccountRoot::open(&s.root.root, pool()).unwrap();
    let before = histories(&s.root);
    let token = s.tokens[0].access.expose();
    for project in [id.as_str(), "../private", "..", "/", "", "malformed\0"] {
        assert!(matches!(
            root.public_list_files(project, token, 100),
            Err(Error::Denied)
        ));
        assert!(matches!(
            root.public_file_info(project, token, 100, FILE),
            Err(Error::Denied)
        ));
        assert!(matches!(
            root.public_read_file(project, token, 100, FILE, usize::MAX),
            Err(Error::Denied)
        ));
    }
    assert_eq!(root.private_schema_version(id, key).unwrap(), 5);
    assert!(!s.root.root.join("files").exists());
    assert_eq!(histories(&s.root), before);
    let image = encode(&capture_account_file_components(&mut root, &mut []).unwrap());
    drop(root);
    crate::restore_account_file_archive_bytes_at(
        &image,
        &File::open(t.path()).unwrap(),
        "unbound-user",
        pool(),
        40,
    )
    .unwrap();
    let target = t.path().join("unbound-user");
    let mut root = AccountRoot::open_v2(&target, pool()).unwrap();
    let a = root.public_admission(id, key).unwrap();
    root.set_public_admission(id, key, a.revision, true)
        .unwrap();
    let session = root.sign_in(id, key, LOGIN, PASSWORD, 101).unwrap();
    let before = fs::read(target.join("private").join(id).join("redo.wal")).unwrap();
    assert!(matches!(
        root.public_list_files(id, session.access.expose(), 101),
        Err(Error::Denied)
    ));
    assert!(matches!(
        root.public_file_info(id, session.access.expose(), 101, FILE),
        Err(Error::Denied)
    ));
    assert!(matches!(
        root.public_read_file(id, session.access.expose(), 101, FILE, 32),
        Err(Error::Denied)
    ));
    assert_eq!(root.private_schema_version(id, key).unwrap(), 5);
    assert_eq!(
        fs::read(target.join("private").join(id).join("redo.wal")).unwrap(),
        before
    );
    assert_eq!(fs::read_dir(target.join("files")).unwrap().count(), 0);
    drop(root);
    verify_source(&s);
}
