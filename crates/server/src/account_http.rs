//! Explicit fixed-root transport; service keys are never public user credentials.
use crate::http::{ApiResult, Failure, Sql, json, method_label};
use crate::{AccountRoot, Error, MAX_PROJECTS, rate::Rate};
use axum::extract::{ConnectInfo, FromRequestParts, MatchedPath, Path, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use emilybase_auth::{
    KeyDigest,
    accounts::{AccountInfo, IssuedSession},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zeroize::{Zeroize, Zeroizing};

const PRIVATE_BODY: usize = 4096;
const PRIVATE_ATTEMPTS: u32 = 30;
const WINDOW: Duration = Duration::from_secs(60);
type Clock = Arc<dyn Fn() -> crate::Result<u64> + Send + Sync>;
#[derive(Clone)]
struct App {
    root: Arc<tokio::sync::Mutex<AccountRoot>>,
    master: KeyDigest,
    workers: Arc<Semaphore>,
    peers: Arc<Mutex<Rate>>,
    projects: Arc<Mutex<PrivateRate>>,
    clock: Clock,
}
#[derive(Default)]
struct PrivateRate(BTreeMap<String, (Instant, u32)>);
impl PrivateRate {
    fn admits(&mut self, project: &str, now: Instant) -> bool {
        if !self.0.contains_key(project) && self.0.len() >= MAX_PROJECTS {
            return false;
        }
        let entry = self.0.entry(project.into()).or_insert((now, 0));
        if now.duration_since(entry.0) >= WINDOW {
            *entry = (now, 0);
        }
        if entry.1 >= PRIVATE_ATTEMPTS {
            return false;
        }
        entry.1 += 1;
        true
    }
}
struct Credentials {
    project: String,
    key: Zeroizing<String>,
}
#[derive(Clone)]
struct Scope {
    credentials: Option<Arc<Credentials>>,
    root: Arc<tokio::sync::Mutex<AccountRoot>>,
    _permit: Arc<OwnedSemaphorePermit>,
}
impl Scope {
    fn credentials(&self) -> crate::Result<(&str, &str)> {
        self.credentials
            .as_ref()
            .map(|c| (c.project.as_str(), c.key.as_str()))
            .ok_or(Error::Denied)
    }
}
fn system_time() -> crate::Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
        .filter(|now| *now <= i64::MAX as u64)
        .ok_or(Error::Config("trusted clock unavailable"))
}
/// Existing root only. No anonymous signup or user-token SQL authorization.
pub fn account_router(root: AccountRoot, master: &str) -> crate::Result<Router> {
    routes(root, master, Arc::new(system_time))
}
fn routes(root: AccountRoot, master: &str, clock: Clock) -> crate::Result<Router> {
    let app = make_app(root, master, clock)?;
    Ok(routes_app(app))
}
fn make_app(root: AccountRoot, master: &str, clock: Clock) -> crate::Result<App> {
    Ok(App {
        root: Arc::new(tokio::sync::Mutex::new(root)),
        master: KeyDigest::from_token(master)?,
        workers: Arc::new(Semaphore::new(4)),
        peers: Arc::new(Mutex::new(Rate::default())),
        projects: Arc::new(Mutex::new(PrivateRate::default())),
        clock,
    })
}
fn routes_app(app: App) -> Router {
    let protected = Router::new()
        .route("/v1/projects", get(list))
        .route("/v1/projects/{id}/keys/rotate", post(rotate))
        .route("/v1/projects/{id}/status", get(status))
        .route("/v1/projects/{id}/sql", post(sql))
        .route("/v1/projects/{id}/explain", post(explain))
        .route("/v1/projects/{id}/auth/users", post(create_user))
        .route("/v1/projects/{id}/auth/users/list", post(list_users))
        .route("/v1/projects/{id}/auth/sign-in", post(sign_in))
        .route("/v1/projects/{id}/auth/refresh", post(refresh))
        .route("/v1/projects/{id}/auth/logout", post(logout))
        .route("/v1/projects/{id}/auth/me", post(me))
        .route("/v1/projects/{id}/auth/password", post(change_password))
        .route("/v1/projects/{id}/auth/disabled", post(set_disabled))
        .route(
            "/v1/projects/{id}/auth/sessions/prune",
            post(prune_sessions),
        )
        .route_layer(middleware::from_fn_with_state(app.clone(), guard));
    Router::new()
        .route(
            "/health",
            get(|| async { Json(serde_json::json!({"status":"experimental"})) }),
        )
        .merge(protected)
        .with_state(app)
        .layer(middleware::from_fn(no_cache))
}
async fn no_cache(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        header::PRAGMA,
        axum::http::HeaderValue::from_static("no-cache"),
    );
    response
}
async fn guard(State(app): State<App>, request: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = method_label(request.method());
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_default();
    let response = match authorize(&app, request, &route).await {
        Ok(request) => next.run(request).await,
        Err(e) => e.into_response(),
    };
    tracing::info!(method=%method,route=%route,status=response.status().as_u16(),elapsed_ms=started.elapsed().as_millis() as u64,"request");
    response
}
async fn authorize(app: &App, request: Request, route: &str) -> ApiResult<Request> {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|p| p.0.ip())
        .ok_or(Failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "peer_unavailable",
        ))?;
    if !app
        .peers
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
    let (mut parts, body) = request.into_parts();
    let credentials = if admin {
        None
    } else {
        let Path(id) = Path::<String>::from_request_parts(&mut parts, &())
            .await
            .map_err(|_| Failure(StatusCode::UNAUTHORIZED, "access_denied"))?;
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(Failure(StatusCode::UNAUTHORIZED, "access_denied"))?;
        let private = route.contains("/auth/");
        app.root.lock().await.admits_project(&id, token, private)?;
        if private
            && !app
                .projects
                .lock()
                .map_err(|_| Failure(StatusCode::SERVICE_UNAVAILABLE, "limiter_unavailable"))?
                .admits(&id, Instant::now())
        {
            return Err(Failure(StatusCode::TOO_MANY_REQUESTS, "account_rate_limit"));
        }
        Some(Arc::new(Credentials {
            project: id,
            key: Zeroizing::new(token.into()),
        }))
    };
    parts.extensions.insert(Scope {
        credentials,
        root: app.root.clone(),
        _permit: permit,
    });
    Ok(Request::from_parts(parts, body))
}
fn account_failure(error: Error) -> Failure {
    use emilybase_auth::{
        accounts::Error as A, password::PasswordError as P, tokens::TokenError as T,
    };
    match error {
        Error::Accounts(A::Denied | A::Token(T::Format)) => {
            Failure(StatusCode::UNAUTHORIZED, "access_denied")
        }
        Error::Accounts(A::Login | A::Cleanup | A::Page | A::Password(P::Input)) => {
            Failure(StatusCode::BAD_REQUEST, "invalid_account_request")
        }
        Error::Accounts(A::Password(P::Busy)) => {
            Failure(StatusCode::SERVICE_UNAVAILABLE, "password_workers_busy")
        }
        Error::Accounts(A::Exists) => Failure(StatusCode::CONFLICT, "account_exists"),
        Error::Accounts(A::Capacity | A::SessionCapacity | A::Generation | A::Epoch) => {
            Failure(StatusCode::CONFLICT, "account_capacity")
        }
        Error::Accounts(A::Clock | A::ClockDisabled) => {
            Failure(StatusCode::SERVICE_UNAVAILABLE, "trusted_clock_unavailable")
        }
        Error::Accounts(A::Storage(_)) => Failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "session_outcome_requires_inspection",
        ),
        other => other.into(),
    }
}
async fn blocking<T: Send + 'static>(
    scope: Scope,
    work: impl FnOnce(&mut AccountRoot, &Scope) -> crate::Result<T> + Send + 'static,
) -> ApiResult<T> {
    tokio::task::spawn_blocking(move || {
        let owner = scope.root.clone();
        let mut root = owner.blocking_lock();
        work(&mut root, &scope)
    })
    .await
    .map_err(|_| Failure(StatusCode::INTERNAL_SERVER_ERROR, "worker_failed"))?
    .map_err(account_failure)
}
#[derive(Deserialize)]
#[serde(transparent)]
struct Secret(String);
impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    login: String,
    password: Secret,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Refresh {
    refresh_token: Secret,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Access {
    access_token: Secret,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordChange {
    login: String,
    current_password: Secret,
    replacement_password: Secret,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Disabled {
    login: String,
    disabled: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prune {
    limit: u16,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListUsers {
    limit: u16,
    after: Option<String>,
}
async fn private_json<T: serde::de::DeserializeOwned>(request: Request) -> ApiResult<T> {
    if !request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(';').next() == Some("application/json"))
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
    // Wipe our owned parsing copy and secret fields. Transport/serde internals
    // can hold other copies; this is not a whole-heap erasure guarantee.
    let bytes = Zeroizing::new(bytes.to_vec());
    serde_json::from_slice(&bytes).map_err(|_| Failure(StatusCode::BAD_REQUEST, "invalid_json"))
}
#[derive(Serialize)]
struct User {
    id: String,
    login: String,
    credential_epoch: String,
    disabled: bool,
}
fn user(info: AccountInfo) -> User {
    let id = info.id.iter().map(|b| format!("{b:02x}")).collect();
    User {
        id,
        login: info.login,
        credential_epoch: info.credential_epoch.to_string(),
        disabled: info.disabled,
    }
}
fn response<T: Serialize>(value: &T) -> crate::Result<Response> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Config("response encoding"))?;
    Ok(([(header::CONTENT_TYPE, "application/json")], bytes).into_response())
}
fn session(pair: IssuedSession) -> crate::Result<Response> {
    #[derive(Serialize)]
    struct Wire<'a> {
        access_token: &'a str,
        refresh_token: &'a str,
        token_type: &'static str,
        expires_at: String,
    }
    response(&Wire {
        access_token: pair.access.expose(),
        refresh_token: pair.refresh.expose(),
        token_type: "Bearer",
        expires_at: pair.metadata.access_until.to_string(),
    })
}
async fn create_user(Extension(scope): Extension<Scope>, request: Request) -> ApiResult<Response> {
    let body: Login = private_json(request).await?;
    blocking(scope, move |root, s| {
        let (id, key) = s.credentials()?;
        let info = root.create_user(id, key, &body.login, body.password.0.as_bytes())?;
        let mut out = response(&user(info))?;
        *out.status_mut() = StatusCode::CREATED;
        Ok(out)
    })
    .await
}
async fn list_users(Extension(scope): Extension<Scope>, request: Request) -> ApiResult<Response> {
    let body: ListUsers = private_json(request).await?;
    blocking(scope, move |root, s| {
        let (id, key) = s.credentials()?;
        let page = root.list_users(id, key, body.after.as_deref(), usize::from(body.limit))?;
        #[derive(Serialize)]
        struct Wire {
            users: Vec<User>,
            next_after: Option<String>,
        }
        response(&Wire {
            users: page.users.into_iter().map(user).collect(),
            next_after: page.next_after,
        })
    })
    .await
}
async fn change_password(
    Extension(scope): Extension<Scope>,
    request: Request,
) -> ApiResult<Response> {
    let body: PasswordChange = private_json(request).await?;
    blocking(scope, move |root, s| {
        let (id, key) = s.credentials()?;
        let info = root.change_password(
            id,
            key,
            &body.login,
            body.current_password.0.as_bytes(),
            body.replacement_password.0.as_bytes(),
        )?;
        response(&user(info))
    })
    .await
}
async fn set_disabled(Extension(scope): Extension<Scope>, request: Request) -> ApiResult<Response> {
    let body: Disabled = private_json(request).await?;
    blocking(scope, move |root, s| {
        let (id, key) = s.credentials()?;
        response(&user(root.set_disabled(
            id,
            key,
            &body.login,
            body.disabled,
        )?))
    })
    .await
}
async fn prune_sessions(
    State(app): State<App>,
    Extension(scope): Extension<Scope>,
    request: Request,
) -> ApiResult<Response> {
    let body: Prune = private_json(request).await?;
    if !(1..=emilybase_auth::accounts::MAX_SESSION_PRUNE).contains(&usize::from(body.limit)) {
        return Err(Failure(StatusCode::BAD_REQUEST, "invalid_account_request"));
    }
    blocking(scope, move |root, s| {
        let (id, key) = s.credentials()?;
        let removed =
            root.prune_session_families(id, key, (app.clock)()?, usize::from(body.limit))?;
        response(&serde_json::json!({"removed":removed}))
    })
    .await
}
async fn sign_in(
    State(app): State<App>,
    Extension(scope): Extension<Scope>,
    request: Request,
) -> ApiResult<Response> {
    let body: Login = private_json(request).await?;
    blocking(scope, move |root, s| {
        let (id, key) = s.credentials()?;
        session(root.sign_in(
            id,
            key,
            &body.login,
            body.password.0.as_bytes(),
            (app.clock)()?,
        )?)
    })
    .await
}
async fn refresh(
    State(app): State<App>,
    Extension(scope): Extension<Scope>,
    request: Request,
) -> ApiResult<Response> {
    let body: Refresh = private_json(request).await?;
    blocking(scope, move |root, s| {
        let (id, key) = s.credentials()?;
        session(root.refresh_session(id, key, &body.refresh_token.0, (app.clock)()?)?)
    })
    .await
}
async fn logout(
    State(app): State<App>,
    Extension(scope): Extension<Scope>,
    request: Request,
) -> ApiResult<Response> {
    let body: Refresh = private_json(request).await?;
    blocking(scope, move |root, s| {
        let (id, key) = s.credentials()?;
        root.logout_session(id, key, &body.refresh_token.0, (app.clock)()?)?;
        response(&serde_json::json!({"logged_out":true}))
    })
    .await
}
async fn me(
    State(app): State<App>,
    Extension(scope): Extension<Scope>,
    request: Request,
) -> ApiResult<Response> {
    let body: Access = private_json(request).await?;
    blocking(scope, move |root, s| {
        let (id, key) = s.credentials()?;
        let info = root.with_access(id, key, &body.access_token.0, (app.clock)()?, |p| {
            p.account().clone()
        })?;
        response(&user(info))
    })
    .await
}
async fn list(Extension(scope): Extension<Scope>) -> ApiResult<Response> {
    blocking(scope, |root, _| response(&root.projects()?)).await
}
async fn rotate(Extension(scope): Extension<Scope>, Path(id): Path<String>) -> ApiResult<Response> {
    blocking(scope, move |root, _| {
        response(&root.rotate_project_key(&id)?)
    })
    .await
}
async fn status(Extension(scope): Extension<Scope>) -> ApiResult<Response> {
    blocking(scope, |root, s| {
        let (id, key) = s.credentials()?;
        response(&root.status(id, key)?)
    })
    .await
}
async fn sql(Extension(scope): Extension<Scope>, request: Request) -> ApiResult<Response> {
    let body: Sql = json(request).await?;
    blocking(scope, move |root, s| {
        let (id, key) = s.credentials()?;
        response(&root.execute(id, key, &body.sql, &body.parameters)?)
    })
    .await
}
async fn explain(Extension(scope): Extension<Scope>, request: Request) -> ApiResult<Response> {
    let body: Sql = json(request).await?;
    blocking(scope, move |root, s| {
        let (id, key) = s.credentials()?;
        response(&root.explain(id, key, &body.sql, &body.parameters)?)
    })
    .await
}

#[cfg(test)]
mod tests;
