//! Access-only row adapter; original typed grammar, policies and durable executor.
use super::{
    public_sessions::{UserScope, blocking_mapped},
    *,
};
use crate::row_api::Operation;

async fn operation(
    scope: UserScope,
    request: Request,
    kind: Operation,
    clock: Clock,
) -> ApiResult<Response> {
    let bytes = crate::http::body(request).await?;
    blocking_mapped(
        scope,
        move |root, scope| {
            // The shared blocking owner has rechecked current admission after the
            // body/root waits and before this decode or the trusted server clock.
            let (table, operation) = crate::row_api::decode_user(kind, &bytes)?;
            let result = root.public_user_table(
                scope.project(),
                &table,
                scope.access()?,
                (clock)()?,
                operation,
            )?;
            Ok(crate::row_api::user_response(&result)?)
        },
        super::user_data::failure,
    )
    .await
}
pub(super) async fn get(
    State(app): State<App>,
    Extension(scope): Extension<UserScope>,
    request: Request,
) -> ApiResult<Response> {
    operation(scope, request, Operation::Get, app.clock).await
}
pub(super) async fn page(
    State(app): State<App>,
    Extension(scope): Extension<UserScope>,
    request: Request,
) -> ApiResult<Response> {
    operation(scope, request, Operation::Page, app.clock).await
}
pub(super) async fn write(
    State(app): State<App>,
    Extension(scope): Extension<UserScope>,
    request: Request,
) -> ApiResult<Response> {
    operation(scope, request, Operation::Batch, app.clock).await
}
