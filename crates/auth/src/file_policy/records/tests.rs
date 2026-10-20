use super::super::{CreateRule, Definition, Rule};
use super::*;
use emilybase_catalog::{MAX_ENCODED_BYTES, decode_row};
use proptest::prelude::*;
const PROJECT: &str = "11111111111111111111111111111111";
const DOCUMENT:&[u8]=br#"{"version":1,"read":{"kind":"deny"},"create":{"kind":"deny"},"rename":{"kind":"deny"},"remove":{"kind":"deny"}}"#;
fn context() -> StoreContext<'static> {
    StoreContext {
        project: PROJECT,
        database: [3; 16],
    }
}
fn row() -> Row {
    encode(context(), 9, 5, DOCUMENT).unwrap().into_row()
}
#[test]
fn encoded_record_debug_does_not_expose_private_policy_or_scope() {
    let record = encode(context(), 9, 5, DOCUMENT).unwrap();
    let text = format!("{record:?}");
    assert!(!text.contains(PROJECT));
    assert!(!text.contains("version"));
    assert!(!text.contains("deny"));
    assert_eq!(record.row(), row());
    assert_eq!(record.into_row(), row());
}
fn repair(row: &mut [Value]) {
    let Value::Bytes(document) = &row[6] else {
        unreachable!()
    };
    row[7] = Value::Bytes(digest(context(), 9, 5, document).to_vec());
}

#[test]
fn exact_typed_records_match_independent_python_digest_and_preserve_original_bytes() {
    let row = row();
    schema().validate_row(&row).unwrap();
    let decoded = inspect(context(), &row).unwrap();
    assert_eq!(decoded.database(), &[3; 16]);
    assert_eq!(decoded.revision(), 9);
    assert_eq!(decoded.previous(), 5);
    assert_eq!(decoded.document(), DOCUMENT);
    let hex = decoded
        .sha256()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(
        hex,
        "5797c7d15aaf9a4d2b3a9854e8e4f53e65411e7b68bf1c3d6d6d3300e577b6e1"
    );
    let binary = encode_row(&row).unwrap();
    assert!(binary.len() <= MAX_ENCODED_BYTES);
    assert_eq!(decode_row(&binary).unwrap(), row);
    assert_eq!(binary.len(), 263);
    let binary_digest = Sha256::digest(&binary)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(
        binary_digest,
        "c98146ea91e91d59ff314779aad2f912dc5b6205ceb887d792af1801c067be86"
    );
    let mut spaced = DOCUMENT.to_vec();
    spaced.extend_from_slice(b" \n");
    let changed = encode(context(), 9, 5, &spaced).unwrap();
    let changed = inspect(context(), changed.row()).unwrap();
    assert_ne!(decoded.sha256(), changed.sha256());
    assert_eq!(changed.document(), spaced);
    let debug = format!("{decoded:?}");
    assert!(!debug.contains(PROJECT));
    assert!(!debug.contains("version"));
    assert!(!debug.contains("deny"));
}
#[test]
fn every_rule_combination_and_maximum_document_fits_original_slotted_page() {
    let rules = [Rule::Deny {}, Rule::Owner {}, Rule::Authenticated {}];
    let mut count = 0;
    for read in rules {
        for create in [CreateRule::Deny {}, CreateRule::SelfOwned {}] {
            for rename in rules {
                for remove in rules {
                    let definition = Definition {
                        version: 1,
                        read,
                        create,
                        rename,
                        remove,
                    };
                    let mut document = serde_json::to_vec(&definition).unwrap();
                    document.resize(MAX_DOCUMENT_BYTES, b' ');
                    let row = encode(context(), u64::MAX, u64::MAX - 1, &document)
                        .unwrap()
                        .into_row();
                    schema().validate_row(&row).unwrap();
                    let binary = encode_row(&row).unwrap();
                    let mut page = emilybase_storage::Page::new(1).unwrap();
                    let slot = page.insert(&binary).unwrap();
                    let bytes = page.encode();
                    let page = emilybase_storage::Page::decode(&bytes, 1).unwrap();
                    let restored = decode_row(page.get(slot).unwrap()).unwrap();
                    let decoded = inspect(context(), &restored).unwrap();
                    assert_eq!(decoded.document(), document);
                    assert_eq!(decoded.revision(), u64::MAX);
                    assert_eq!(decoded.previous(), u64::MAX - 1);
                    count += 1;
                }
            }
        }
    }
    assert_eq!(count, 54);
    let mut excessive = DOCUMENT.to_vec();
    excessive.resize(MAX_DOCUMENT_BYTES + 1, b' ');
    assert!(matches!(
        encode(context(), 9, 5, &excessive),
        Err(PolicyError::Limit)
    ));
    let mut bad = row();
    bad[6] = Value::Bytes(excessive);
    assert!(matches!(inspect(context(), &bad), Err(PolicyError::Limit)));
}
#[test]
fn trusted_native_scope_uses_all_database_bytes_and_canonical_key_without_excluding_zero_ids() {
    for database in [[0; 16], [0xab; 16], [0xff; 16]] {
        let context = StoreContext {
            database,
            ..context()
        };
        let row = encode(context, 9, 5, DOCUMENT).unwrap().into_row();
        assert_eq!(inspect(context, &row).unwrap().database(), &database);
    }
    let row = row();
    for index in 0..16 {
        let mut context = context();
        context.database[index] ^= 1;
        assert!(matches!(inspect(context, &row), Err(PolicyError::Scope)));
    }
    for project in ["22222222222222222222222222222222", "../outside", ""] {
        assert!(matches!(
            inspect(
                StoreContext {
                    project,
                    ..context()
                },
                &row
            ),
            Err(PolicyError::Scope)
        ));
    }
    let ctx = StoreContext {
        database: [0xab; 16],
        ..context()
    };
    let mut uppercase = encode(ctx, 9, 5, DOCUMENT).unwrap().into_row();
    uppercase[0] = Value::Text("AB".repeat(16));
    assert!(inspect(ctx, &uppercase).is_err());
    let mut foreign = row.clone();
    foreign[2] = Value::Text("22222222222222222222222222222222".into());
    assert!(matches!(
        inspect(context(), &foreign),
        Err(PolicyError::Scope)
    ));
    for length in [0, 15, 17, 1024] {
        let mut bad = row.clone();
        bad[5] = Value::Bytes(vec![3; length]);
        assert!(inspect(context(), &bad).is_err());
    }
}
#[test]
fn canonical_full_width_revisions_and_exact_types_arity_and_version_are_required() {
    for (revision, previous) in [(2, 0), (9, 2), (u64::MAX, u64::MAX - 1)] {
        let row = encode(context(), revision, previous, DOCUMENT)
            .unwrap()
            .into_row();
        let d = inspect(context(), &row).unwrap();
        assert_eq!(d.revision(), revision);
        assert_eq!(d.previous(), previous);
    }
    for (revision, previous) in [(0, 0), (1, 0), (2, 1), (2, 2), (9, 10)] {
        assert!(encode(context(), revision, previous, DOCUMENT).is_err());
        let mut bad = row();
        bad[3] = Value::Text(revision.to_string());
        bad[4] = Value::Text(previous.to_string());
        assert!(inspect(context(), &bad).is_err());
    }
    for index in [3, 4] {
        for text in [
            "",
            "+2",
            "02",
            "-2",
            " 2",
            "18446744073709551616",
            "999999999999999999999",
        ] {
            let mut bad = row();
            bad[index] = Value::Text(text.into());
            assert!(inspect(context(), &bad).is_err());
        }
    }
    for length in 0..8 {
        assert!(inspect(context(), &row()[..length]).is_err());
    }
    let mut extra = row();
    extra.push(Value::Null);
    assert!(inspect(context(), &extra).is_err());
    for index in 0..8 {
        let mut bad = row();
        bad[index] = Value::Null;
        assert!(inspect(context(), &bad).is_err());
    }
    for version in [-1, 0, 2, i64::MAX] {
        let mut bad = row();
        bad[1] = Value::Integer(version);
        assert!(matches!(
            inspect(context(), &bad),
            Err(PolicyError::Version)
        ));
    }
    for length in [0, 31, 33, 1024] {
        let mut bad = row();
        bad[7] = Value::Bytes(vec![0; length]);
        assert!(inspect(context(), &bad).is_err());
    }
}
#[test]
fn every_document_and_checksum_byte_change_is_detected_and_repaired_digest_does_not_bypass_grammar()
{
    for position in 0..DOCUMENT.len() {
        let mut bad = row();
        let Value::Bytes(document) = &mut bad[6] else {
            unreachable!()
        };
        document[position] ^= 1;
        assert!(inspect(context(), &bad).is_err());
    }
    for position in 0..32 {
        let mut bad = row();
        let Value::Bytes(checksum) = &mut bad[7] else {
            unreachable!()
        };
        checksum[position] ^= 1;
        assert!(inspect(context(), &bad).is_err());
    }
    for index in [3, 4] {
        let mut bad = row();
        bad[index] = Value::Text(if index == 3 { "10" } else { "2" }.into());
        assert!(inspect(context(), &bad).is_err());
    }
    let mut future = row();
    future[6] = Value::Bytes(
        String::from_utf8(DOCUMENT.to_vec())
            .unwrap()
            .replacen("\"version\":1", "\"version\":2", 1)
            .into_bytes(),
    );
    repair(&mut future);
    assert!(matches!(
        inspect(context(), &future),
        Err(PolicyError::Version)
    ));
    for document in [
        b"{}".to_vec(),
        b"null".to_vec(),
        vec![0xff],
        DOCUMENT.iter().copied().chain(*b"x").collect(),
        String::from_utf8(DOCUMENT.to_vec())
            .unwrap()
            .replacen("\"version\":1", "\"version\":1,\"version\":1", 1)
            .into_bytes(),
    ] {
        let mut bad = row();
        bad[6] = Value::Bytes(document);
        repair(&mut bad);
        assert!(inspect(context(), &bad).is_err());
    }
}
#[test]
fn parsed_model_requires_genuine_borrowed_private_principal_and_remains_scoped() {
    use crate::{accounts::AccountStore, password::PasswordPool};
    let t = tempfile::tempdir().unwrap();
    let mut store = AccountStore::create(
        t.path().join("accounts"),
        PROJECT,
        PasswordPool::new(1).unwrap(),
    )
    .unwrap();
    let password = b"synthetic-record-password";
    store.create_user("first", password).unwrap();
    store.enable_session_clock(100).unwrap();
    let session = store.sign_in("first", password, 100).unwrap();
    let principal = store.verify_access(session.access.expose(), 100).unwrap();
    let owner = principal.account().id;
    let definition = Definition {
        version: 1,
        read: Rule::Owner {},
        create: CreateRule::SelfOwned {},
        rename: Rule::Deny {},
        remove: Rule::Deny {},
    };
    let row = encode(context(), 9, 5, &serde_json::to_vec(&definition).unwrap())
        .unwrap()
        .into_row();
    let decoded = inspect(context(), &row).unwrap();
    assert!(decoded.check_context(context(), &principal).is_ok());
    assert!(
        decoded
            .authorize(context(), &principal, Change::Read { owner })
            .is_ok()
    );
    assert!(
        decoded
            .authorize(context(), &principal, Change::Create { owner })
            .is_ok()
    );
    assert!(matches!(
        decoded.authorize(context(), &principal, Change::Remove { owner }),
        Err(PolicyError::Denied)
    ));
    let mut changed = context();
    changed.database[15] ^= 1;
    assert!(matches!(
        decoded.check_context(changed, &principal),
        Err(PolicyError::Scope)
    ));
}
fn value() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Boolean),
        any::<i64>().prop_map(Value::Integer),
        "[a-zA-Z0-9 +:-]{0,40}".prop_map(Value::Text),
        proptest::collection::vec(any::<u8>(), 0..1100).prop_map(Value::Bytes)
    ]
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_native_ids_rules_and_revisions_roundtrip_exactly(database in any::<[u8;16]>(),revision in 2u64..u64::MAX,padding in 0usize..800,r in 0usize..3,c in any::<bool>(),n in 0usize..3,d in 0usize..3){
        let rules=[Rule::Deny{},Rule::Owner{},Rule::Authenticated{}];let definition=Definition{version:1,read:rules[r],create:if c{CreateRule::SelfOwned{}}else{CreateRule::Deny{}},rename:rules[n],remove:rules[d]};let mut doc=serde_json::to_vec(&definition).unwrap();doc.extend(std::iter::repeat_n(b' ',padding));let context=StoreContext{database,..context()};let record=encode(context,revision,0,&doc).unwrap();let binary=encode_row(record.row()).unwrap();let decoded=inspect(context,&decode_row(&binary).unwrap()).unwrap();prop_assert_eq!(decoded.document(),doc);prop_assert_eq!(decoded.database(),&database);prop_assert_eq!(decoded.revision(),revision);
    }
    #[test]
    fn arbitrary_typed_rows_and_binary_records_never_panic(row in proptest::collection::vec(value(),0..11),bytes in proptest::collection::vec(any::<u8>(),0..4097)){
        let _=inspect(context(),&row);if let Ok(row)=decode_row(&bytes){let _=inspect(context(),&row);}
    }
}
