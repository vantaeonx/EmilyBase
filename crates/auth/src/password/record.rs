use super::{PASSWORD_ITERATIONS, PASSWORD_MEMORY_KIB, PASSWORD_PARALLELISM, PasswordError};

pub const PASSWORD_RECORD_BYTES: usize = 72;
const MAGIC: &[u8; 8] = b"EBPWD\0\0\0";
const RECORD_VERSION: u16 = 1;

/// Opaque fixed-policy password verifier. Encoding is an explicit storage export;
/// Debug redacts the entire record, and no Display/implicit serialization is supplied.
#[derive(Clone)]
pub struct PasswordDigest {
    pub(super) salt: [u8; 16],
    pub(super) hash: [u8; 32],
}
impl std::fmt::Debug for PasswordDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PasswordDigest(redacted)")
    }
}
impl PasswordDigest {
    pub(super) fn from_parts(salt: [u8; 16], hash: [u8; 32]) -> Self {
        Self { salt, hash }
    }

    pub fn encode(&self) -> [u8; PASSWORD_RECORD_BYTES] {
        let mut bytes = [0; PASSWORD_RECORD_BYTES];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..10].copy_from_slice(&RECORD_VERSION.to_le_bytes());
        bytes[10] = 2; // Argon2id algorithm tag.
        bytes[11] = 0x13;
        bytes[12..16].copy_from_slice(&PASSWORD_MEMORY_KIB.to_le_bytes());
        bytes[16..20].copy_from_slice(&PASSWORD_ITERATIONS.to_le_bytes());
        bytes[20..24].copy_from_slice(&PASSWORD_PARALLELISM.to_le_bytes());
        bytes[24..40].copy_from_slice(&self.salt);
        bytes[40..72].copy_from_slice(&self.hash);
        bytes
    }

    /// No allocation or hashing. Reject unknown versions/costs before a record
    /// can influence a workspace request; larger or trailing records never pass.
    pub fn decode(bytes: &[u8]) -> Result<Self, PasswordError> {
        if bytes.len() != PASSWORD_RECORD_BYTES || &bytes[..8] != MAGIC {
            return Err(PasswordError::Record);
        }
        let version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if version != RECORD_VERSION {
            return Err(PasswordError::Version(version));
        }
        if bytes[10..12] != [2, 0x13]
            || bytes[12..16] != PASSWORD_MEMORY_KIB.to_le_bytes()
            || bytes[16..20] != PASSWORD_ITERATIONS.to_le_bytes()
            || bytes[20..24] != PASSWORD_PARALLELISM.to_le_bytes()
        {
            return Err(PasswordError::Policy);
        }
        let mut salt = [0; 16];
        let mut hash = [0; 32];
        salt.copy_from_slice(&bytes[24..40]);
        hash.copy_from_slice(&bytes[40..72]);
        Ok(Self { salt, hash })
    }
}
