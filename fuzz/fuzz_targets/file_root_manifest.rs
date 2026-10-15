#![no_main]
#![forbid(unsafe_code)]
use emilybase_server::{
    MAX_ACCOUNT_FILE_ROOT_MANIFEST_BYTES, inspect_account_file_root_manifest_bytes,
};
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

fn assert_canonical(bytes: &[u8]) {
    if let Ok(value) = inspect_account_file_root_manifest_bytes(bytes) {
        assert_eq!(value.to_bytes().unwrap(), bytes);
        assert!(value.reset_at() <= i64::MAX as u64);
        assert!(value.private_projects().len() <= 128);
        assert!(value.file_stores().len() <= 128);
        assert!(value.private_projects().windows(2).all(|p| p[0] < p[1]));
        assert!(
            value
                .file_stores()
                .windows(2)
                .all(|p| p[0].project() < p[1].project())
        );
        let mut databases = std::collections::BTreeSet::new();
        for store in value.file_stores() {
            assert!(
                value
                    .private_projects()
                    .iter()
                    .any(|p| p == store.project())
            );
            assert!(databases.insert(store.database_id()));
        }
    }
}
fuzz_target!(|bytes: &[u8]| {
    assert_canonical(bytes);
    if bytes.len() > MAX_ACCOUNT_FILE_ROOT_MANIFEST_BYTES {
        return;
    }
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return;
    };
    let Some(payload) = value.get("payload") else {
        return;
    };
    let stores = match payload["file_stores"].as_array() {
        None => payload["file_stores"].to_string(),
        Some(stores) => {
            let ordered = stores
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
                            text.push_str(&format!(
                                ",{}:{value}",
                                serde_json::to_string(key).unwrap()
                            ));
                        }
                    }
                    text.push('}');
                    text
                })
                .collect::<Vec<_>>();
            format!("[{}]", ordered.join(","))
        }
    };
    // Repair the CRC and canonical known-key order to reach semantic validation.
    let payload = format!(
        "{{\"version\":{},\"private_projects\":{},\"file_stores\":{},\"reset_at\":{}}}",
        payload["version"], payload["private_projects"], stores, payload["reset_at"]
    );
    let repaired = format!(
        "{{\"payload\":{payload},\"checksum\":{}}}",
        crc32fast::hash(payload.as_bytes())
    );
    assert_canonical(repaired.as_bytes());
});
