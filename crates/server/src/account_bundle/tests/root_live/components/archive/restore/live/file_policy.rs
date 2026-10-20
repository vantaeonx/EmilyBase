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
