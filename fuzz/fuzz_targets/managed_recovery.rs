#![no_main]
#![forbid(unsafe_code)]

use emilybase_transactions::recover_snapshot;
use libfuzzer_sys::fuzz_target;
mod support;

fuzz_target!(|bytes: &[u8]| {
    let _ = recover_snapshot(bytes, None);
    if let Some(repaired) = support::repaired_wal(bytes) {
        let _ = recover_snapshot(&repaired, Some([7; 16]));
    }
});
