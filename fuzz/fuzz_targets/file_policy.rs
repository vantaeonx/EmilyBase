#![no_main]
use emilybase_auth::file_policy::{BoundPolicy, MAX_DOCUMENT_BYTES, StoreContext, decode};
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if let Ok(definition) = decode(data) {
        let _ = BoundPolicy::compile(
            StoreContext {
                project: "11111111111111111111111111111111",
                database: [3; 16],
            },
            &definition,
        );
        let normalized = serde_json::to_vec(&definition).unwrap();
        assert!(normalized.len() <= MAX_DOCUMENT_BYTES);
        assert_eq!(decode(&normalized).unwrap(), definition);
    }
    // Grammar/model only: no fabricated principal, filesystem or credential work.
});
