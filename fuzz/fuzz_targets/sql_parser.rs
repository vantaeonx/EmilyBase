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
        // The migration admission parser must remain bounded/panic-free as well.
        let _ = emilybase_migrations::prepare(1, "fuzz", sql);
        let version = bytes.first().copied().map_or(0, u32::from);
        let label = sql.get(..sql.len().min(64)).unwrap_or("");
        let _ = emilybase_migrations::prepare(version, label, sql);
    }
});
