#![no_main]
use emilybase_server::{FileReadRequest, validate_file_read_request};
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if let Some((&tag, bytes)) = data.split_first() {
        let kind = match tag % 3 {
            0 => FileReadRequest::List,
            1 => FileReadRequest::Info,
            _ => FileReadRequest::Read,
        };
        if validate_file_read_request(kind, bytes).is_ok() {
            assert!(bytes.len() <= 4096);
            let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            let object = value.as_object().unwrap();
            if matches!(kind, FileReadRequest::List) {
                assert!(object.is_empty());
            } else {
                let id = object["id"].as_str().unwrap();
                assert_eq!(id.len(), 32);
                assert!(
                    id.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                );
                if matches!(kind, FileReadRequest::Read) {
                    assert!(object["maximum_bytes"].as_u64().unwrap() <= 8 * 1024 * 1024);
                }
            }
        }
    }
});
