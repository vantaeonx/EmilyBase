//! Model decisions over actual file metadata; not an installed file-service route.
use super::*;
use emilybase_auth::file_policy::{
    BoundPolicy, Change, CreateRule, Definition, PolicyError, Rule, StoreContext,
};

#[test]
fn native_file_owner_bytes_and_database_context_match_real_private_principals_without_granting_a_route()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 2, compact);
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let (id, key) = &s.root.credentials[0];
        let (other, other_key) = &s.root.credentials[1];
        let session = root.sign_in(id, key, LOGIN, PASSWORD, 101).unwrap();
        let other_session = root
            .sign_in(other, other_key, LOGIN, PASSWORD, 101)
            .unwrap();
        let user = root
            .with_access(id, key, session.access.expose(), 101, |principal| {
                principal.account().id
            })
            .unwrap();
        let info = root.inspect_project_files(id, key).unwrap();
        let context = StoreContext {
            project: id,
            database: *info.database_id(),
        };
        let definition = Definition {
            version: 1,
            read: Rule::Owner {},
            create: CreateRule::SelfOwned {},
            rename: Rule::Owner {},
            remove: Rule::Owner {},
        };
        let policy = BoundPolicy::compile(context, &definition).unwrap();
        // The backend can create metadata, but that act is not a user grant.
        let owned = root
            .publish_project_file(
                id,
                key,
                crate::ProjectFileUpload {
                    id: FileId::from_bytes([71; 16]),
                    object: ObjectId::from_bytes([72; 16]),
                    owner: user,
                    name: "synthetic-owned",
                    payload: b"synthetic-owned-payload",
                },
            )
            .unwrap();
        let foreign = root
            .publish_project_file(
                id,
                key,
                crate::ProjectFileUpload {
                    id: FileId::from_bytes([73; 16]),
                    object: ObjectId::from_bytes([74; 16]),
                    owner: [75; 16],
                    name: "synthetic-foreign",
                    payload: b"synthetic-foreign-payload",
                },
            )
            .unwrap();
        let before = copied_history(&s, &path);
        root.with_access(id, key, session.access.expose(), 101, |principal| {
            policy.check_context(context, &principal).unwrap();
            for change in [
                Change::Read {
                    owner: *owned.owner(),
                },
                Change::Create { owner: user },
                Change::Rename {
                    owner: *owned.owner(),
                },
                Change::Remove {
                    owner: *owned.owner(),
                },
            ] {
                assert!(policy.authorize(context, &principal, change).is_ok());
            }
            for change in [
                Change::Read {
                    owner: *foreign.owner(),
                },
                Change::Create {
                    owner: *foreign.owner(),
                },
                Change::Rename {
                    owner: *foreign.owner(),
                },
                Change::Remove {
                    owner: *foreign.owner(),
                },
            ] {
                assert!(matches!(
                    policy.authorize(context, &principal, change),
                    Err(PolicyError::Denied)
                ));
            }
            let mut changed = context;
            changed.database[0] ^= 1;
            assert!(matches!(
                policy.check_context(changed, &principal),
                Err(PolicyError::Scope)
            ));
        })
        .unwrap();
        root.with_access(
            other,
            other_key,
            other_session.access.expose(),
            101,
            |principal| {
                assert!(matches!(
                    policy.check_context(context, &principal),
                    Err(PolicyError::Scope)
                ));
            },
        )
        .unwrap();
        assert_eq!(copied_history(&s, &path), before);
        assert_eq!(
            root.read_project_file(id, key, owned.id(), 32)
                .unwrap()
                .payload(),
            b"synthetic-owned-payload"
        );
        assert!(matches!(
            root.read_project_file(id, session.access.expose(), owned.id(), 32),
            Err(Error::Denied)
        ));
        root.logout_session(id, key, session.refresh.expose(), 102)
            .unwrap();
        assert!(
            root.with_access(id, key, session.access.expose(), 102, |_| panic!(
                "revoked session supplied a file principal"
            ))
            .is_err()
        );
        drop(root);
        verify_source(&s);
    }
}

#[test]
fn explicit_private_v6_catalog_survives_native_v2_reopen_capture_and_independent_common_restore() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 2, compact);
        let (id, key) = &s.root.credentials[0];
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        let native = root.inspect_project_files(id, key).unwrap();
        let database = *native.database_id();
        drop(root);
        // Explicit offline native migration after the real common owner closes.
        let private_path = path.join("private").join(id);
        let mut private =
            emilybase_auth::accounts::AccountStore::open(&private_path, id, pool()).unwrap();
        private.enable_row_policy_catalog().unwrap();
        let admission = private.enable_public_admission_catalog().unwrap();
        let opened = private
            .set_public_admission(admission.revision, true)
            .unwrap();
        private.enable_file_policy_catalog().unwrap();
        let context = StoreContext {
            project: id,
            database,
        };
        let document = serde_json::to_vec(&Definition {
            version: 1,
            read: Rule::Owner {},
            create: CreateRule::SelfOwned {},
            rename: Rule::Deny {},
            remove: Rule::Deny {},
        })
        .unwrap();
        let installed = private.install_file_policy(context, 0, &document).unwrap();
        let source_session = private.sign_in(LOGIN, PASSWORD, 101).unwrap();
        drop(private);
        let before = copied_history(&s, &path);
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        assert_eq!(root.private_schema_version(id, key).unwrap(), 6);
        assert_eq!(root.public_admission(id, key).unwrap(), opened);
        root.with_access(id, key, source_session.access.expose(), 101, |_| ())
            .unwrap();
        let snapshot = root.capture_v2().unwrap();
        let bytes = encode(&snapshot);
        assert_eq!(copied_history(&s, &path), before);
        let target = path.parent().unwrap().join("independent-v6");
        let report = crate::restore_account_file_archive_bytes_at(
            &bytes,
            &File::open(path.parent().unwrap()).unwrap(),
            "independent-v6",
            pool(),
            50,
        )
        .unwrap();
        assert_eq!(
            report
                .private_accounts
                .iter()
                .find(|p| p.project == *id)
                .unwrap()
                .inventory
                .private_version,
            6
        );
        let mut second = AccountRoot::open_v2(&target, pool()).unwrap();
        assert_eq!(
            *second.inspect_project_files(id, key).unwrap().database_id(),
            database
        );
        assert!(!second.public_admission(id, key).unwrap().enabled);
        assert!(
            second
                .with_access(id, key, source_session.access.expose(), 50, |_| ())
                .is_err()
        );
        let fresh = second.sign_in(id, key, LOGIN, PASSWORD, 50).unwrap();
        second
            .with_access(id, key, fresh.access.expose(), 50, |_| ())
            .unwrap();
        drop(second);
        let mut private = emilybase_auth::accounts::AccountStore::open(
            target.join("private").join(id),
            id,
            pool(),
        )
        .unwrap();
        assert_eq!(private.file_policy_receipts().unwrap(), vec![installed]);
        let proof = private
            .verify_file_policy_access(fresh.access.expose(), 50, context)
            .unwrap();
        proof
            .authorize(
                context,
                Change::Read {
                    owner: proof.account_id(),
                },
            )
            .unwrap();
        drop(proof);
        assert_eq!(copied_history(&s, &path), before);
        drop(root);
        verify_source(&s);
    }
}
