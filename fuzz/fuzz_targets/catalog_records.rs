#![no_main]
#![forbid(unsafe_code)]

use emilybase_catalog::{decode_row, decode_schema, encode_row, encode_schema};
use emilybase_database::Event;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if let Ok(schema) = decode_schema(bytes) {
        assert_eq!(
            decode_schema(&encode_schema(&schema).unwrap()).unwrap(),
            schema
        );
    }
    if let Ok(row) = decode_row(bytes) {
        assert_eq!(decode_row(&encode_row(&row).unwrap()).unwrap(), row);
    }
    if let Ok(event) = Event::decode(bytes) {
        assert_eq!(Event::decode(&event.encode().unwrap()).unwrap(), event);
    }
});
