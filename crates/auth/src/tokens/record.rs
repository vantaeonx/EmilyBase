use super::{Parts, TokenError, TokenKind, TokenScope, hash};
use subtle::ConstantTimeEq;

pub const TOKEN_DIGEST_BYTES: usize = 92;
const MAGIC: &[u8; 8] = b"EBSK\0\0\0\0";

/// Private verifier record. Matching does not check user state, expiration,
/// refresh rotation or revocation; those require the future durable session layer.
#[derive(Clone)]
pub struct TokenDigest {
    pub(super) kind: TokenKind,
    pub(super) scope: TokenScope,
    pub(super) family_id: [u8; 16],
    pub(super) hash: [u8; 32],
}
impl std::fmt::Debug for TokenDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TokenDigest(redacted)")
    }
}
impl TokenDigest {
    pub fn encode(&self) -> [u8; TOKEN_DIGEST_BYTES] {
        let mut bytes = [0; TOKEN_DIGEST_BYTES];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..10].copy_from_slice(&1_u16.to_le_bytes());
        bytes[10] = self.kind.tag();
        bytes[12..28].copy_from_slice(&self.scope.project);
        bytes[28..44].copy_from_slice(&self.scope.incarnation);
        bytes[44..60].copy_from_slice(&self.family_id);
        bytes[60..92].copy_from_slice(&self.hash);
        bytes
    }
    /// Fixed policy/size; no heap, hashing or caller-controlled crypto costs.
    pub fn decode(bytes: &[u8]) -> Result<Self, TokenError> {
        if bytes.len() != TOKEN_DIGEST_BYTES || &bytes[..8] != MAGIC || bytes[11] != 0 {
            return Err(TokenError::Record);
        }
        let version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if version != 1 {
            return Err(TokenError::Version(version));
        }
        let kind = match bytes[10] {
            1 => TokenKind::Access,
            2 => TokenKind::Refresh,
            _ => return Err(TokenError::Record),
        };
        let mut project = [0; 16];
        let mut incarnation = [0; 16];
        let mut family_id = [0; 16];
        let mut hash = [0; 32];
        project.copy_from_slice(&bytes[12..28]);
        incarnation.copy_from_slice(&bytes[28..44]);
        family_id.copy_from_slice(&bytes[44..60]);
        hash.copy_from_slice(&bytes[60..92]);
        Ok(Self {
            kind,
            scope: TokenScope {
                project,
                incarnation,
            },
            family_id,
            hash,
        })
    }
    /// Check against independently selected current scope metadata. The complete
    /// fixed-size secret digest uses timing-safe comparison; full flow is not timed.
    pub fn matches(&self, text: &str, expected_scope: &TokenScope) -> Result<bool, TokenError> {
        let parsed = Parts::parse(text)?;
        if &self.scope != expected_scope
            || self.kind != parsed.metadata.kind
            || self.family_id != parsed.metadata.family_id
        {
            return Ok(false);
        }
        let actual = hash(self.kind, expected_scope, &self.family_id, &parsed.secret);
        Ok(bool::from(self.hash.ct_eq(&actual)))
    }
}
