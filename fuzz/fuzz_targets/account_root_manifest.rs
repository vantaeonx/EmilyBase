#![no_main]
#![forbid(unsafe_code)]
use emilybase_server::inspect_account_bundle_root_manifest_bytes;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let _ = inspect_account_bundle_root_manifest_bytes(bytes);
    if bytes.len() > 8192 {
        return;
    }
    // Repair only the envelope checksum to reach canonical inventory validation.
    let Ok(mut envelope) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return;
    };
    let Some(payload) = envelope.get("payload") else {
        return;
    };
    let Ok(typed) =
        serde_json::from_value::<emilybase_server::AccountBundleRootManifest>(payload.clone())
    else {
        return;
    };
    let payload = serde_json::to_vec(&typed).unwrap();
    envelope["checksum"] = crc32fast::hash(&payload).into();
    // Value maps sort keys, so explicitly use the writer's envelope field order.
    let repaired = format!(
        "{{\"payload\":{},\"checksum\":{}}}",
        std::str::from_utf8(&payload).unwrap(),
        envelope["checksum"]
    );
    if let Ok(report) = inspect_account_bundle_root_manifest_bytes(repaired.as_bytes()) {
        assert_eq!(report.version, 1);
        assert!(report.private_projects.len() <= 128);
        assert!(report.reset_at <= i64::MAX as u64);
        assert!(
            report
                .private_projects
                .iter()
                .all(|id| emilybase_auth::valid_project_id(id))
        );
        assert!(report.private_projects.windows(2).all(|p| p[0] < p[1]));
    }
});
