use super::*;
use crate::{accounts::AccountStore, password::PasswordPool};
use proptest::prelude::*;
use std::path::Path;
const PROJECT: &str = "11111111111111111111111111111111";
const OTHER: &str = "22222222222222222222222222222222";
const PASSWORD: &[u8] = b"synthetic-file-password";
fn context() -> StoreContext<'static> {
    StoreContext {
        project: PROJECT,
        database: [3; 16],
    }
}
fn definition(read: Rule, create: CreateRule, rename: Rule, remove: Rule) -> Definition {
    Definition {
        version: 1,
        read,
        create,
        rename,
        remove,
    }
}
fn document() -> Vec<u8> {
    serde_json::to_vec(&Definition::deny_all()).unwrap()
}
fn store(path: &Path, project: &str, compacted: bool) -> AccountStore {
    let mut store = AccountStore::create(path, project, PasswordPool::new(1).unwrap()).unwrap();
    store.create_user("first", PASSWORD).unwrap();
    store.create_user("second", PASSWORD).unwrap();
    store.enable_session_clock(100).unwrap();
    if compacted {
        store.compact().unwrap();
    }
    store
}
fn changes(owner: [u8; 16]) -> [Change; 4] {
    [
        Change::Read { owner },
        Change::Create { owner },
        Change::Rename { owner },
        Change::Remove { owner },
    ]
}
#[test]
fn decoder_requires_every_action_and_refuses_duplicate_unknown_ambiguous_and_malformed_fields() {
    let json: String = String::from_utf8(document()).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let mut bad = vec![
        vec![],
        vec![0xff],
        b"null".to_vec(),
        b"[]".to_vec(),
        b"{}".to_vec(),
        format!("{json}false").into_bytes(),
    ];
    for name in ["version", "read", "create", "rename", "remove"] {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(name);
        bad.push(serde_json::to_vec(&missing).unwrap());
        let repeated = format!("{{\"{name}\":{},{}", value[name], &json[1..]);
        bad.push(repeated.into_bytes());
    }
    for name in ["read", "rename", "remove"] {
        for rule in [
            serde_json::json!({"kind":"unknown"}),
            serde_json::json!({"kind":"deny","path":"synthetic"}),
            serde_json::json!({"kind":"owner","terms":[]}),
            serde_json::json!({"kind":"authenticated","role":"synthetic"}),
            serde_json::json!({"kind":"self_owned"}),
            serde_json::json!(true),
        ] {
            let mut changed = value.clone();
            changed[name] = rule;
            bad.push(serde_json::to_vec(&changed).unwrap());
        }
    }
    for rule in [
        serde_json::json!({"kind":"owner"}),
        serde_json::json!({"kind":"authenticated"}),
        serde_json::json!({"kind":"self_owned","owner":"synthetic"}),
    ] {
        let mut changed = value.clone();
        changed["create"] = rule;
        bad.push(serde_json::to_vec(&changed).unwrap());
    }
    bad.push(
        json.replacen(
            "\"kind\":\"deny\"",
            "\"kind\":\"deny\",\"kind\":\"owner\"",
            1,
        )
        .into_bytes(),
    );
    let mut unknown = value;
    unknown["bucket"] = serde_json::json!("synthetic");
    bad.push(serde_json::to_vec(&unknown).unwrap());
    for input in bad {
        assert!(decode(&input).is_err(), "invalid file rule accepted");
    }
    assert_eq!(decode(&document()).unwrap(), Definition::deny_all());
}
#[test]
fn exact_input_bound_and_typed_scope_version_failures_do_not_echo_metadata() {
    let mut exact = document();
    exact.resize(MAX_DOCUMENT_BYTES, b' ');
    assert!(decode(&exact).is_ok());
    exact.push(b' ');
    assert!(matches!(decode(&exact), Err(PolicyError::Limit)));
    for project in ["../outside", "", "1111111111111111111111111111111A"] {
        assert!(matches!(
            BoundPolicy::compile(
                StoreContext {
                    project,
                    database: [3; 16]
                },
                &Definition::deny_all()
            ),
            Err(PolicyError::Scope)
        ));
    }
    for version in [0, 2, u16::MAX] {
        let mut d = Definition::deny_all();
        d.version = version;
        let encoded = serde_json::to_vec(&d).unwrap();
        assert!(decode(&encoded).is_ok());
        assert!(matches!(
            BoundPolicy::compile(context(), &d),
            Err(PolicyError::Version)
        ));
    }
    let p = BoundPolicy::compile(context(), &Definition::deny_all()).unwrap();
    let text = format!("{p:?} {:?} {:?}", Definition::deny_all(), Rule::Owner {});
    assert!(!text.contains(PROJECT));
    assert!(!text.contains("owner"));
    for error in [
        PolicyError::Document,
        PolicyError::Version,
        PolicyError::Limit,
        PolicyError::Scope,
        PolicyError::Denied,
    ] {
        assert!(!format!("{error:?} {error}").contains(PROJECT));
    }
}
#[test]
fn all_explicit_permission_combinations_match_independent_owner_matrix_without_storage_writes() {
    let rules = [Rule::Deny {}, Rule::Owner {}, Rule::Authenticated {}];
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("accounts");
        let mut store = store(&path, PROJECT, compact);
        let session = store.sign_in("first", PASSWORD, 100).unwrap();
        let before = std::fs::read(path.join("redo.wal")).unwrap();
        let principal = store.verify_access(session.access.expose(), 100).unwrap();
        let user = principal.account().id;
        let mut foreign = user;
        foreign[0] ^= 1;
        for r in 0..3 {
            for c in 0..2 {
                for n in 0..3 {
                    for d in 0..3 {
                        let definition = definition(
                            rules[r],
                            if c == 1 {
                                CreateRule::SelfOwned {}
                            } else {
                                CreateRule::Deny {}
                            },
                            rules[n],
                            rules[d],
                        );
                        let p = BoundPolicy::compile(context(), &definition).unwrap();
                        for owner in [user, foreign] {
                            let own = owner == user;
                            let expected = [
                                r == 2 || (r == 1 && own),
                                c == 1 && own,
                                n == 2 || (n == 1 && own),
                                d == 2 || (d == 1 && own),
                            ];
                            for (change, allowed) in changes(owner).into_iter().zip(expected) {
                                assert_eq!(
                                    p.authorize(context(), &principal, change).is_ok(),
                                    allowed
                                );
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
    }
}
#[test]
fn real_project_session_and_exact_native_database_binding_are_required_before_any_reference_decision()
 {
    let t = tempfile::tempdir().unwrap();
    let mut first = store(&t.path().join("first"), PROJECT, false);
    let mut second = store(&t.path().join("second"), OTHER, false);
    let a = first.sign_in("first", PASSWORD, 100).unwrap();
    let b = second.sign_in("first", PASSWORD, 100).unwrap();
    let p = BoundPolicy::compile(
        context(),
        &definition(
            Rule::Authenticated {},
            CreateRule::SelfOwned {},
            Rule::Authenticated {},
            Rule::Authenticated {},
        ),
    )
    .unwrap();
    let principal = first.verify_access(a.access.expose(), 100).unwrap();
    let foreign = second.verify_access(b.access.expose(), 100).unwrap();
    assert!(matches!(
        p.check_context(context(), &foreign),
        Err(PolicyError::Scope)
    ));
    for i in 0..16 {
        let mut changed = context();
        changed.database[i] ^= 1;
        assert!(matches!(
            p.check_context(changed, &principal),
            Err(PolicyError::Scope)
        ));
        for change in changes(principal.account().id) {
            assert!(matches!(
                p.authorize(changed, &principal, change),
                Err(PolicyError::Scope)
            ));
        }
    }
    let foreign_context = StoreContext {
        project: OTHER,
        database: context().database,
    };
    for change in changes(foreign.account().id) {
        assert!(matches!(
            p.authorize(foreign_context, &foreign, change),
            Err(PolicyError::Scope)
        ));
    }
    // No existing reference is needed to enforce the context gate.
    assert!(p.check_context(context(), &principal).is_ok());
}
#[test]
fn deny_all_and_operation_independence_do_not_turn_read_or_creation_into_other_permissions() {
    let t = tempfile::tempdir().unwrap();
    let mut store = store(&t.path().join("accounts"), PROJECT, false);
    let session = store.sign_in("first", PASSWORD, 100).unwrap();
    let principal = store.verify_access(session.access.expose(), 100).unwrap();
    let owner = principal.account().id;
    let deny = BoundPolicy::compile(context(), &Definition::deny_all()).unwrap();
    for change in changes(owner) {
        assert!(matches!(
            deny.authorize(context(), &principal, change),
            Err(PolicyError::Denied)
        ));
    }
    let p = BoundPolicy::compile(
        context(),
        &definition(
            Rule::Deny {},
            CreateRule::Deny {},
            Rule::Owner {},
            Rule::Deny {},
        ),
    )
    .unwrap();
    assert!(
        p.authorize(context(), &principal, Change::Rename { owner })
            .is_ok()
    );
    for change in [
        Change::Read { owner },
        Change::Create { owner },
        Change::Remove { owner },
    ] {
        assert!(matches!(
            p.authorize(context(), &principal, change),
            Err(PolicyError::Denied)
        ));
    }
}
#[test]
fn stale_logout_disabled_password_rotated_expired_and_restored_credentials_cannot_supply_a_principal()
 {
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("accounts");
        let mut store = store(&path, PROJECT, compact);
        let first = store.sign_in("first", PASSWORD, 100).unwrap();
        assert!(store.verify_access(first.refresh.expose(), 100).is_err());
        let rotated = store.refresh_session(first.refresh.expose(), 101).unwrap();
        assert!(store.verify_access(first.access.expose(), 101).is_err());
        store.logout_session(rotated.refresh.expose(), 102).unwrap();
        assert!(store.verify_access(rotated.access.expose(), 102).is_err());
        let active = store.sign_in("first", PASSWORD, 103).unwrap();
        store.set_disabled("first", true).unwrap();
        assert!(store.verify_access(active.access.expose(), 103).is_err());
        store.set_disabled("first", false).unwrap();
        assert!(store.verify_access(active.access.expose(), 103).is_err());
        let active = store.sign_in("first", PASSWORD, 104).unwrap();
        store
            .change_password("first", PASSWORD, b"synthetic-replacement-password")
            .unwrap();
        assert!(store.verify_access(active.access.expose(), 104).is_err());
        let active = store
            .sign_in("first", b"synthetic-replacement-password", 105)
            .unwrap();
        let image = store.backup_image().unwrap();
        let restored = t.path().join("restored-copy");
        crate::accounts::restore_private_account_bytes(
            &image,
            &restored,
            PROJECT,
            PasswordPool::new(1).unwrap(),
            106,
        )
        .unwrap();
        let mut copy =
            AccountStore::open(&restored, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
        assert!(copy.verify_access(active.access.expose(), 106).is_err());
        let fresh = copy
            .sign_in("first", b"synthetic-replacement-password", 107)
            .unwrap();
        let principal = copy.verify_access(fresh.access.expose(), 107).unwrap();
        let owner = principal.account().id;
        let p = BoundPolicy::compile(
            context(),
            &definition(
                Rule::Owner {},
                CreateRule::SelfOwned {},
                Rule::Owner {},
                Rule::Owner {},
            ),
        )
        .unwrap();
        assert!(
            p.authorize(context(), &principal, Change::Read { owner })
                .is_ok()
        );
        drop(principal);
        assert!(
            copy.verify_access(fresh.access.expose(), fresh.metadata.access_until)
                .is_err()
        );
        assert!(store.verify_access(active.access.expose(), 106).is_ok());
        drop(store);
        let mut reopened =
            AccountStore::open(&path, PROJECT, PasswordPool::new(1).unwrap()).unwrap();
        assert!(reopened.verify_access(active.access.expose(), 106).is_ok());
    }
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn arbitrary_bytes_decode_without_panics_or_unbounded_normalized_output(bytes in proptest::collection::vec(any::<u8>(),0..2049)) {
        if let Ok(d)=decode(&bytes) {let encoded=serde_json::to_vec(&d).unwrap();prop_assert!(encoded.len()<=MAX_DOCUMENT_BYTES);prop_assert_eq!(&decode(&encoded).unwrap(),&d);let _=BoundPolicy::compile(context(),&d);}
    }
    #[test]
    fn generated_explicit_rules_roundtrip_and_compile_exactly(r in 0usize..3,c in any::<bool>(),n in 0usize..3,d in 0usize..3,version in 0u16..4) {
        let rules=[Rule::Deny{},Rule::Owner{},Rule::Authenticated{}];let mut definition=definition(rules[r],if c{CreateRule::SelfOwned{}}else{CreateRule::Deny{}},rules[n],rules[d]);definition.version=version;let encoded=serde_json::to_vec(&definition).unwrap();let decoded=decode(&encoded).unwrap();prop_assert_eq!(&decoded,&definition);prop_assert_eq!(BoundPolicy::compile(context(),&decoded).is_ok(),version==VERSION);
    }
}

#[test]
fn generated_decisions_use_every_owner_byte_and_exact_scope_with_a_genuine_borrowed_principal() {
    use proptest::test_runner::{Config, TestRunner};
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("accounts");
    let mut store = store(&path, PROJECT, false);
    let session = store.sign_in("first", PASSWORD, 100).unwrap();
    let before = std::fs::read(path.join("redo.wal")).unwrap();
    let principal = store.verify_access(session.access.expose(), 100).unwrap();
    let user = principal.account().id;
    let mut runner = TestRunner::new(Config::with_cases(128));
    runner
        .run(
            &(
                any::<bool>(),
                0usize..16,
                1u8..=255,
                0usize..4,
                0usize..3,
                any::<bool>(),
                any::<bool>(),
            ),
            |(own, position, delta, operation, rule, create, scope_changed)| {
                let rules = [Rule::Deny {}, Rule::Owner {}, Rule::Authenticated {}];
                let definition = definition(
                    rules[rule],
                    if create {
                        CreateRule::SelfOwned {}
                    } else {
                        CreateRule::Deny {}
                    },
                    rules[rule],
                    rules[rule],
                );
                let policy = BoundPolicy::compile(context(), &definition).unwrap();
                let mut owner = user;
                if !own {
                    owner[position] ^= delta;
                }
                let mut selected = context();
                if scope_changed {
                    selected.database[position] ^= delta;
                }
                let change = match operation {
                    0 => Change::Read { owner },
                    1 => Change::Create { owner },
                    2 => Change::Rename { owner },
                    _ => Change::Remove { owner },
                };
                let result = policy.authorize(selected, &principal, change);
                if scope_changed {
                    prop_assert!(matches!(result, Err(PolicyError::Scope)));
                } else {
                    let permitted = if operation == 1 {
                        create && own
                    } else {
                        rule == 2 || (rule == 1 && own)
                    };
                    prop_assert_eq!(result.is_ok(), permitted);
                    if !permitted {
                        prop_assert!(matches!(result, Err(PolicyError::Denied)));
                    }
                }
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
}
