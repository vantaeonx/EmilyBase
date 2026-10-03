#![no_main]
#![forbid(unsafe_code)]
use emilybase_server::{MAX_METADATA_BYTES, inspect_project_metadata};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if bytes.len() < 32 || bytes.len() > 32 + MAX_METADATA_BYTES as usize {
        return;
    }
    let Ok(id) = std::str::from_utf8(&bytes[..32]) else {
        return;
    };
    let body = &bytes[32..];
    let _ = inspect_project_metadata(id, body);
    if let Ok(mut envelope) = serde_json::from_slice::<serde_json::Value>(body)
        && let Some(payload) = envelope.get("payload")
    {
        let mut ordered = Vec::new();
        ordered.push(b'{');
        for (i, field) in ["version", "id", "name", "key", "epoch"].iter().enumerate() {
            let Some(value) = payload.get(field) else {
                return;
            };
            if i > 0 {
                ordered.push(b',');
            }
            ordered.extend_from_slice(format!("\"{field}\":").as_bytes());
            ordered.extend_from_slice(&serde_json::to_vec(value).unwrap());
        }
        ordered.push(b'}');
        if let Some(object) = envelope.as_object_mut() {
            object.insert(
                "checksum".into(),
                serde_json::json!(crc32fast::hash(&ordered)),
            );
            let repaired = serde_json::to_vec(&envelope).unwrap();
            let _ = inspect_project_metadata(id, &repaired);
        }
    }
});
