//! Purpose/context-bound high-entropy token primitives, not a session registry.
mod record;
#[cfg(test)]
mod tests;

pub use record::{TOKEN_DIGEST_BYTES, TokenDigest};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const TOKEN_TEXT_BYTES: usize = 102;
const DOMAIN: &[u8] = b"EmilyBaseSessionToken-v1\0";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TokenError {
    #[error("invalid canonical token scope")]
    Scope,
    #[error("invalid session token format")]
    Format,
    #[error("invalid token digest record")]
    Record,
    #[error("unsupported token digest version {0}")]
    Version(u16),
    #[error("operating-system randomness is unavailable")]
    Randomness,
    #[error("token output allocation failed")]
    Allocation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Access,
    Refresh,
}
impl TokenKind {
    fn tag(self) -> u8 {
        match self {
            Self::Access => 1,
            Self::Refresh => 2,
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            Self::Access => "eba1_",
            Self::Refresh => "ebr1_",
        }
    }
}

/// Public scope metadata. Persist/rotate incarnation under a future durable
/// session protocol; choosing this value does not grant project permissions.
#[derive(Clone, PartialEq, Eq)]
pub struct TokenScope {
    project: [u8; 16],
    incarnation: [u8; 16],
}
impl std::fmt::Debug for TokenScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TokenScope(redacted)")
    }
}
impl TokenScope {
    pub fn new(project: &str, incarnation: [u8; 16]) -> Result<Self, TokenError> {
        if !crate::valid_project_id(project) {
            return Err(TokenError::Scope);
        }
        Ok(Self {
            project: unhex(project.as_bytes()).map_err(|_| TokenError::Scope)?,
            incarnation,
        })
    }
}

/// A caller-owned plaintext credential. Debug is redacted; no Clone, Display
/// or implicit serialization exists. Explicit exposure transfers its handling
/// responsibility to the caller; it does not wipe copies made by that caller.
pub struct IssuedToken(Zeroizing<String>);
impl std::fmt::Debug for IssuedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IssuedToken(redacted)")
    }
}
impl IssuedToken {
    pub fn expose(&self) -> &str {
        self.0.as_str()
    }
}

/// Routing metadata parsed from untrusted text; never an authorization capability.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TokenMetadata {
    pub kind: TokenKind,
    pub family_id: [u8; 16],
}
impl std::fmt::Debug for TokenMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenMetadata")
            .field("kind", &self.kind)
            .field("family", &"redacted")
            .finish()
    }
}

struct Parts {
    metadata: TokenMetadata,
    secret: Zeroizing<[u8; 32]>,
}
impl Parts {
    fn parse(text: &str) -> Result<Self, TokenError> {
        let bytes = text.as_bytes();
        if bytes.len() != TOKEN_TEXT_BYTES || bytes[37] != b'.' {
            return Err(TokenError::Format);
        }
        let kind = match &bytes[..5] {
            b"eba1_" => TokenKind::Access,
            b"ebr1_" => TokenKind::Refresh,
            _ => return Err(TokenError::Format),
        };
        let mut secret = Zeroizing::new([0; 32]);
        decode_hex(&bytes[38..102], secret.as_mut_slice())?;
        Ok(Self {
            metadata: TokenMetadata {
                kind,
                family_id: unhex(&bytes[5..37])?,
            },
            secret,
        })
    }
}

pub fn metadata(text: &str) -> Result<TokenMetadata, TokenError> {
    Ok(Parts::parse(text)?.metadata)
}

/// Produce a fresh 256-bit OS-random secret. Family/scope metadata is selected by
/// the trusted issuer; this primitive does not allocate or persist session state.
pub fn issue(
    kind: TokenKind,
    scope: &TokenScope,
    family_id: [u8; 16],
) -> Result<(IssuedToken, TokenDigest), TokenError> {
    let mut secret = Zeroizing::new([0; 32]);
    getrandom::fill(secret.as_mut_slice()).map_err(|_| TokenError::Randomness)?;
    let token = encode(kind, &family_id, &secret)?;
    let digest = TokenDigest {
        kind,
        scope: scope.clone(),
        family_id,
        hash: hash(kind, scope, &family_id, &secret),
    };
    Ok((token, digest))
}

fn encode(
    kind: TokenKind,
    family: &[u8; 16],
    secret: &[u8; 32],
) -> Result<IssuedToken, TokenError> {
    let mut text = Zeroizing::new(String::new());
    text.try_reserve_exact(TOKEN_TEXT_BYTES)
        .map_err(|_| TokenError::Allocation)?;
    text.push_str(kind.prefix());
    append_hex(&mut text, family);
    text.push('.');
    append_hex(&mut text, secret);
    Ok(IssuedToken(text))
}

fn append_hex(output: &mut String, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 15)] as char);
    }
}
fn unhex<const N: usize>(bytes: &[u8]) -> Result<[u8; N], TokenError> {
    let mut result = [0; N];
    decode_hex(bytes, &mut result)?;
    Ok(result)
}
fn decode_hex(bytes: &[u8], result: &mut [u8]) -> Result<(), TokenError> {
    if result.len().checked_mul(2) != Some(bytes.len()) {
        return Err(TokenError::Format);
    }
    for (i, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
        let nibble = |b: u8| match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            _ => Err(TokenError::Format),
        };
        result[i] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Ok(())
}
fn hash(kind: TokenKind, scope: &TokenScope, family: &[u8; 16], secret: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    hasher.update([kind.tag()]);
    hasher.update(scope.project);
    hasher.update(scope.incarnation);
    hasher.update(family);
    hasher.update(secret);
    hasher.finalize().into()
}
