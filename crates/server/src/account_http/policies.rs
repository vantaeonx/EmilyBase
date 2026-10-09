//! Service-key-only policy administration; end-user data routes remain closed.
use super::{
    ApiResult, Error, Extension, Failure, Request, Response, Scope, StatusCode, blocking_mapped,
};
use emilybase_auth::{
    accounts::{Error as AccountError, PolicyReceipt},
    row_policy,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum PolicyTransportError {
    #[error("invalid bounded policy request document")]
    Document,
    #[error("policy response outcome requires inspection")]
    Response,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Install {
    table: String,
    expected: String,
    document: String,
}
fn decode(bytes: &[u8]) -> Result<(Install, u64), PolicyTransportError> {
    if bytes.len() > crate::http::MAX_BODY {
        return Err(PolicyTransportError::Document);
    }
    let input: Install =
        serde_json::from_slice(bytes).map_err(|_| PolicyTransportError::Document)?;
    let expected = input
        .expected
        .parse::<u64>()
        .map_err(|_| PolicyTransportError::Document)?;
    if input.expected != expected.to_string()
        || input.table.is_empty()
        || input.table.len() > emilybase_catalog::MAX_NAME_BYTES
    {
        return Err(PolicyTransportError::Document);
    }
    row_policy::decode(input.document.as_bytes()).map_err(|_| PolicyTransportError::Document)?;
    Ok((input, expected))
}
/// Pure bounded grammar inspection; no table assertion, revision grant or storage.
pub fn validate_policy_install_request(bytes: &[u8]) -> Result<(), PolicyTransportError> {
    decode(bytes).map(|_| ())
}
#[derive(Serialize)]
struct Receipt {
    table: String,
    revision: String,
    previous: String,
    sha256: String,
}
impl From<PolicyReceipt> for Receipt {
    fn from(value: PolicyReceipt) -> Self {
        Self {
            table: value.table.to_string(),
            revision: value.revision.to_string(),
            previous: value.previous.to_string(),
            sha256: value.sha256.iter().map(|b| format!("{b:02x}")).collect(),
        }
    }
}
fn response(value: &impl Serialize) -> crate::Result<Response> {
    let bytes = serde_json::to_vec(value).map_err(|_| PolicyTransportError::Response)?;
    if bytes.len() > crate::http::MAX_BODY {
        return Err(PolicyTransportError::Response.into());
    }
    Ok(crate::http::transfer_response(bytes))
}
fn failure(error: Error) -> Failure {
    match error {
        Error::Policies(PolicyTransportError::Document)
        | Error::Accounts(AccountError::Policy(_)) => {
            Failure(StatusCode::BAD_REQUEST, "policy_rejected")
        }
        Error::Policies(PolicyTransportError::Response)
        | Error::Accounts(AccountError::Storage(_)) => Failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "policy_outcome_requires_inspection",
        ),
        Error::Accounts(AccountError::PolicySchema) => {
            Failure(StatusCode::CONFLICT, "policy_catalog_disabled")
        }
        Error::Accounts(AccountError::PolicyConflict) => {
            Failure(StatusCode::CONFLICT, "policy_revision_conflict")
        }
        Error::Accounts(AccountError::PolicyCapacity) => {
            Failure(StatusCode::CONFLICT, "policy_capacity")
        }
        Error::Accounts(AccountError::Corrupt) => {
            Failure(StatusCode::SERVICE_UNAVAILABLE, "policy_catalog_invalid")
        }
        other => other.into(),
    }
}
pub(super) async fn list(Extension(scope): Extension<Scope>) -> ApiResult<Response> {
    blocking_mapped(
        scope,
        |root, s| {
            let (id, key) = s.credentials()?;
            #[derive(Serialize)]
            struct Inventory {
                policies: Vec<Receipt>,
            }
            response(&Inventory {
                policies: root
                    .row_policy_receipts(id, key)?
                    .into_iter()
                    .map(Into::into)
                    .collect(),
            })
        },
        failure,
    )
    .await
}
pub(super) async fn enable(
    Extension(scope): Extension<Scope>,
    request: Request,
) -> ApiResult<Response> {
    let bytes = crate::http::body(request).await?;
    blocking_mapped(
        scope,
        move |root, s| {
            let (id, key) = s.credentials()?;
            root.admits_project(id, key, true)?;
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Empty {}
            let _: Empty =
                serde_json::from_slice(&bytes).map_err(|_| PolicyTransportError::Document)?;
            root.enable_row_policy_catalog(id, key)?;
            let version = root.private_schema_version(id, key)?;
            response(&serde_json::json!({"private_version":version}))
        },
        failure,
    )
    .await
}
pub(super) async fn install(
    Extension(scope): Extension<Scope>,
    request: Request,
) -> ApiResult<Response> {
    let bytes = crate::http::body(request).await?;
    blocking_mapped(
        scope,
        move |root, s| {
            let (id, key) = s.credentials()?;
            root.admits_project(id, key, true)?;
            let (input, expected) = decode(&bytes)?;
            let receipt = root.install_row_policy(
                id,
                key,
                &input.table,
                expected,
                input.document.as_bytes(),
            )?;
            #[derive(Serialize)]
            struct Installed {
                receipt: Receipt,
            }
            response(&Installed {
                receipt: receipt.into(),
            })
        },
        failure,
    )
    .await
}
#[cfg(test)]
mod tests;
