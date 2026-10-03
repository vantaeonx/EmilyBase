use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use emilybase_server::{CreatedProject, ProjectStore, router, serve};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;

const MASTER: &str = "0000000000000000000000000000000000000000000000000000000000000000";
struct Fixture {
    directory: tempfile::TempDir,
    app: Router,
    first: CreatedProject,
    second: CreatedProject,
}
fn fixture() -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let mut store = ProjectStore::open(directory.path().join("projects")).unwrap();
    let first = store.create("Первый проект").unwrap();
    let second = store.create("second").unwrap();
    let app = router(store, MASTER).unwrap();
    Fixture {
        directory,
        app,
        first,
        second,
    }
}
fn request(method: &str, path: &str, key: &str, body: Body) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {key}"))
        .header("content-type", "application/json")
        .body(body)
        .unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo("127.0.0.1:4123".parse::<SocketAddr>().unwrap()));
    request
}
async fn call(
    app: &Router,
    method: &str,
    path: &str,
    key: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(request(method, path, key, Body::from(body.to_string())))
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 32 * 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
async fn sql(
    app: &Router,
    project: &CreatedProject,
    source: &str,
    parameters: Value,
) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        &format!("/v1/projects/{}/sql", project.project.id),
        &project.api_key,
        json!({"sql":source,"parameters":parameters}),
    )
    .await
}
#[tokio::test]
async fn administrator_scope_creates_lists_and_rotates_without_exposing_secrets() {
    let f = fixture();
    let (status, _) = call(
        &f.app,
        "POST",
        "/v1/projects",
        &f.first.api_key,
        json!({"name":"denied"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, created) = call(
        &f.app,
        "POST",
        "/v1/projects",
        MASTER,
        json!({"name":"HTTP project"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["project"]["id"].as_str().unwrap();
    let key = created["api_key"].as_str().unwrap();
    assert_eq!(key.len(), 64);
    let (status, listed) = call(&f.app, "GET", "/v1/projects", MASTER, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().unwrap().len(), 3);
    assert!(!listed.to_string().contains(key));
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v.get("api_key").is_none() && v.get("key").is_none())
    );
    let path = format!("/v1/projects/{id}/keys/rotate");
    assert_eq!(
        call(&f.app, "POST", &path, key, Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, rotated) = call(&f.app, "POST", &path, MASTER, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rotated["project"]["key_epoch"], 2);
    let new_key = rotated["api_key"].as_str().unwrap();
    assert_ne!(key, new_key);
    let path = format!("/v1/projects/{id}/status");
    assert_eq!(
        call(&f.app, "GET", &path, key, Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&f.app, "GET", &path, new_key, Value::Null).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(&f.app, "GET", &path, MASTER, Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn project_scopes_atomic_sql_parameters_and_restart_are_real() {
    let f = fixture();
    let schema =
        "CREATE TABLE notes(id INTEGER PRIMARY KEY, text TEXT); INSERT INTO notes VALUES (1,$1);";
    let injection = "literal'); DROP TABLE notes; --";
    assert_eq!(
        sql(
            &f.app,
            &f.first,
            schema,
            json!([{"type":"text","value":injection}])
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        sql(
            &f.app,
            &f.second,
            schema,
            json!([{"type":"text","value":"second"}])
        )
        .await
        .0,
        StatusCode::OK
    );
    let path = format!("/v1/projects/{}/sql", f.second.project.id);
    let (status, denied) = call(
        &f.app,
        "POST",
        &path,
        &f.first.api_key,
        json!({"sql":"SELECT * FROM notes"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(denied, json!({"code":"access_denied"}));
    for id in [
        "..",
        "..%2Foutside",
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF",
        "1",
    ] {
        assert_eq!(
            call(
                &f.app,
                "GET",
                &format!("/v1/projects/{id}/status"),
                &f.first.api_key,
                Value::Null
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (_, first) = sql(
        &f.app,
        &f.first,
        "SELECT * FROM notes WHERE id=$1",
        json!([{"type":"integer","value":1}]),
    )
    .await;
    assert_eq!(
        first["results"][0]["rows"][0][1],
        json!({"type":"text","value":injection})
    );
    assert_eq!(first["committed"], true);
    let transaction = first["transaction"].as_u64().unwrap();
    assert_eq!(
        sql(
            &f.app,
            &f.first,
            "INSERT INTO notes VALUES (2,'staged'); UPDATE notes SET text=3 WHERE id=1",
            json!([])
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, rows) = sql(
        &f.app,
        &f.first,
        "SELECT * FROM notes ORDER BY id",
        json!([]),
    )
    .await;
    assert_eq!(rows["results"][0]["rows"].as_array().unwrap().len(), 1);
    assert_eq!(rows["transaction"], transaction);
    let path = format!("/v1/projects/{}/explain", f.first.project.id);
    let (status, plan) = call(&f.app,"POST",&path,&f.first.api_key,json!({"sql":"SELECT * FROM notes WHERE id=$1","parameters":[{"type":"integer","value":1}]})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(plan["access"], "primary_key");
    let root = f.directory.path().join("projects");
    drop(f.app);
    let app = router(ProjectStore::open(root).unwrap(), MASTER).unwrap();
    let (_, rows) = sql(&app, &f.first, "SELECT * FROM notes", json!([])).await;
    assert_eq!(rows["results"][0]["rows"][0][1]["value"], injection);
    let (_, rows) = sql(&app, &f.second, "SELECT * FROM notes", json!([])).await;
    assert_eq!(rows["results"][0]["rows"][0][1]["value"], "second");
}
#[tokio::test]
async fn rejected_inputs_are_bounded_generic_and_authorized_first() {
    let f = fixture();
    let path = format!("/v1/projects/{}/sql", f.first.project.id);
    let mut req = request(
        "POST",
        &path,
        "secret-invalid-value",
        Body::from("invalid json"),
    );
    req.headers_mut().remove("content-type");
    let response = f.app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
    assert_eq!(bytes, b"{\"code\":\"access_denied\"}".as_slice());
    let mut req = request("POST", &path, &f.first.api_key, Body::empty());
    req.headers_mut().remove("content-type");
    assert_eq!(
        f.app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    for bad in [
        json!({"sql":"SELECT 1","secret":"should never echo"}),
        json!({"sql":4}),
        json!({"sql":"SELECT 1","parameters":[{"type":"float","value":"NaN"}]}),
        json!({"sql":"SELECT 1","parameters":[{"type":"null","value":1}]}),
    ] {
        let (status, body) = call(&f.app, "POST", &path, &f.first.api_key, bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({"code":"invalid_json"}));
    }
    let response = f
        .app
        .clone()
        .oneshot(request(
            "POST",
            &path,
            &f.first.api_key,
            Body::from("x".repeat(65_537)),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        sql(&f.app, &f.first, &"x".repeat(16_385), json!([]))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (_, status) = call(
        &f.app,
        "GET",
        &format!("/v1/projects/{}/status", f.first.project.id),
        &f.first.api_key,
        Value::Null,
    )
    .await;
    assert_eq!(status, json!({"transaction":1,"tables":0,"rows":0}));
}
#[tokio::test]
async fn socket_rate_limit_ignores_forged_forwarding_headers() {
    let f = fixture();
    for count in 0..120 {
        let mut req = request("GET", "/v1/projects", "invalid", Body::empty());
        req.headers_mut().insert(
            "x-forwarded-for",
            format!("192.0.2.{count}").parse().unwrap(),
        );
        assert_eq!(
            f.app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        call(&f.app, "GET", "/v1/projects", MASTER, Value::Null)
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    let response = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut req = request("GET", "/v1/projects", MASTER, Body::empty());
    req.extensions_mut()
        .insert(ConnectInfo("127.0.0.2:4123".parse::<SocketAddr>().unwrap()));
    assert_eq!(
        f.app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::OK
    );
}
#[tokio::test]
async fn four_slow_bodies_bound_workers_and_cancellation_releases_admission() {
    let f = fixture();
    let mut tasks = Vec::new();
    for _ in 0..4 {
        let app = f.app.clone();
        let body =
            Body::from_stream(futures_util::stream::pending::<Result<Bytes, std::io::Error>>());
        let req = request("POST", "/v1/projects", MASTER, body);
        tasks.push(tokio::spawn(app.oneshot(req)));
    }
    // Wait for scheduling, not for a body or disk operation.
    tokio::time::sleep(Duration::from_millis(30)).await;
    let (status, problem) = call(
        &f.app,
        "POST",
        "/v1/projects",
        MASTER,
        json!({"name":"must not write"}),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(problem, json!({"code":"workers_busy"}));
    for task in &tasks {
        task.abort();
    }
    for task in tasks {
        assert!(task.await.unwrap_err().is_cancelled());
    }
    let (_, listed) = call(&f.app, "GET", "/v1/projects", MASTER, Value::Null).await;
    assert_eq!(listed.as_array().unwrap().len(), 2);
    let body = Body::from_stream(futures_util::stream::pending::<Result<Bytes, std::io::Error>>());
    let response = tokio::time::timeout(
        Duration::from_secs(7),
        f.app
            .clone()
            .oneshot(request("POST", "/v1/projects", MASTER, body)),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert_eq!(
        call(&f.app, "GET", "/v1/projects", MASTER, Value::Null)
            .await
            .0,
        StatusCode::OK
    );
}
async fn tcp(address: SocketAddr, request: &str) -> String {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        socket.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        socket
            .take(1024 * 1024)
            .read_to_end(&mut response)
            .await
            .unwrap();
        String::from_utf8(response).unwrap()
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn actual_tcp_transport_and_graceful_shutdown_preserve_ownership_and_commits() {
    let f = fixture();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let root = f.directory.path().join("projects");
    let task = tokio::spawn(serve(listener, f.app, async {
        let _ = stopped.await;
    }));
    let health = tcp(
        address,
        "GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(health.starts_with("HTTP/1.1 200"));
    assert!(health.contains("experimental"));
    let body =
        json!({"sql":"CREATE TABLE tcp(id INTEGER PRIMARY KEY); INSERT INTO tcp VALUES (1)"})
            .to_string();
    let request = format!(
        "POST /v1/projects/{}/sql HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        f.first.project.id,
        f.first.api_key,
        body.len(),
        body
    );
    let response = tcp(address, &request).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("\"committed\":true"));
    assert!(matches!(
        ProjectStore::open(&root),
        Err(emilybase_server::Error::Busy)
    ));
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let store = ProjectStore::open(root).unwrap();
    let recovered = store
        .authorize(&f.first.project.id, &f.first.api_key)
        .unwrap()
        .execute("SELECT * FROM tcp", &[])
        .unwrap();
    assert_eq!(recovered.results[0].rows.len(), 1);
    assert_eq!(recovered.transaction, 2);
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
}
