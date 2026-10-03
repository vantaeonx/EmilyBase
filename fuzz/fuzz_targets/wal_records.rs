#![no_main]
#![forbid(unsafe_code)]
use emilybase_wal::{encode_header, recover};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let _ = recover(bytes, None);
    if bytes.len() <= 20000 {
        if let Ok(header) = encode_header([7; 16]) {
            let mut framed = header.to_vec();
            framed.extend_from_slice(bytes);
            let _ = recover(&framed, Some([7; 16]));
        }
    }
});
