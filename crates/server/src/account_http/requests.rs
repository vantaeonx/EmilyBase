//! Pure bounded request grammar, never authentication, clock or database authority.
use super::{Login, PRIVATE_BODY, Refresh};
use serde::{Deserialize, de::DeserializeOwned};

#[derive(Debug, Clone, Copy)]
pub enum SessionRequest {
    SignIn,
    Refresh,
    Logout,
    Me,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid bounded session request")]
pub struct SessionRequestError;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Empty {}

pub(super) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, SessionRequestError> {
    // Serde structs can also deserialize positional arrays. The HTTP contract
    // accepts objects only, including an exact empty object for own metadata.
    if bytes.len() > PRIVATE_BODY
        || bytes
            .iter()
            .copied()
            .find(|b| !matches!(b, b' ' | b'\r' | b'\n' | b'\t'))
            != Some(b'{')
    {
        return Err(SessionRequestError);
    }
    serde_json::from_slice(bytes).map_err(|_| SessionRequestError)
}
/// Syntax/size only. A successful parse grants no session or project authority.
/// Secret fields use the same wiping owners as real HTTP parsing.
pub fn validate_session_request(
    kind: SessionRequest,
    bytes: &[u8],
) -> Result<(), SessionRequestError> {
    match kind {
        SessionRequest::SignIn => decode::<Login>(bytes).map(drop),
        SessionRequest::Refresh | SessionRequest::Logout => decode::<Refresh>(bytes).map(drop),
        SessionRequest::Me => decode::<Empty>(bytes).map(drop),
    }
}
