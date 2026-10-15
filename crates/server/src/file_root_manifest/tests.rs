use super::*;
use crate::inspect_account_bundle_root_manifest_bytes;
use proptest::prelude::*;
use serde_json::{Value, json};

fn id(value: usize) -> String {
    format!("{value:032x}")
}
fn fixture(count: usize) -> AccountFileRootManifest {
    AccountFileRootManifest::new(
        (0..count).map(id).collect(),
        (0..count)
            .map(|p| FileRootBinding::new(&id(p), &id(1000 + p)).unwrap())
            .collect(),
        100,
    )
    .unwrap()
}
fn raw(payload: &Value) -> Vec<u8> {
    // Explicit field order independent of the canonical Rust structs.
    let stores = ordered_stores(&payload["file_stores"]);
    let value = format!(
        "{{\"version\":{},\"private_projects\":{},\"file_stores\":{},\"reset_at\":{}}}",
        payload["version"], payload["private_projects"], stores, payload["reset_at"]
    );
    format!(
        "{{\"payload\":{value},\"checksum\":{}}}",
        crc32fast::hash(value.as_bytes())
    )
    .into_bytes()
}
fn ordered_stores(stores: &Value) -> String {
    let Some(stores) = stores.as_array() else {
        return stores.to_string();
    };
    let stores = stores
        .iter()
        .map(|store| {
            let Some(fields) = store.as_object() else {
                return store.to_string();
            };
            let mut text = format!(
                "{{\"project\":{},\"database_id\":{}",
                store["project"], store["database_id"]
            );
            for (key, value) in fields {
                if key != "project" && key != "database_id" {
                    text.push_str(&format!(",{}:{value}", serde_json::to_string(key).unwrap()));
                }
            }
            text.push('}');
            text
        })
        .collect::<Vec<_>>();
    format!("[{}]", stores.join(","))
}

#[test]
fn exact_v2_golden_encoding_is_stable_and_has_no_filesystem_authority() {
    let project = "01010101010101010101010101010101";
    let database = "02020202020202020202020202020202";
    let binding = FileRootBinding::new(project, database).unwrap();
    assert_eq!(binding.project(), project);
    assert_eq!(binding.database_id(), database);
    let value = AccountFileRootManifest::new(vec![project.into()], vec![binding], 100).unwrap();
    let expected = format!(
        "{{\"payload\":{{\"version\":2,\"private_projects\":[\"{project}\"],\"file_stores\":[{{\"project\":\"{project}\",\"database_id\":\"{database}\"}}],\"reset_at\":100}},\"checksum\":2506296039}}"
    );
    assert_eq!(value.to_bytes().unwrap(), expected.as_bytes());
    assert_eq!(
        inspect_account_file_root_manifest_bytes(expected.as_bytes()).unwrap(),
        value
    );
    assert_eq!(value.reset_at(), 100);
}

#[test]
fn empty_subset_and_full_128_rosters_roundtrip_under_explicit_manifest_bound() {
    for count in [0, 1, 4, 128] {
        let value = fixture(count);
        let bytes = value.to_bytes().unwrap();
        assert!(bytes.len() < MAX_ACCOUNT_FILE_ROOT_MANIFEST_BYTES);
        assert_eq!(
            inspect_account_file_root_manifest_bytes(&bytes).unwrap(),
            value
        );
        assert_eq!(value.private_projects().len(), count);
        assert_eq!(value.file_stores().len(), count);
    }
    let value = AccountFileRootManifest::new(
        vec![id(0), id(1), id(2)],
        vec![FileRootBinding::new(&id(1), &id(1000)).unwrap()],
        i64::MAX as u64,
    )
    .unwrap();
    assert_eq!(
        inspect_account_file_root_manifest_bytes(&value.to_bytes().unwrap()).unwrap(),
        value
    );
    assert!(matches!(
        AccountFileRootManifest::new(vec![], vec![], i64::MAX as u64 + 1),
        Err(Error::FileRootManifest(_))
    ));
}

#[test]
fn constructors_refuse_noncanonical_identity_order_membership_duplicate_scope_and_counts() {
    let invalid = [
        "",
        "../private",
        "0101010101010101010101010101010A",
        "０１０１０１０１０１０１０１０１０１０１０１０１０１０１０１０１０１０１０１０１",
        "01010101010101010101010101010101\0",
    ];
    for bad in invalid {
        assert!(FileRootBinding::new(bad, &id(1000)).is_err());
        assert!(FileRootBinding::new(&id(1), bad).is_err());
        assert!(AccountFileRootManifest::new(vec![bad.into()], vec![], 0).is_err());
    }
    for private in [vec![id(1), id(1)], vec![id(2), id(1)]] {
        assert!(AccountFileRootManifest::new(private, vec![], 0).is_err());
    }
    let binding = FileRootBinding::new(&id(1), &id(1000)).unwrap();
    assert!(AccountFileRootManifest::new(vec![], vec![binding.clone()], 0).is_err());
    assert!(AccountFileRootManifest::new(vec![id(1)], vec![binding.clone(), binding], 0).is_err());
    let stores = vec![
        FileRootBinding::new(&id(2), &id(1002)).unwrap(),
        FileRootBinding::new(&id(1), &id(1001)).unwrap(),
    ];
    assert!(AccountFileRootManifest::new(vec![id(1), id(2)], stores, 0).is_err());
    let stores = (1..=2)
        .map(|p| FileRootBinding::new(&id(p), &id(1000)).unwrap())
        .collect();
    assert!(AccountFileRootManifest::new(vec![id(1), id(2)], stores, 0).is_err());
    assert!(matches!(
        AccountFileRootManifest::new((0..129).map(id).collect(), vec![], 0),
        Err(Error::Limit)
    ));
    let stores = (0..129)
        .map(|p| FileRootBinding::new(&id(p), &id(1000 + p)).unwrap())
        .collect();
    assert!(matches!(
        AccountFileRootManifest::new(vec![], stores, 0),
        Err(Error::Limit)
    ));
}

#[test]
fn checksum_repaired_semantic_mutations_refuse_all_roster_and_schema_violations() {
    let original: Value = serde_json::from_slice(&fixture(2).to_bytes().unwrap()).unwrap();
    assert!(inspect_account_file_root_manifest_bytes(&raw(&original["payload"])).is_ok());
    for kind in 0..16 {
        let mut payload = original["payload"].clone();
        match kind {
            0 => payload["version"] = 3.into(),
            1 => payload["version"] = 1.into(),
            2 => payload["private_projects"] = json!([id(1), id(0)]),
            3 => payload["private_projects"] = json!([id(0), id(0)]),
            4 => payload["private_projects"] = json!([]),
            5 => payload["private_projects"][0] = "../scope".into(),
            6 => payload["file_stores"][1]["project"] = id(0).into(),
            7 => payload["file_stores"][0]["project"] = id(9).into(),
            8 => payload["file_stores"][1]["database_id"] = id(1000).into(),
            9 => {
                payload["file_stores"][0]["database_id"] =
                    "ABCDEFABCDEFABCDEFABCDEFABCDEFABCD".into()
            }
            10 => payload["reset_at"] = (i64::MAX as u64 + 1).into(),
            11 => payload["reset_at"] = json!(-1),
            12 => payload["file_stores"] = json!("not-a-list"),
            13 => payload["private_projects"] = json!((0..129).map(id).collect::<Vec<_>>()),
            14 => {
                payload["file_stores"] = json!(
                    (0..129)
                        .map(|p| json!({"project":id(p),"database_id":id(1000+p)}))
                        .collect::<Vec<_>>()
                )
            }
            _ => payload["file_stores"][0]["unexpected"] = true.into(),
        }
        assert!(
            inspect_account_file_root_manifest_bytes(&raw(&payload)).is_err(),
            "kind={kind}"
        );
    }
}

#[test]
fn duplicate_unknown_alternate_escaping_order_whitespace_and_wrong_json_types_refuse() {
    let original = fixture(1).to_bytes().unwrap();
    let text = String::from_utf8(original.clone()).unwrap();
    let values = vec![
        text.replace("\"version\":2", "\"version\":2,\"version\":2"),
        text.replace("\"reset_at\":100", "\"reset_at\":100,\"unknown\":false"),
        text.replace("\"checksum\":", "\"checksum\":0,\"checksum\":"),
        text.replace("\"payload\":{", "\"payload\":{\"extra\":null,"),
        text.replace("\"private_projects\":[", "\"private_projects\":[null,"),
        text.replace("\"private_projects\":[", "\"private_projects\":[true,"),
        text.replace("\"reset_at\":100", "\"reset_at\":100.0"),
        text.replace("\"reset_at\":100", "\"reset_at\":1e2"),
        text.replace(
            "00000000000000000000000000000000",
            "\\u00300000000000000000000000000000000",
        ),
        format!("{text}\n"),
        format!(" {text}"),
        format!("{text}{{}}"),
        text.replace("\"version\":2,", "\"version\":2, "),
        text.replace("\"file_stores\":", "\"file_stores\":null,\"ignored\":"),
    ];
    for value in values {
        assert!(inspect_account_file_root_manifest_bytes(value.as_bytes()).is_err());
    }
    let payload: Value = serde_json::from_slice(&original).unwrap();
    // A well-formed alternate envelope order is still a distinct encoding.
    let alternate = format!(
        "{{\"checksum\":{},\"payload\":{}}}",
        payload["checksum"], payload["payload"]
    );
    assert!(inspect_account_file_root_manifest_bytes(alternate.as_bytes()).is_err());
}

#[test]
fn every_truncation_single_byte_change_and_oversized_input_refuses_without_panic() {
    let bytes = fixture(2).to_bytes().unwrap();
    for end in 0..bytes.len() {
        assert!(inspect_account_file_root_manifest_bytes(&bytes[..end]).is_err());
    }
    for offset in 0..bytes.len() {
        let mut changed = bytes.clone();
        changed[offset] ^= 0x80;
        assert!(inspect_account_file_root_manifest_bytes(&changed).is_err());
    }
    assert!(matches!(
        inspect_account_file_root_manifest_bytes(&vec![
            b' ';
            MAX_ACCOUNT_FILE_ROOT_MANIFEST_BYTES + 1
        ]),
        Err(Error::Limit)
    ));
    for bytes in [b"null".as_slice(), b"[]", b"0", b"\xff", b"{\"payload\":"] {
        assert!(inspect_account_file_root_manifest_bytes(bytes).is_err());
    }
}

#[test]
fn legacy_reader_and_live_root_refuse_v2_without_creating_files_or_migrating_state() {
    use emilybase_auth::password::PasswordPool;
    use std::fs;
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("root");
    let pool = PasswordPool::new(1).unwrap();
    let report = crate::initialize_account_root(&path, "synthetic", pool.clone(), 100).unwrap();
    let old = fs::read(path.join("root.json")).unwrap();
    assert_eq!(
        inspect_account_bundle_root_manifest_bytes(&old)
            .unwrap()
            .version,
        1
    );
    assert!(inspect_account_file_root_manifest_bytes(&old).is_err());
    let project = &report.private_accounts[0].project;
    let new = AccountFileRootManifest::new(
        vec![project.clone()],
        vec![FileRootBinding::new(project, &id(1000)).unwrap()],
        100,
    )
    .unwrap()
    .to_bytes()
    .unwrap();
    assert!(inspect_account_bundle_root_manifest_bytes(&new).is_err());
    fn files(path: &std::path::Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
        let mut all = std::collections::BTreeMap::new();
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                all.extend(files(&entry.path()));
            } else if entry.file_name() != "root.json" {
                all.insert(entry.path(), fs::read(entry.path()).unwrap());
            }
        }
        all
    }
    let before = files(&path);
    fs::write(path.join("root.json"), &new).unwrap();
    assert!(crate::AccountRoot::open(&path, pool.clone()).is_err());
    assert_eq!(fs::read(path.join("root.json")).unwrap(), new);
    assert_eq!(files(&path), before);
    assert!(!path.join("files").exists());
    fs::write(path.join("root.json"), &old).unwrap();
    crate::AccountRoot::open(&path, pool).unwrap();
    assert_eq!(fs::read(path.join("root.json")).unwrap(), old);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn generated_sorted_subsets_have_unique_databases_and_exact_canonical_roundtrip(keys in prop::collection::btree_set(0u16..1024,0..129), chosen in any::<u8>(), reset in 0u64..=i64::MAX as u64) {
        let private=keys.iter().map(|k|id(*k as usize)).collect::<Vec<_>>();let files=keys.iter().filter(|k|(**k as u8)%3==chosen%3).map(|k|FileRootBinding::new(&id(*k as usize),&id(2048+*k as usize)).unwrap()).collect::<Vec<_>>();let manifest=AccountFileRootManifest::new(private.clone(),files.clone(),reset).unwrap();let bytes=manifest.to_bytes().unwrap();prop_assert!(bytes.len()<=MAX_ACCOUNT_FILE_ROOT_MANIFEST_BYTES);let decoded=inspect_account_file_root_manifest_bytes(&bytes).unwrap();prop_assert_eq!(decoded.private_projects(),private.as_slice());prop_assert_eq!(decoded.file_stores(),files.as_slice());prop_assert_eq!(decoded.reset_at(),reset);prop_assert_eq!(decoded.to_bytes().unwrap(),bytes);
    }
    #[test]
    fn arbitrary_bounded_bytes_never_panic_and_accepted_input_is_exact_canonical(bytes in prop::collection::vec(any::<u8>(),0..32770)) {
        if let Ok(value)=inspect_account_file_root_manifest_bytes(&bytes) {prop_assert_eq!(value.to_bytes().unwrap(),bytes);prop_assert!(value.private_projects().len()<=128);prop_assert!(value.file_stores().len()<=128);}
    }
}
