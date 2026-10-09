use super::*;
use proptest::prelude::*;
use std::cell::Cell;

#[test]
fn password_stream_preserves_binary_nul_newline_and_exact_maximum() {
    for bytes in [
        vec![0],
        vec![255, 0, 10, 13],
        "пароль 界\n".as_bytes().to_vec(),
        vec![42; MAX_PASSWORD_BYTES],
    ] {
        let secret = password(bytes.as_slice()).unwrap();
        assert_eq!(secret.as_slice(), bytes);
    }
    assert!(password([].as_slice()).is_err());
    assert!(password(vec![0; MAX_PASSWORD_BYTES + 1].as_slice()).is_err());
}

#[test]
fn password_stream_reads_limit_plus_one_and_hides_nested_io_errors() {
    struct Endless<'a>(&'a Cell<usize>);
    impl Read for Endless<'_> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            out.fill(42);
            self.0.set(self.0.get() + out.len());
            Ok(out.len())
        }
    }
    let count = Cell::new(0);
    assert!(password(Endless(&count)).is_err());
    assert_eq!(count.get(), MAX_PASSWORD_BYTES + 1);
    struct Failed;
    impl Read for Failed {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("synthetic-password-reader-secret"))
        }
    }
    assert_eq!(
        password(Failed).unwrap_err().to_string(),
        "password input unavailable"
    );
}

#[test]
fn metadata_is_exact_and_does_not_serialize_credentials_or_verifiers() {
    let user = User::from(AccountInfo {
        id: [255; 16],
        login: "synthetic_user".into(),
        credential_epoch: u64::MAX,
        disabled: true,
    });
    let json = serde_json::to_value(user).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"id":"ff".repeat(16),"login":"synthetic_user","credential_epoch":u64::MAX.to_string(),"disabled":true})
    );
    assert_eq!(json.as_object().unwrap().len(), 4);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn generated_password_streams_never_normalize_or_truncate(
        bytes in prop::collection::vec(any::<u8>(),0..=MAX_PASSWORD_BYTES+32)
    ) {
        let loaded=password(bytes.as_slice());
        if bytes.is_empty() || bytes.len()>MAX_PASSWORD_BYTES {
            prop_assert!(loaded.is_err());
        } else {
            let loaded=loaded.unwrap();
            prop_assert_eq!(loaded.as_slice(),bytes.as_slice());
        }
    }
}
