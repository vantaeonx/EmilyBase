//! Trusted backend adapter: current service key plus a separate user access token.
use super::*;
use crate::{UserRowTransportError, UserRowsError, row_api::Operation};
use emilybase_auth::{accounts::Error as A, row_policy::PolicyError};
pub const USER_ACCESS_HEADER: &str = "x-emilybase-access";
fn access(request: &Request) -> ApiResult<Zeroizing<String>> {
    let mut values = request.headers().get_all(USER_ACCESS_HEADER).iter();
    let token = values
        .next()
        .and_then(|v| v.to_str().ok())
        .filter(|text| text.len() == emilybase_auth::tokens::TOKEN_TEXT_BYTES && text.is_ascii())
        .ok_or(Failure(StatusCode::UNAUTHORIZED, "access_denied"))?;
    if values.next().is_some() {
        return Err(Failure(StatusCode::UNAUTHORIZED, "access_denied"));
    }
    Ok(Zeroizing::new(token.to_owned()))
}
fn failure(error: Error) -> Failure {
    match error {
        Error::UserTransport(UserRowTransportError::Request)
        | Error::UserRows(UserRowsError::Input) => {
            Failure(StatusCode::BAD_REQUEST, "user_row_rejected")
        }
        Error::UserRows(UserRowsError::Rejected | UserRowsError::Policy(PolicyError::Denied))
        | Error::Accounts(A::PolicyDenied) => Failure(StatusCode::FORBIDDEN, "user_row_rejected"),
        Error::UserRows(UserRowsError::Policy(PolicyError::Scope)) => {
            Failure(StatusCode::SERVICE_UNAVAILABLE, "policy_binding_invalid")
        }
        Error::UserRows(UserRowsError::Policy(PolicyError::Row)) => {
            Failure(StatusCode::BAD_REQUEST, "user_row_rejected")
        }
        Error::Accounts(A::PolicySchema) => {
            Failure(StatusCode::CONFLICT, "policy_catalog_disabled")
        }
        Error::Accounts(A::Corrupt | A::Policy(_)) => {
            Failure(StatusCode::SERVICE_UNAVAILABLE, "policy_catalog_invalid")
        }
        Error::UserTransport(UserRowTransportError::Response)
        | Error::UserRows(UserRowsError::Storage(_) | UserRowsError::Database(_))
        | Error::Accounts(A::Storage(_)) => Failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "user_row_outcome_requires_inspection",
        ),
        other => account_failure(other),
    }
}
async fn operation(
    scope: Scope,
    request: Request,
    op: Operation,
    clock: Clock,
) -> ApiResult<Response> {
    let token = access(&request)?;
    let bytes = crate::http::body(request).await?;
    blocking_mapped(
        scope,
        move |root, s| {
            let (id, key) = s.credentials()?;
            // Recheck current service key/private roster after the body wait and
            // before decoding. Session/policy/time are verified inside the held root.
            root.admits_project(id, key, true)?;
            let (table, operation) = crate::row_api::decode_user(op, &bytes)?;
            let result = root.user_table(id, key, &table, &token, (clock)()?, operation)?;
            Ok(crate::row_api::user_response(&result)?)
        },
        failure,
    )
    .await
}
pub(super) async fn get(
    State(app): State<App>,
    Extension(scope): Extension<Scope>,
    request: Request,
) -> ApiResult<Response> {
    operation(scope, request, Operation::Get, app.clock).await
}
pub(super) async fn page(
    State(app): State<App>,
    Extension(scope): Extension<Scope>,
    request: Request,
) -> ApiResult<Response> {
    operation(scope, request, Operation::Page, app.clock).await
}
pub(super) async fn write(
    State(app): State<App>,
    Extension(scope): Extension<Scope>,
    request: Request,
) -> ApiResult<Response> {
    operation(scope, request, Operation::Batch, app.clock).await
}
