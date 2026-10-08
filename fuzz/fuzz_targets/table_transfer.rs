#![no_main]
#![forbid(unsafe_code)]
use emilybase_transfer::{MAX_TRANSFER_BYTES, MAX_TRANSFER_ROWS, decode_table};
use libfuzzer_sys::fuzz_target;
fuzz_target!(|bytes: &[u8]| {
    if let Ok(table) = decode_table(bytes) {
        assert!(bytes.len() <= MAX_TRANSFER_BYTES);
        assert!(table.report().rows <= MAX_TRANSFER_ROWS);
        assert!(table.report().columns <= emilybase_catalog::MAX_COLUMNS);
        assert!(!format!("{table:?}").contains("schema"));
    }
});
