#![no_main]
#![forbid(unsafe_code)]

use emilybase_model_profile::{MAX_REPORT_BYTES, decode_report};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if bytes.len() > MAX_REPORT_BYTES {
        return;
    }
    if let Ok(report) = decode_report(bytes) {
        report.validate().unwrap();
        let canonical = serde_json::to_vec(&report).unwrap();
        assert!(canonical.len() <= MAX_REPORT_BYTES);
        assert_eq!(decode_report(&canonical).unwrap(), report);
    }
});
