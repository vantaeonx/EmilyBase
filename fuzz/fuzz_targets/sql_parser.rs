#![no_main]
#![forbid(unsafe_code)]
use emilybase_query::{MAX_SQL_BYTES, parse};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if bytes.len() > MAX_SQL_BYTES {
        return;
    }
    if let Ok(sql) = std::str::from_utf8(bytes) {
        let first = parse(sql);
        assert_eq!(first, parse(sql));
    }
});
