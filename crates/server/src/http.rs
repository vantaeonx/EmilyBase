use crate::projects::ProjectStatus;
use crate::rate::Rate;
use crate::{AuthorizedProject, CreatedProject, Error, ProjectInfo, ProjectStore};
use axum::extract::{ConnectInfo, FromRequestParts, MatchedPath, Path, Request, State};
use axum::http::{Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use emilybase_auth::KeyDigest;
use emilybase_catalog::Value;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(crate) const MAX_BODY: usize = 65_536;
const BODY_TIMEOUT: Duration = Duration::from_secs(5);
#[derive(Clone)]
struct App {
    store: Arc<tokio::sync::Mutex<ProjectStore>>,
    master: KeyDigest,
    workers: Arc<Semaphore>,
    rate: Arc<Mutex<Rate>>,
}
#[derive(Clone)]
struct Scope {
    project: Arc<Mutex<Option<AuthorizedProject>>>,
    _permit: Arc<OwnedSemaphorePermit>,
}
#[derive(Serialize)]
struct Problem {
    code: &'static str,
}
pub(crate) struct Failure(pub(crate) StatusCode, pub(crate) &'static str);
pub(crate) type ApiResult<T> = std::result::Result<T, Failure>;
impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (self.0, Json(Problem { code: self.1 })).into_response()
    }
}
impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        use emilybase_query::ExecutionError as Q;
        match error {
            Error::Tables(error) => table_failure(error),
            Error::Transfer(error) => transfer_failure(error),
            Error::Denied => Self(StatusCode::UNAUTHORIZED, "access_denied"),
            Error::Name | Error::Limit => Self(StatusCode::BAD_REQUEST, "invalid_project_request"),
            Error::Query(
                Q::Syntax(_)
                | Q::Column
                | Q::Binding(_)
                | Q::Type
                | Q::Limit(_)
                | Q::Control
                | Q::Assignment
                | Q::Catalog(_),
            ) => Self(StatusCode::BAD_REQUEST, "query_rejected"),
            Error::Query(Q::Transaction(
                emilybase_transactions::Error::Catalog(_)
                | emilybase_transactions::Error::Limit
                | emilybase_transactions::Error::Aborted,
            )) => Self(StatusCode::BAD_REQUEST, "query_rejected"),
            Error::Query(Q::Database(error))
            | Error::Query(Q::Transaction(emilybase_transactions::Error::Database(error))) => {
                use emilybase_database::Error as D;
                if matches!(
                    error,
                    D::Catalog(_)
                        | D::TableExists
                        | D::NoTable
                        | D::DuplicateKey
                        | D::NoRow
                        | D::PrimaryKeyChange
                        | D::Limit(_)
                ) {
                    Self(StatusCode::BAD_REQUEST, "query_rejected")
                } else {
                    Self(StatusCode::SERVICE_UNAVAILABLE, "storage_unavailable")
                }
            }
            Error::Query(Q::Transaction(_)) => Self(
                StatusCode::SERVICE_UNAVAILABLE,
                "transaction_outcome_requires_inspection",
            ),
            Error::PublicationUnknown(_) => Self(
                StatusCode::SERVICE_UNAVAILABLE,
                "publication_outcome_requires_inspection",
            ),
            _ => Self(StatusCode::SERVICE_UNAVAILABLE, "storage_unavailable"),
        }
    }
}
fn transfer_failure(error: emilybase_transfer::Error) -> Failure {
    use emilybase_transfer::Error as T;
    match error {
        T::Limit | T::Document | T::Version | T::Order | T::Existing | T::Catalog(_) => {
            Failure(StatusCode::BAD_REQUEST, "transfer_rejected")
        }
        T::Database(error) | T::Transactions(emilybase_transactions::Error::Database(error)) => {
            use emilybase_database::Error as D;
            if matches!(
                error,
                D::Catalog(_)
                    | D::TableExists
                    | D::NoTable
                    | D::DuplicateKey
                    | D::NoRow
                    | D::PrimaryKeyChange
                    | D::Limit(_)
            ) {
                Failure(StatusCode::BAD_REQUEST, "transfer_rejected")
            } else {
                Failure(StatusCode::SERVICE_UNAVAILABLE, "storage_unavailable")
            }
        }
        T::Transactions(
            emilybase_transactions::Error::Catalog(_)
            | emilybase_transactions::Error::Limit
            | emilybase_transactions::Error::Aborted,
        ) => Failure(StatusCode::BAD_REQUEST, "transfer_rejected"),
        T::Transactions(_) => Failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "transaction_outcome_requires_inspection",
        ),
        T::Io(_) => Failure(StatusCode::SERVICE_UNAVAILABLE, "storage_unavailable"),
    }
}
fn table_failure(error: crate::TableError) -> Failure {
    use crate::TableError as T;
    match error {
        T::Document | T::Limit | T::Catalog(_) => {
            Failure(StatusCode::BAD_REQUEST, "table_rejected")
        }
        T::Database(e) | T::Transaction(emilybase_transactions::Error::Database(e)) => {
            use emilybase_database::Error as D;
            if matches!(
                e,
                D::Catalog(_)
                    | D::TableExists
                    | D::NoTable
                    | D::DuplicateKey
                    | D::NoRow
                    | D::PrimaryKeyChange
                    | D::Limit(_)
            ) {
                Failure(StatusCode::BAD_REQUEST, "table_rejected")
            } else {
                Failure(StatusCode::SERVICE_UNAVAILABLE, "storage_unavailable")
            }
        }
        T::Transaction(
            emilybase_transactions::Error::Catalog(_)
            | emilybase_transactions::Error::Limit
            | emilybase_transactions::Error::Aborted,
        ) => Failure(StatusCode::BAD_REQUEST, "table_rejected"),
        T::Transaction(_) => Failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "transaction_outcome_requires_inspection",
        ),
    }
}

/// All filesystem/recovery/query work runs in bounded blocking tasks, outside reactor threads.
pub fn router(store: ProjectStore, master_token: &str) -> crate::Result<Router> {
    let app = App {
        store: Arc::new(tokio::sync::Mutex::new(store)),
        master: KeyDigest::from_token(master_token)?,
        workers: Arc::new(Semaphore::new(4)),
        rate: Arc::new(Mutex::new(Rate::default())),
    };
    Ok(routes(app))
}
fn routes(app: App) -> Router {
    let protected = Router::new()
        .route("/v1/projects", get(list).post(create))
        .route("/v1/projects/{id}/keys/rotate", post(rotate))
        .route("/v1/projects/{id}/sql", post(sql))
        .route("/v1/projects/{id}/explain", post(explain))
        .route("/v1/projects/{id}/status", get(status))
        .route("/v1/projects/{id}/tables/export", post(export_table))
        .route("/v1/projects/{id}/tables/import", post(import_table))
        .route("/v1/projects/{id}/tables", get(table_list))
        .route("/v1/projects/{id}/tables/schema", post(table_schema))
        .route("/v1/projects/{id}/tables/create", post(table_create))
        .route("/v1/projects/{id}/tables/drop", post(table_drop))
        .route("/v1/projects/{id}/tables/rows/get", post(row_get))
        .route("/v1/projects/{id}/tables/rows/page", post(row_page))
        .route("/v1/projects/{id}/tables/rows/insert", post(row_insert))
        .route("/v1/projects/{id}/tables/rows/update", post(row_update))
        .route("/v1/projects/{id}/tables/rows/delete", post(row_delete))
        .route("/v1/projects/{id}/tables/rows/batch", post(row_batch))
        .route_layer(middleware::from_fn_with_state(app.clone(), guard));
    Router::new()
        .route(
            "/health",
            get(|| async { Json(serde_json::json!({"status":"experimental"})) }),
        )
        .merge(protected)
        .with_state(app)
}

pub async fn serve(
    listener: tokio::net::TcpListener,
    app: Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> crate::Result<()> {
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
    .map_err(Error::Transport)
}

async fn guard(State(app): State<App>, request: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = method_label(request.method());
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_default();
    let mut response = match authorize(&app, request).await {
        Ok(request) => next.run(request).await,
        Err(error) => error.into_response(),
    };
    if route == "/v1/projects/{id}/tables" || route.starts_with("/v1/projects/{id}/tables/") {
        prevent_cache(&mut response);
    }
    // Static labels exclude extension-method text, IDs and query strings.
    // Never record headers, tokens, SQL or bodies.
    tracing::info!(method=%method,route=%route,status=response.status().as_u16(),elapsed_ms=started.elapsed().as_millis() as u64,"request");
    response
}
pub(crate) fn method_label(method: &Method) -> &'static str {
    match method.as_str() {
        "GET" => "GET",
        "POST" => "POST",
        "PUT" => "PUT",
        "PATCH" => "PATCH",
        "DELETE" => "DELETE",
        "HEAD" => "HEAD",
        "OPTIONS" => "OPTIONS",
        "CONNECT" => "CONNECT",
        "TRACE" => "TRACE",
        _ => "OTHER",
    }
}
async fn authorize(app: &App, request: Request) -> ApiResult<Request> {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|p| p.0.ip())
        .ok_or(Failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "peer_unavailable",
        ))?;
    if !app
        .rate
        .lock()
        .map_err(|_| Failure(StatusCode::SERVICE_UNAVAILABLE, "limiter_unavailable"))?
        .admits(peer, Instant::now())
    {
        return Err(Failure(StatusCode::TOO_MANY_REQUESTS, "rate_limit"));
    }
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(Failure(StatusCode::UNAUTHORIZED, "access_denied"))?;
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str())
        .unwrap_or_default();
    let admin = route == "/v1/projects" || route == "/v1/projects/{id}/keys/rotate";
    if admin && !app.master.verifies(token) {
        return Err(Failure(StatusCode::UNAUTHORIZED, "access_denied"));
    }
    let permit = Arc::new(
        app.workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| Failure(StatusCode::SERVICE_UNAVAILABLE, "workers_busy"))?,
    );
    let project = if admin {
        None
    } else {
        let (mut parts, body) = request.into_parts();
        let Path(id) = Path::<String>::from_request_parts(&mut parts, &())
            .await
            .map_err(|_| Failure(StatusCode::UNAUTHORIZED, "access_denied"))?;
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(Failure(StatusCode::UNAUTHORIZED, "access_denied"))?;
        let project = app.store.lock().await.authorize(&id, token)?;
        let mut request = Request::from_parts(parts, body);
        request.extensions_mut().insert(Scope {
            project: Arc::new(Mutex::new(Some(project))),
            _permit: permit,
        });
        return Ok(request);
    };
    let mut request = request;
    request.extensions_mut().insert(Scope {
        project: Arc::new(Mutex::new(project)),
        _permit: permit,
    });
    Ok(request)
}
pub(crate) async fn json<T: serde::de::DeserializeOwned>(request: Request) -> ApiResult<T> {
    let bytes = body(request).await?;
    serde_json::from_slice(&bytes).map_err(|_| Failure(StatusCode::BAD_REQUEST, "invalid_json"))
}
pub(crate) async fn body(request: Request) -> ApiResult<axum::body::Bytes> {
    if !request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(';').next() == Some("application/json"))
    {
        return Err(Failure(StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required"));
    }
    tokio::time::timeout(
        BODY_TIMEOUT,
        axum::body::to_bytes(request.into_body(), MAX_BODY),
    )
    .await
    .map_err(|_| Failure(StatusCode::REQUEST_TIMEOUT, "body_timeout"))?
    .map_err(|_| Failure(StatusCode::PAYLOAD_TOO_LARGE, "body_limit"))
}
pub(crate) fn prevent_cache(response: &mut Response) {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        header::PRAGMA,
        axum::http::HeaderValue::from_static("no-cache"),
    );
}
pub(crate) fn transfer_response(bytes: Vec<u8>) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], bytes).into_response()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExportTable {
    pub table: String,
}
#[derive(Serialize)]
pub(crate) struct ImportedTable {
    pub transfer: emilybase_transfer::Report,
    pub transaction: u64,
}
async fn export_table(Extension(scope): Extension<Scope>, request: Request) -> ApiResult<Response> {
    let input: ExportTable = json(request).await?;
    let project = take_project(&scope)?;
    let bytes = blocking(scope, move || project.export_table(&input.table, MAX_BODY)).await?;
    Ok(transfer_response(bytes))
}
async fn import_table(
    Extension(scope): Extension<Scope>,
    request: Request,
) -> ApiResult<Json<ImportedTable>> {
    let bytes = body(request).await?;
    let project = take_project(&scope)?;
    let report = blocking(scope, move || {
        let table = emilybase_transfer::decode_table(&bytes)?;
        project.import_table(table)
    })
    .await?;
    Ok(Json(report))
}
async fn row_get(Extension(scope): Extension<Scope>, request: Request) -> ApiResult<Response> {
    row_operation(scope, request, crate::row_api::Operation::Get).await
}
async fn row_page(Extension(scope): Extension<Scope>, request: Request) -> ApiResult<Response> {
    row_operation(scope, request, crate::row_api::Operation::Page).await
}
async fn row_insert(Extension(scope): Extension<Scope>, request: Request) -> ApiResult<Response> {
    row_operation(scope, request, crate::row_api::Operation::Insert).await
}
async fn row_update(Extension(scope): Extension<Scope>, request: Request) -> ApiResult<Response> {
    row_operation(scope, request, crate::row_api::Operation::Update).await
}
async fn row_delete(Extension(scope): Extension<Scope>, request: Request) -> ApiResult<Response> {
    row_operation(scope, request, crate::row_api::Operation::Delete).await
}
async fn row_batch(Extension(scope): Extension<Scope>, request: Request) -> ApiResult<Response> {
    row_operation(scope, request, crate::row_api::Operation::Batch).await
}
async fn row_operation(
    scope: Scope,
    request: Request,
    op: crate::row_api::Operation,
) -> ApiResult<Response> {
    let bytes = body(request).await?;
    let project = take_project(&scope)?;
    blocking(scope, move || {
        project.table_operation(|db| crate::row_api::run(db, op, &bytes))
    })
    .await
}
async fn table_list(Extension(scope): Extension<Scope>) -> ApiResult<Response> {
    let project = take_project(&scope)?;
    blocking(scope, move || {
        project.table_operation(|db| crate::table_api::response(&crate::table_api::list(db)?))
    })
    .await
}
async fn table_schema(Extension(scope): Extension<Scope>, request: Request) -> ApiResult<Response> {
    let bytes = body(request).await?;
    let project = take_project(&scope)?;
    blocking(scope, move || {
        let name = crate::table_api::name_request(&bytes)?;
        project.table_operation(|db| {
            crate::table_api::response(&crate::table_api::describe(db, &name)?)
        })
    })
    .await
}
async fn table_create(Extension(scope): Extension<Scope>, request: Request) -> ApiResult<Response> {
    let bytes = body(request).await?;
    let project = take_project(&scope)?;
    blocking(scope, move || {
        let schema = crate::table_api::schema_request(&bytes)?;
        project.table_operation(|db| {
            crate::table_api::response(&crate::table_api::create(db, schema)?)
        })
    })
    .await
}
async fn table_drop(Extension(scope): Extension<Scope>, request: Request) -> ApiResult<Response> {
    let bytes = body(request).await?;
    let project = take_project(&scope)?;
    blocking(scope, move || {
        let name = crate::table_api::name_request(&bytes)?;
        project.table_operation(|db| {
            crate::table_api::response(&crate::table_api::drop_table(db, &name)?)
        })
    })
    .await
}
async fn blocking<T: Send + 'static>(
    scope: Scope,
    work: impl FnOnce() -> crate::Result<T> + Send + 'static,
) -> ApiResult<T> {
    tokio::task::spawn_blocking(move || {
        let _scope = scope;
        work()
    })
    .await
    .map_err(|_| Failure(StatusCode::INTERNAL_SERVER_ERROR, "worker_failed"))?
    .map_err(Into::into)
}
fn take_project(scope: &Scope) -> ApiResult<AuthorizedProject> {
    scope
        .project
        .lock()
        .map_err(|_| Failure(StatusCode::INTERNAL_SERVER_ERROR, "request_scope"))?
        .take()
        .ok_or(Failure(StatusCode::UNAUTHORIZED, "access_denied"))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sql {
    pub(crate) sql: String,
    #[serde(default)]
    pub(crate) parameters: Vec<Value>,
}

async fn list(
    State(app): State<App>,
    Extension(scope): Extension<Scope>,
) -> ApiResult<Json<Vec<ProjectInfo>>> {
    let result = blocking(scope, move || app.store.blocking_lock().list()).await?;
    Ok(Json(result))
}
async fn create(
    State(app): State<App>,
    Extension(scope): Extension<Scope>,
    request: Request,
) -> ApiResult<(StatusCode, Json<CreatedProject>)> {
    let body: Create = json(request).await?;
    let result = blocking(scope, move || app.store.blocking_lock().create(&body.name)).await?;
    Ok((StatusCode::CREATED, Json(result)))
}
async fn rotate(
    State(app): State<App>,
    Extension(scope): Extension<Scope>,
    Path(id): Path<String>,
) -> ApiResult<Json<CreatedProject>> {
    let result = blocking(scope, move || app.store.blocking_lock().rotate(&id)).await?;
    Ok(Json(result))
}
async fn sql(
    Extension(scope): Extension<Scope>,
    request: Request,
) -> ApiResult<Json<emilybase_query::Report>> {
    let body: Sql = json(request).await?;
    let project = take_project(&scope)?;
    Ok(Json(
        blocking(scope, move || project.execute(&body.sql, &body.parameters)).await?,
    ))
}
async fn explain(
    Extension(scope): Extension<Scope>,
    request: Request,
) -> ApiResult<Json<emilybase_query::PlanDescription>> {
    let body: Sql = json(request).await?;
    let project = take_project(&scope)?;
    Ok(Json(
        blocking(scope, move || project.explain(&body.sql, &body.parameters)).await?,
    ))
}
async fn status(Extension(scope): Extension<Scope>) -> ApiResult<Json<ProjectStatus>> {
    let project = take_project(&scope)?;
    Ok(Json(blocking(scope, move || project.status()).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    #[tokio::test]
    async fn registry_wait_does_not_block_the_reactor() {
        let _serial = crate::durability::PROCESS_TESTS.lock().await;
        let temp = tempfile::tempdir().unwrap();
        let master = "0".repeat(64);
        let mut store = ProjectStore::open(temp.path().join("projects")).unwrap();
        let created = store.create("reactor").unwrap();
        let app = App {
            store: Arc::new(tokio::sync::Mutex::new(store)),
            master: KeyDigest::from_token(&master).unwrap(),
            workers: Arc::new(Semaphore::new(4)),
            rate: Arc::new(Mutex::new(Rate::default())),
        };
        let held_store = app.store.clone();
        let (ready, wait) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _lock = held_store.blocking_lock();
            ready.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(300));
        });
        wait.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut request = Request::builder()
            .uri(format!("/v1/projects/{}/status", created.project.id))
            .header(header::AUTHORIZATION, format!("Bearer {}", created.api_key))
            .body(axum::body::Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()));
        let router = routes(app);
        let started = Instant::now();
        let response = tokio::spawn(router.oneshot(request));
        tokio::time::sleep(Duration::from_millis(20)).await;
        let responsive = started.elapsed() < Duration::from_millis(150);
        assert_eq!(response.await.unwrap().unwrap().status(), StatusCode::OK);
        holder.join().unwrap();
        assert!(
            responsive,
            "waiting for registry ownership blocked the reactor"
        );
    }

    #[tokio::test]
    async fn cancellation_cannot_release_a_started_commit_permit_or_its_root_owner() {
        let _serial = crate::durability::PROCESS_TESTS.lock().await;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("projects");
        let mut store = ProjectStore::open(&root).unwrap();
        let created = store.create("cancelled response").unwrap();
        let project = store
            .authorize(&created.project.id, &created.api_key)
            .unwrap();
        let workers = Arc::new(Semaphore::new(4));
        let scope = Scope {
            project: Arc::new(Mutex::new(None)),
            _permit: Arc::new(workers.clone().try_acquire_owned().unwrap()),
        };
        let (started, begin) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let (finished, done) = tokio::sync::oneshot::channel();
        let request = tokio::spawn(async move {
            blocking(scope, move || {
                started.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(5)).unwrap();
                let report = project.execute(
                    "CREATE TABLE t(id INT PRIMARY KEY);INSERT INTO t VALUES (7)",
                    &[],
                )?;
                finished.send(report.transaction).unwrap();
                Ok(())
            })
            .await
        });
        begin.await.unwrap();
        request.abort();
        assert!(matches!(request.await, Err(error) if error.is_cancelled()));
        drop(store);
        assert_eq!(workers.available_permits(), 3);
        assert!(matches!(ProjectStore::open(&root), Err(Error::Busy)));
        release.send(()).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), done)
                .await
                .unwrap()
                .unwrap(),
            2
        );
        // The finished signal precedes closure drop by a few instructions.
        tokio::time::timeout(Duration::from_secs(5), async {
            while workers.available_permits() != 4 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let reopened = ProjectStore::open(root).unwrap();
        let status = reopened
            .authorize(&created.project.id, &created.api_key)
            .unwrap()
            .status()
            .unwrap();
        assert_eq!(status.transaction, 2);
        assert_eq!(status.rows, 1);
    }
}
