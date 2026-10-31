//! Complete bounded current-user file reads through the original native gateway.
use super::{
    public_sessions::{UserScope, blocking_mapped},
    *,
};
use emilybase_auth::{accounts::Error as A, file_policy::PolicyError};
use emilybase_files::{FileId, FileInfo};

#[derive(Clone, Copy, Debug)]
pub enum FileReadRequest {
    List,
    Info,
    Read,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid bounded file read request")]
pub struct FileReadRequestError;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    id: String,
    maximum_bytes: usize,
}
enum Command {
    List,
    Info(FileId),
    Read(FileId, usize),
}
fn decode(kind: FileReadRequest, bytes: &[u8]) -> Result<Command, FileReadRequestError> {
    let bad = |_| FileReadRequestError;
    match kind {
        FileReadRequest::List => requests::decode::<requests::Empty>(bytes)
            .map(|_| Command::List)
            .map_err(bad),
        FileReadRequest::Info => {
            let body: Identity = requests::decode(bytes).map_err(bad)?;
            Ok(Command::Info(
                body.id.parse().map_err(|_| FileReadRequestError)?,
            ))
        }
        FileReadRequest::Read => {
            let body: Read = requests::decode(bytes).map_err(bad)?;
            if body.maximum_bytes > crate::MAX_ACCOUNT_FILE_READ_BYTES {
                return Err(FileReadRequestError);
            }
            Ok(Command::Read(
                body.id.parse().map_err(|_| FileReadRequestError)?,
                body.maximum_bytes,
            ))
        }
    }
}
/// Pure grammar/size validation only, never a file, session or permission grant.
pub fn validate_file_read_request(
    kind: FileReadRequest,
    bytes: &[u8],
) -> Result<(), FileReadRequestError> {
    decode(kind, bytes).map(drop)
}
#[derive(Serialize)]
struct Metadata<'a> {
    id: String,
    name: &'a str,
    payload_bytes: usize,
    revision: String,
}
fn metadata(info: &FileInfo) -> Metadata<'_> {
    Metadata {
        id: info.id().to_string(),
        name: info.name(),
        payload_bytes: info.report().payload_bytes,
        revision: info.revision().to_string(),
    }
}
fn failure(error: Error) -> Failure {
    match error {
        Error::Denied
        | Error::Accounts(A::FilePolicySchema | A::FilePolicyDenied)
        | Error::Accounts(A::FilePolicy(PolicyError::Denied)) => {
            Failure(StatusCode::FORBIDDEN, "file_read_rejected")
        }
        Error::Limit => Failure(StatusCode::PAYLOAD_TOO_LARGE, "file_read_limit"),
        Error::Files(_) | Error::Accounts(A::Corrupt | A::FilePolicy(_)) => {
            Failure(StatusCode::SERVICE_UNAVAILABLE, "file_read_unavailable")
        }
        other => account_failure(other),
    }
}
async fn operation(
    scope: UserScope,
    request: Request,
    kind: FileReadRequest,
    clock: Clock,
) -> ApiResult<Response> {
    let mut content_types = request.headers().get_all(header::CONTENT_TYPE).iter();
    let content_type = content_types.next().and_then(|v| v.to_str().ok());
    if content_types.next().is_some()
        || !content_type.is_some_and(|v| v.split(';').next() == Some("application/json"))
    {
        return Err(Failure(StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required"));
    }
    let bytes = tokio::time::timeout(
        Duration::from_secs(5),
        axum::body::to_bytes(request.into_body(), PRIVATE_BODY),
    )
    .await
    .map_err(|_| Failure(StatusCode::REQUEST_TIMEOUT, "body_timeout"))?
    .map_err(|_| Failure(StatusCode::PAYLOAD_TOO_LARGE, "body_limit"))?;
    blocking_mapped(
        scope,
        move |root, scope| {
            // Current admission is checked again after body/root waits, before decode
            // and trusted time. Native methods then derive and consume current proof.
            let command = match decode(kind, &bytes) {
                Ok(command) => command,
                Err(_) => {
                    return Ok(
                        Failure(StatusCode::BAD_REQUEST, "file_request_rejected").into_response()
                    );
                }
            };
            let now = clock()?;
            let project = scope.project();
            let access = scope.access()?;
            match command {
                Command::List => {
                    let files = root.public_list_files(project, access, now)?;
                    let visible: Vec<_> = files.iter().map(metadata).collect();
                    response(&serde_json::json!({"files": visible}))
                }
                Command::Info(id) => {
                    let info = root.public_file_info(project, access, now, id)?;
                    response(&serde_json::json!({"file":info.as_ref().map(metadata)}))
                }
                Command::Read(id, maximum) => {
                    let read = root.public_read_file(project, access, now, id, maximum)?;
                    // Complete checked copy only: no partial payload, file handle,
                    // display-name header, physical ID or owner escapes the frame.
                    Ok((
                        [
                            (header::CONTENT_TYPE, "application/octet-stream"),
                            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                        ],
                        read.into_payload(),
                    )
                        .into_response())
                }
            }
        },
        failure,
    )
    .await
}
pub(super) async fn list(
    State(app): State<App>,
    Extension(scope): Extension<UserScope>,
    request: Request,
) -> ApiResult<Response> {
    operation(scope, request, FileReadRequest::List, app.clock).await
}
pub(super) async fn info(
    State(app): State<App>,
    Extension(scope): Extension<UserScope>,
    request: Request,
) -> ApiResult<Response> {
    operation(scope, request, FileReadRequest::Info, app.clock).await
}
pub(super) async fn read(
    State(app): State<App>,
    Extension(scope): Extension<UserScope>,
    request: Request,
) -> ApiResult<Response> {
    operation(scope, request, FileReadRequest::Read, app.clock).await
}
