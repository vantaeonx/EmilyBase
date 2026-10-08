use super::*;
use crate::{ProjectStore, durability, restore_account_bundle_bytes};
use axum::body::{Body, Bytes, to_bytes};
use emilybase_auth::{accounts::AccountStore, password::PasswordPool};
use serde_json::{Value, json};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use tower::ServiceExt;

const MASTER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PASSWORD: &str = "synthetic-password";
struct Fixture {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    credentials: Vec<(String, String)>,
    app: App,
    clock: Arc<AtomicU64>,
}
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = ProjectStore::open(dir.path().join("registry")).unwrap();
    let pool = PasswordPool::new(1).unwrap();
    let mut accounts = Vec::new();
    let mut credentials = Vec::new();
    for i in 0..2 {
        let created = registry.create("synthetic project").unwrap();
        registry
            .authorize(&created.project.id, &created.api_key)
            .unwrap()
            .execute(
                "CREATE TABLE t(id INT PRIMARY KEY);INSERT INTO t VALUES(1)",
                &[],
            )
            .unwrap();
        let mut account = AccountStore::create(
            dir.path().join(format!("private-{i}")),
            &created.project.id,
            pool.clone(),
        )
        .unwrap();
        account
            .create_user("synthetic_user", PASSWORD.as_bytes())
            .unwrap();
        accounts.push(account);
        credentials.push((created.project.id, created.api_key));
    }
    let image = registry.capture_account_bundle(&mut accounts).unwrap();
    let path = dir.path().join("root");
    restore_account_bundle_bytes(&image, &path, pool.clone(), 50).unwrap();
    let clock = Arc::new(AtomicU64::new(50));
    let now = clock.clone();
    let app = make_app(
        AccountRoot::open(&path, pool).unwrap(),
        MASTER,
        Arc::new(move || Ok(now.load(Ordering::SeqCst))),
    )
    .unwrap();
    Fixture {
        _dir: dir,
        path,
        credentials,
        app,
        clock,
    }
}
fn request(path: &str, key: &str, body: Body) -> Request {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()));
    request
}
async fn send(router: &Router, request: Request) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()[header::PRAGMA], "no-cache");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, json)
}
async fn call(
    router: &Router,
    id: &str,
    key: &str,
    operation: &str,
    body: Value,
) -> (StatusCode, Value) {
    send(
        router,
        request(
            &format!("/v1/projects/{id}/auth/{operation}"),
            key,
            Body::from(body.to_string()),
        ),
    )
    .await
}
fn wal(f: &Fixture) -> Vec<Vec<u8>> {
    f.credentials
        .iter()
        .map(|(id, _)| fs::read(f.path.join("private").join(id).join("redo.wal")).unwrap())
        .collect()
}
async fn sign(router: &Router, id: &str, key: &str) -> Value {
    let (status, pair) = call(
        router,
        id,
        key,
        "sign-in",
        json!({"login":"synthetic_user","password":PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pair["token_type"], "Bearer");
    assert_eq!(pair["expires_at"], "950");
    pair
}

#[tokio::test]
async fn real_routes_provision_sign_in_rotate_refresh_verify_logout_and_keep_sql_separate() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let (status, created) = call(
        &router,
        id,
        key,
        "users",
        json!({"login":"new_user","password":"точный\u{0}пароль"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["credential_epoch"], "1");
    assert_eq!(created["id"].as_str().unwrap().len(), 32);
    assert_eq!(
        call(
            &router,
            id,
            key,
            "users",
            json!({"login":"new_user","password":PASSWORD})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, pair) = call(
        &router,
        id,
        key,
        "sign-in",
        json!({"login":"new_user","password":"точный\u{0}пароль"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(
            &router,
            id,
            key,
            "me",
            json!({"access_token":pair["access_token"]})
        )
        .await
        .1["login"],
        "new_user"
    );
    let (status, next) = call(
        &router,
        id,
        key,
        "refresh",
        json!({"refresh_token":pair["refresh_token"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(
            &router,
            id,
            key,
            "me",
            json!({"access_token":pair["access_token"]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &router,
            id,
            key,
            "refresh",
            json!({"refresh_token":pair["refresh_token"]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &router,
            id,
            key,
            "me",
            json!({"access_token":next["access_token"]})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &router,
            id,
            key,
            "logout",
            json!({"refresh_token":next["access_token"]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &router,
            id,
            key,
            "logout",
            json!({"refresh_token":next["refresh_token"]})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &router,
            id,
            key,
            "me",
            json!({"access_token":next["access_token"]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, sql) = send(
        &router,
        request(
            &format!("/v1/projects/{id}/sql"),
            key,
            Body::from(r#"{"sql":"SELECT * FROM t"}"#),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sql["results"][0]["rows"].as_array().unwrap().len(), 1);
    let mut list = request("/v1/projects", MASTER, Body::empty());
    *list.method_mut() = axum::http::Method::GET;
    assert_eq!(send(&router, list).await.1.as_array().unwrap().len(), 2);
    assert_eq!(
        send(&router, request("/v1/projects", MASTER, Body::empty()))
            .await
            .0,
        StatusCode::METHOD_NOT_ALLOWED
    );
    let pair = sign(&router, id, key).await;
    let (status, rotated) = send(
        &router,
        request(
            &format!("/v1/projects/{id}/keys/rotate"),
            MASTER,
            Body::empty(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(
            &router,
            id,
            key,
            "me",
            json!({"access_token":pair["access_token"]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &router,
            id,
            rotated["api_key"].as_str().unwrap(),
            "me",
            json!({"access_token":pair["access_token"]})
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn service_scope_and_body_admission_refuse_cross_project_master_and_user_sql_authority() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let (other, other_key) = &f.credentials[1];
    let pair = sign(&router, id, key).await;
    let before = wal(&f);
    for bad in [
        other_key.as_str(),
        MASTER,
        pair["access_token"].as_str().unwrap(),
        pair["refresh_token"].as_str().unwrap(),
        "synthetic-secret",
    ] {
        let (status, problem) = call(
            &router,
            id,
            bad,
            "sign-in",
            json!({"login":"synthetic_user","password":PASSWORD}),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(problem, json!({"code":"access_denied"}));
        assert_eq!(
            send(
                &router,
                request(
                    &format!("/v1/projects/{id}/sql"),
                    bad,
                    Body::from(r#"{"sql":"DELETE FROM t"}"#)
                )
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        call(
            &router,
            other,
            other_key,
            "me",
            json!({"access_token":pair["access_token"]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &router,
            request(
                &format!("/v1/projects/{id}/sql"),
                key,
                Body::from(r#"{"sql":"SELECT * FROM auth_users"}"#)
            )
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let polls = Arc::new(AtomicU64::new(0));
    let observed = polls.clone();
    let body = Body::from_stream(futures_util::stream::poll_fn(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        std::task::Poll::Ready(Some(Ok::<Bytes, std::io::Error>(Bytes::from_static(
            b"secret",
        ))))
    }));
    assert_eq!(
        send(
            &router,
            request(&format!("/v1/projects/{id}/auth/sign-in"), "invalid", body)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    assert_eq!(wal(&f), before);
}

#[tokio::test]
async fn private_json_is_bounded_redacted_and_rejects_unknown_client_time_and_duplicate_fields() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let path = format!("/v1/projects/{id}/auth/sign-in");
    let before = wal(&f);
    for body in [
        "{synthetic-private-input".to_owned(),
        r#"{"login":"synthetic_user","password":"synthetic-password","now":50}"#.into(),
        r#"{"login":"synthetic_user","login":"hidden","password":"synthetic-password"}"#.into(),
        json!({"login":"Bad/../login","password":PASSWORD}).to_string(),
        json!({"login":"synthetic_user","password":""}).to_string(),
        json!({"login":"synthetic_user","password":"x".repeat(1025)}).to_string(),
    ] {
        let (status, problem) = send(&router, request(&path, key, Body::from(body))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            problem == json!({"code":"invalid_json"})
                || problem == json!({"code":"invalid_account_request"})
        );
    }
    let mut req = request(&path, key, Body::from("synthetic-password"));
    req.headers_mut().insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/plain"),
    );
    assert_eq!(
        send(&router, req).await.0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(
        send(
            &router,
            request(&path, key, Body::from("x".repeat(PRIVATE_BODY + 1)))
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(wal(&f), before);
}

#[tokio::test]
async fn per_project_private_rate_precedes_kdf_and_other_projects_keep_independent_buckets() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let (other, other_key) = &f.credentials[1];
    let before = wal(&f);
    for _ in 0..PRIVATE_ATTEMPTS {
        assert_eq!(
            call(
                &router,
                id,
                key,
                "me",
                json!({"access_token":"synthetic-invalid"})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (status, problem) = call(
        &router,
        id,
        key,
        "sign-in",
        json!({"login":"synthetic_user","password":PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(problem, json!({"code":"account_rate_limit"}));
    assert_eq!(
        call(
            &router,
            other,
            other_key,
            "me",
            json!({"access_token":"synthetic-invalid"})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(wal(&f), before);
}

#[test]
fn private_rate_exact_window_and_known_bucket_capacity_do_not_grow_unbounded() {
    let mut rate = PrivateRate::default();
    let now = Instant::now();
    for i in 0..MAX_PROJECTS {
        assert!(rate.admits(&format!("{i:032x}"), now));
    }
    assert!(!rate.admits(&format!("{:032x}", MAX_PROJECTS), now));
    assert_eq!(rate.0.len(), MAX_PROJECTS);
    let id = format!("{:032x}", 0);
    for _ in 1..PRIVATE_ATTEMPTS {
        assert!(rate.admits(&id, now));
    }
    assert!(!rate.admits(&id, now + WINDOW - Duration::from_nanos(1)));
    assert!(rate.admits(&id, now + WINDOW));
}

#[tokio::test]
async fn four_slow_bodies_hold_admission_and_cancelled_unstarted_requests_release_it() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let path = format!("/v1/projects/{id}/auth/sign-in");
    let mut requests = Vec::new();
    for _ in 0..4 {
        requests.push(tokio::spawn(router.clone().oneshot(request(
            &path,
            key,
            Body::from_stream(futures_util::stream::pending::<Result<Bytes, std::io::Error>>()),
        ))));
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.app.workers.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        send(&router, request(&path, key, Body::empty())).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    for task in requests {
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.app.workers.available_permits() != 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn pending_private_body_times_out_without_kdf_or_wal_work() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let before = wal(&f);
    let body = Body::from_stream(futures_util::stream::pending::<Result<Bytes, std::io::Error>>());
    assert_eq!(
        send(
            &router,
            request(&format!("/v1/projects/{id}/auth/sign-in"), key, body)
        )
        .await
        .0,
        StatusCode::REQUEST_TIMEOUT
    );
    assert_eq!(wal(&f), before);
    assert_eq!(f.app.workers.available_permits(), 4);
}

#[tokio::test]
async fn trusted_time_rollback_fails_closed_and_body_time_cannot_choose_service_clock() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let pair = sign(&router, id, key).await;
    f.clock.store(100, Ordering::SeqCst);
    assert_eq!(
        call(
            &router,
            id,
            key,
            "sign-in",
            json!({"login":"synthetic_user","password":"synthetic-wrong"})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let before = wal(&f);
    f.clock.store(99, Ordering::SeqCst);
    assert_eq!(
        call(
            &router,
            id,
            key,
            "me",
            json!({"access_token":pair["access_token"]})
        )
        .await
        .1,
        json!({"code":"trusted_clock_unavailable"})
    );
    assert_eq!(
        call(
            &router,
            id,
            key,
            "me",
            json!({"access_token":pair["access_token"],"now":100})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(wal(&f), before);
}

#[tokio::test]
async fn waiting_for_root_does_not_block_health_or_the_async_reactor() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let held = f.app.root.clone();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _root = held.blocking_lock();
        started.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    ready.await.unwrap();
    let req = request(
        &format!("/v1/projects/{id}/auth/me"),
        key,
        Body::from(r#"{"access_token":"synthetic-invalid"}"#),
    );
    let pending = tokio::spawn(router.clone().oneshot(req));
    let response = tokio::time::timeout(
        Duration::from_millis(500),
        router.clone().oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!pending.is_finished());
    release.send(()).unwrap();
    holder.join().unwrap();
    assert_eq!(
        pending.await.unwrap().unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn cancelled_started_worker_keeps_permit_and_root_until_its_durable_work_finishes() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let (id, key) = f.credentials[0].clone();
    let workers = f.app.workers.clone();
    let scope = Scope {
        credentials: Some(Arc::new(Credentials {
            project: id.clone(),
            key: Zeroizing::new(key),
        })),
        root: f.app.root.clone(),
        _permit: Arc::new(workers.clone().try_acquire_owned().unwrap()),
    };
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let (done, finished) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(blocking(scope, move |root, s| {
        started.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        let (id, key) = s.credentials()?;
        root.create_user(id, key, "after_cancel", PASSWORD.as_bytes())?;
        done.send(()).unwrap();
        Ok(())
    }));
    ready.await.unwrap();
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    drop(f.app);
    assert_eq!(workers.available_permits(), 3);
    assert!(matches!(
        AccountRoot::open(&f.path, PasswordPool::new(1).unwrap()),
        Err(Error::Busy)
    ));
    release.send(()).unwrap();
    finished.await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while workers.available_permits() != 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let account = AccountStore::open(
        f.path.join("private").join(id),
        &f.credentials[0].0,
        PasswordPool::new(1).unwrap(),
    )
    .unwrap();
    assert_eq!(account.count().unwrap(), 2);
}

#[tokio::test]
async fn credential_management_revokes_all_old_families_and_preserves_other_project_and_noop_history()
 {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let (other, other_key) = &f.credentials[1];
    let first = sign(&router, id, key).await;
    let other_pair = sign(&router, other, other_key).await;
    let before = wal(&f);
    let (status, user) = call(
        &router,
        id,
        key,
        "disabled",
        json!({"login":"synthetic_user","disabled":false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["credential_epoch"], "1");
    assert_eq!(wal(&f), before);
    let replacement = "synthetic-new-界\0-password";
    let (status,_) = call(&router,id,key,"password",json!({"login":"synthetic_user","current_password":"synthetic-wrong","replacement_password":replacement})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(wal(&f), before);
    let (status,user) = call(&router,id,key,"password",json!({"login":"synthetic_user","current_password":PASSWORD,"replacement_password":replacement})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["credential_epoch"], "2");
    for (operation, field) in [("me", "access_token"), ("refresh", "refresh_token")] {
        assert_eq!(
            call(&router, id, key, operation, json!({field:first[field]}))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        call(
            &router,
            id,
            key,
            "sign-in",
            json!({"login":"synthetic_user","password":PASSWORD})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, fresh) = call(
        &router,
        id,
        key,
        "sign-in",
        json!({"login":"synthetic_user","password":replacement}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, user) = call(
        &router,
        id,
        key,
        "disabled",
        json!({"login":"synthetic_user","disabled":true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["credential_epoch"], "3");
    assert_eq!(user["disabled"], true);
    let disabled_wal = wal(&f);
    assert_eq!(
        call(
            &router,
            id,
            key,
            "disabled",
            json!({"login":"synthetic_user","disabled":true})
        )
        .await
        .1["credential_epoch"],
        "3"
    );
    assert_eq!(wal(&f), disabled_wal);
    for (operation, field) in [("me", "access_token"), ("refresh", "refresh_token")] {
        assert_eq!(
            call(&router, id, key, operation, json!({field:fresh[field]}))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        call(
            &router,
            id,
            key,
            "sign-in",
            json!({"login":"synthetic_user","password":replacement})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, user) = call(
        &router,
        id,
        key,
        "disabled",
        json!({"login":"synthetic_user","disabled":false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["credential_epoch"], "4");
    assert_eq!(
        call(
            &router,
            id,
            key,
            "me",
            json!({"access_token":fresh["access_token"]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &router,
            id,
            key,
            "sign-in",
            json!({"login":"synthetic_user","password":replacement})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &router,
            other,
            other_key,
            "me",
            json!({"access_token":other_pair["access_token"]})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(wal(&f)[1], before[1]);
    drop(router);
    drop(f.app);
    let mut root = AccountRoot::open(&f.path, PasswordPool::new(1).unwrap()).unwrap();
    assert!(
        root.sign_in(id, key, "synthetic_user", PASSWORD.as_bytes(), 50)
            .is_err()
    );
    assert!(
        root.sign_in(id, key, "synthetic_user", replacement.as_bytes(), 50)
            .is_ok()
    );
}

#[tokio::test]
async fn management_body_limits_unknown_fields_types_and_unauthorized_scopes_never_change_history()
{
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let before = wal(&f);
    for (operation, body) in [
        (
            "password",
            json!({"login":"synthetic_user","current_password":PASSWORD,"replacement_password":""}),
        ),
        (
            "password",
            json!({"login":"synthetic_user","current_password":PASSWORD,"replacement_password":"x".repeat(1025)}),
        ),
        (
            "password",
            json!({"login":"synthetic_user","current_password":PASSWORD,"replacement_password":"synthetic-new","now":50}),
        ),
        (
            "disabled",
            json!({"login":"synthetic_user","disabled":"true"}),
        ),
        (
            "disabled",
            json!({"login":"synthetic_user","disabled":true,"now":50}),
        ),
    ] {
        let (status, _) = call(&router, id, key, operation, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(wal(&f), before);
    }
    for operation in ["password", "disabled"] {
        let path = format!("/v1/projects/{id}/auth/{operation}");
        for forged in [MASTER, &f.credentials[1].1] {
            assert_eq!(
                send(
                    &router,
                    request(&path, forged, Body::from("malformed-private-input"))
                )
                .await
                .0,
                StatusCode::UNAUTHORIZED
            );
        }
        assert_eq!(
            send(
                &router,
                request(&path, key, Body::from("x".repeat(PRIVATE_BODY + 1)))
            )
            .await
            .0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(wal(&f), before);
    }
    let duplicate = r#"{"login":"synthetic_user","disabled":true,"disabled":false}"#;
    assert_eq!(
        send(
            &router,
            request(
                &format!("/v1/projects/{id}/auth/disabled"),
                key,
                Body::from(duplicate)
            )
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(wal(&f), before);
}

#[tokio::test]
async fn bounded_session_pruning_preserves_refreshable_families_and_other_project_history() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let (other_id, other_key) = &f.credentials[1];
    let revoked = sign(&router, id, key).await;
    let old_epoch = sign(&router, id, key).await;
    let other = sign(&router, other_id, other_key).await;
    assert_eq!(
        call(
            &router,
            id,
            key,
            "logout",
            json!({"refresh_token":revoked["refresh_token"]})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(call(&router, id, key, "password", json!({"login":"synthetic_user","current_password":PASSWORD,"replacement_password":PASSWORD})).await.0, StatusCode::OK);
    let live = sign(&router, id, key).await;
    let other_before = wal(&f)[1].clone();
    let public_before: Vec<_> = f
        .credentials
        .iter()
        .map(|(project, _)| {
            fs::read(f.path.join("registry").join(project).join("data/redo.wal")).unwrap()
        })
        .collect();
    // Access is expired, but refresh remains valid. Cleanup must preserve it.
    f.clock.store(950, Ordering::SeqCst);
    for _ in 0..2 {
        assert_eq!(
            call(&router, id, key, "sessions/prune", json!({"limit":1})).await,
            (StatusCode::OK, json!({"removed":1}))
        );
    }
    let before_noop = wal(&f);
    assert_eq!(
        call(&router, id, key, "sessions/prune", json!({"limit":128})).await,
        (StatusCode::OK, json!({"removed":0}))
    );
    assert_eq!(wal(&f), before_noop);
    for pair in [&revoked, &old_epoch] {
        for (operation, field) in [("me", "access_token"), ("refresh", "refresh_token")] {
            assert_eq!(
                call(&router, id, key, operation, json!({field:pair[field]}))
                    .await
                    .0,
                StatusCode::UNAUTHORIZED
            );
        }
    }
    assert_eq!(
        call(
            &router,
            id,
            key,
            "me",
            json!({"access_token":live["access_token"]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, replacement) = call(
        &router,
        id,
        key,
        "refresh",
        json!({"refresh_token":live["refresh_token"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(
            &router,
            id,
            key,
            "me",
            json!({"access_token":replacement["access_token"]})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(wal(&f)[1], other_before);
    // Each project has its own floor: a sibling's earlier valid time stays valid.
    f.clock.store(50, Ordering::SeqCst);
    assert_eq!(
        call(
            &router,
            other_id,
            other_key,
            "me",
            json!({"access_token":other["access_token"]})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(wal(&f)[1], other_before);
    f.clock.store(605750, Ordering::SeqCst);
    assert_eq!(
        call(&router, id, key, "sessions/prune", json!({"limit":128})).await,
        (StatusCode::OK, json!({"removed":1}))
    );
    assert_eq!(
        call(
            &router,
            id,
            key,
            "refresh",
            json!({"refresh_token":replacement["refresh_token"]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    for (index, (project, _)) in f.credentials.iter().enumerate() {
        assert_eq!(
            fs::read(f.path.join("registry").join(project).join("data/redo.wal")).unwrap(),
            public_before[index]
        );
    }
    drop(router);
    drop(f.app);
    let report =
        crate::inspect_account_bundle_root(&f.path, PasswordPool::new(1).unwrap()).unwrap();
    let own = report
        .private_accounts
        .iter()
        .find(|r| &r.project == id)
        .unwrap();
    let sibling = report
        .private_accounts
        .iter()
        .find(|r| &r.project == other_id)
        .unwrap();
    assert_eq!(own.inventory.session_families, 0);
    assert_eq!(own.inventory.clock_floor, Some(605750));
    assert_eq!(sibling.inventory.session_families, 1);
    assert_eq!(sibling.inventory.clock_floor, Some(50));
}

#[tokio::test]
async fn pruning_rejects_invalid_limits_and_client_time_without_advancing_private_history() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let before = wal(&f);
    f.clock.store(1000, Ordering::SeqCst);
    for body in [
        json!({"limit":0}),
        json!({"limit":129}),
        json!({"limit":65535}),
        json!({"limit":-1}),
        json!({"limit":65536}),
        json!({"limit":1.5}),
        json!({"limit":"1"}),
        json!({"limit":true}),
        json!({"limit":null}),
        json!({}),
        json!({"limit":1,"now":1000}),
    ] {
        assert_eq!(
            call(&router, id, key, "sessions/prune", body).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(wal(&f), before);
    }
    let path = format!("/v1/projects/{id}/auth/sessions/prune");
    assert_eq!(
        send(
            &router,
            request(&path, key, Body::from("{\"limit\":1,\"limit\":2}"))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    for wrong in [
        MASTER,
        f.credentials[1].1.as_str(),
        "synthetic-invalid-service-key",
    ] {
        assert_eq!(
            send(&router, request(&path, wrong, Body::from(vec![b'x'; 4097])))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        send(&router, request(&path, key, Body::from(vec![b'x'; 4097])))
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(wal(&f), before);
    f.clock.store(49, Ordering::SeqCst);
    assert_eq!(
        call(&router, id, key, "sessions/prune", json!({"limit":1})).await,
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"code":"trusted_clock_unavailable"})
        )
    );
    assert_eq!(wal(&f), before);
    f.clock.store(1000, Ordering::SeqCst);
    assert_eq!(
        call(&router, id, key, "sessions/prune", json!({"limit":1})).await,
        (StatusCode::OK, json!({"removed":0}))
    );
    // Zero removals can still persist the separate forward clock observation.
    assert_ne!(wal(&f)[0], before[0]);
    assert_eq!(wal(&f)[1], before[1]);
}
