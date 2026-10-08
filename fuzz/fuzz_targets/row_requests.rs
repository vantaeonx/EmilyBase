#![no_main]
use emilybase_server::{RowOperation, validate_row_request};
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if let Some((&kind, bytes)) = data.split_first() {
        let operation = match kind % 5 {
            0 => RowOperation::Get,
            1 => RowOperation::Page,
            2 => RowOperation::Insert,
            3 => RowOperation::Update,
            _ => RowOperation::Delete,
        };
        let _ = validate_row_request(operation, bytes);
    }
});
