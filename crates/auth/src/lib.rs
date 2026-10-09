//! API keys, password verifiers and a separate private original-engine account store.
//! Native private sessions are integrated by the separately admitted server mode.
pub mod accounts;
pub mod key_file;
pub mod password;
pub mod row_policy;
pub mod tokens;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("operating-system randomness is unavailable")]
    Randomness,
    #[error("invalid API key format")]
    Format,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeyDigest([u8; 32]);
impl std::fmt::Debug for KeyDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KeyDigest(redacted)")
    }
}
impl KeyDigest {
    pub fn from_token(token: &str) -> Result<Self, Error> {
        if !is_hex(token, 64) {
            return Err(Error::Format);
        }
        Ok(Self(Sha256::digest(token.as_bytes()).into()))
    }
    pub fn verifies(&self, token: &str) -> bool {
        match Self::from_token(token) {
            Ok(other) => bool::from(self.0.ct_eq(&other.0)),
            Err(_) => false,
        }
    }
}

pub fn issue_key() -> Result<String, Error> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Randomness)?;
    Ok(hex(&bytes))
}
pub fn issue_project_id() -> Result<String, Error> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|_| Error::Randomness)?;
    Ok(hex(&bytes))
}
pub fn valid_project_id(id: &str) -> bool {
    is_hex(id, 32)
}
fn is_hex(text: &str, length: usize) -> bool {
    text.len() == length
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 15) as usize] as char);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn high_entropy_keys_and_ids_are_distinct_and_hashes_do_not_contain_tokens() {
        let first = issue_key().unwrap();
        let second = issue_key().unwrap();
        assert!(first != second);
        let digest = KeyDigest::from_token(&first).unwrap();
        assert!(digest.verifies(&first));
        assert!(!digest.verifies(&second));
        assert!(!serde_json::to_string(&digest).unwrap().contains(&first));
        assert!(!format!("{digest:?}").contains(&first));
        let copy: KeyDigest =
            serde_json::from_str(&serde_json::to_string(&digest).unwrap()).unwrap();
        assert!(copy.verifies(&first));
        let a = issue_project_id().unwrap();
        let b = issue_project_id().unwrap();
        assert!(valid_project_id(&a) && valid_project_id(&b) && a != b);
    }
    #[test]
    fn malformed_secrets_and_traversal_identifiers_fail_without_echo() {
        let digest = KeyDigest::from_token(&"0".repeat(64)).unwrap();
        for token in [
            String::new(),
            "0".repeat(63),
            "0".repeat(65),
            "G".repeat(64),
            "é".repeat(32),
            "synthetic-secret".into(),
        ] {
            assert!(!digest.verifies(&token));
            assert!(
                !KeyDigest::from_token(&token)
                    .unwrap_err()
                    .to_string()
                    .contains(&token)
                    || token.is_empty()
            );
        }
        for id in [
            "../outside",
            "/tmp/owned",
            ".",
            "..",
            "A0000000000000000000000000000000",
            "0000000000000000000000000000000/",
            "%2e%2e",
            "00000000000000000000000000000000\0",
        ] {
            assert!(!valid_project_id(id));
        }
    }
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        #[test]
        fn every_changed_token_byte_is_refused(position in 0usize..64) {
            let token="0".repeat(64);let digest=KeyDigest::from_token(&token).unwrap();
            let mut changed=token.into_bytes();changed[position]=b'1';
            prop_assert!(!digest.verifies(std::str::from_utf8(&changed).unwrap()));
        }
    }
}
