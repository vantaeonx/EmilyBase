#![no_main]
use emilybase_server::{RowOperation, validate_row_request};
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if let Some((&kind, bytes)) = data.split_first() {
        if kind % 8 == 7 {
            let _ = emilybase_server::validate_policy_install_request(bytes);
            return;
        }
        if kind % 7 == 6 {
            let _ = emilybase_server::validate_migration_request(bytes);
            return;
        }
        let operation = match kind % 6 {
            0 => RowOperation::Get,
            1 => RowOperation::Page,
            2 => RowOperation::Insert,
            3 => RowOperation::Update,
            4 => RowOperation::Delete,
            _ => RowOperation::Batch,
        };
        let service = validate_row_request(operation, bytes);
        let user = emilybase_server::validate_user_row_request(operation, bytes);
        if service.is_ok() || user.is_ok() {
            assert!(bytes.len() <= 65_536);
            let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            assert!(value.is_object());
        }
    }
});
