//! Separate admitted user sessions. No project-service or administrator scope.
use super::*;

#[derive(Clone)]
struct UserScope {
    project: Arc<str>,
    access: Option<Arc<Zeroizing<String>>>,
    root: Arc<tokio::sync::Mutex<AccountRoot>>,
    _permit: Arc<OwnedSemaphorePermit>,
}
pub(super) fn routes(app: App) -> Router<App> {
    Router::new()
        .route("/v1/projects/{id}/user/sign-in", post(sign_in))
        .route("/v1/projects/{id}/user/refresh", post(refresh))
        .route("/v1/projects/{id}/user/logout", post(logout))
        .route("/v1/projects/{id}/user/me", post(me))
        .route_layer(middleware::from_fn_with_state(app, guard))
}
async fn guard(State(app): State<App>, request: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = method_label(request.method());
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_owned())
        .unwrap_or_default();
    let response = match authorize(&app, request, &route).await {
        Ok(request) => next.run(request).await,
        Err(error) => error.into_response(),
    };
    tracing::info!(method=%method,route=%route,status=response.status().as_u16(),elapsed_ms=started.elapsed().as_millis() as u64,"request");
    response
}
async fn authorize(app: &App, request: Request, route: &str) -> ApiResult<Request> {
    let denied = || Failure(StatusCode::UNAUTHORIZED, "access_denied");
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|peer| peer.0.ip())
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
    let mut headers = request.headers().get_all(header::AUTHORIZATION).iter();
    let first = headers.next();
    if headers.next().is_some() {
        return Err(denied());
    }
    let access = if route == "/v1/projects/{id}/user/me" {
        let token = first
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .filter(|token| token.len() == 102 && token.is_ascii())
            .ok_or_else(denied)?;
        Some(Arc::new(Zeroizing::new(token.to_owned())))
    } else {
        if first.is_some() {
            return Err(denied());
        }
        None
    };
    let permit = Arc::new(
        app.workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| Failure(StatusCode::SERVICE_UNAVAILABLE, "workers_busy"))?,
    );
    let (mut parts, body) = request.into_parts();
    let Path(project) = Path::<String>::from_request_parts(&mut parts, &())
        .await
        .map_err(|_| denied())?;
    app.root
        .lock()
        .await
        .admits_public_user(&project)
        .map_err(account_failure)?;
    // Only a known currently admitted private project may consume this bounded map.
    // The original native call repeats admission after body/root waits.
    if !app
        .projects
        .lock()
        .map_err(|_| Failure(StatusCode::SERVICE_UNAVAILABLE, "limiter_unavailable"))?
        .admits(&project, Instant::now())
    {
        return Err(Failure(StatusCode::TOO_MANY_REQUESTS, "account_rate_limit"));
    }
    parts.extensions.insert(UserScope {
        project: project.into(),
        access,
        root: app.root.clone(),
        _permit: permit,
    });
    Ok(Request::from_parts(parts, body))
}
async fn blocking<T: Send + 'static>(
    scope: UserScope,
    work: impl FnOnce(&mut AccountRoot, &UserScope) -> crate::Result<T> + Send + 'static,
) -> ApiResult<T> {
    tokio::task::spawn_blocking(move || {
        let owner = scope.root.clone();
        let mut root = owner.blocking_lock();
        // Refuse a flag closed during body/root waits before consulting the clock.
        // The native operation also checks actual selected filesystem identities.
        root.admits_public_user(&scope.project)?;
        work(&mut root, &scope)
    })
    .await
    .map_err(|_| Failure(StatusCode::INTERNAL_SERVER_ERROR, "worker_failed"))?
    .map_err(account_failure)
}
async fn sign_in(
    State(app): State<App>,
    Extension(scope): Extension<UserScope>,
    request: Request,
) -> ApiResult<Response> {
    let body: Login = private_json(request).await?;
    blocking(scope, move |root, scope| {
        session(root.public_sign_in(
            &scope.project,
            &body.login,
            body.password.0.as_bytes(),
            (app.clock)()?,
        )?)
    })
    .await
}
async fn refresh(
    State(app): State<App>,
    Extension(scope): Extension<UserScope>,
    request: Request,
) -> ApiResult<Response> {
    let body: Refresh = private_json(request).await?;
    blocking(scope, move |root, scope| {
        session(root.public_refresh_session(
            &scope.project,
            &body.refresh_token.0,
            (app.clock)()?,
        )?)
    })
    .await
}
async fn logout(
    State(app): State<App>,
    Extension(scope): Extension<UserScope>,
    request: Request,
) -> ApiResult<Response> {
    let body: Refresh = private_json(request).await?;
    blocking(scope, move |root, scope| {
        root.public_logout_session(&scope.project, &body.refresh_token.0, (app.clock)()?)?;
        response(&serde_json::json!({"logged_out":true}))
    })
    .await
}
async fn me(
    State(app): State<App>,
    Extension(scope): Extension<UserScope>,
    request: Request,
) -> ApiResult<Response> {
    let _: requests::Empty = private_json(request).await?;
    blocking(scope, move |root, scope| {
        let token = scope.access.as_ref().ok_or(Error::Denied)?;
        response(&user(root.public_user(
            &scope.project,
            token,
            (app.clock)()?,
        )?))
    })
    .await
}
