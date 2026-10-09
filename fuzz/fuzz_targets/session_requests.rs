#![no_main]
use emilybase_server::{SessionRequest, validate_session_request};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some((&kind, bytes)) = data.split_first() {
        let kind = match kind % 5 {
            0 => SessionRequest::SignIn,
            1 => SessionRequest::Refresh,
            2 => SessionRequest::Logout,
            3 => SessionRequest::Me,
            _ => SessionRequest::Password,
        };
        if validate_session_request(kind, bytes).is_ok() {
            assert!(bytes.len() <= 4096);
            let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            let object = value.as_object().unwrap();
            if matches!(kind, SessionRequest::Me) {
                assert!(object.is_empty());
            }
            if matches!(kind, SessionRequest::Password) {
                assert_eq!(object.len(), 2);
                assert!(object["current_password"].is_string());
                assert!(object["replacement_password"].is_string());
            }
        }
    }
});
