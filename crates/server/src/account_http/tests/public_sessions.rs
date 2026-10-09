use super::*;
use axum::http::HeaderValue;

pub(super) async fn enable(f: &Fixture, index: usize) {
    let (id, key) = &f.credentials[index];
    let mut root = f.app.root.lock().await;
    root.enable_row_policy_catalog(id, key).unwrap();
    let closed = root.enable_public_admission_catalog(id, key).unwrap();
    root.set_public_admission(id, key, closed.revision, true)
        .unwrap();
}
pub(super) fn public_request(
    id: &str,
    operation: &str,
    access: Option<&str>,
    body: Body,
) -> Request {
    let mut request = Request::builder()
        .method("POST")
        .uri(format!("/v1/projects/{id}/user/{operation}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .unwrap();
    if let Some(access) = access {
        request.headers_mut().insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {access}")).unwrap(),
        );
    }
    request
        .extensions_mut()
        .insert(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()));
    request
}
async fn public_call(
    router: &Router,
    id: &str,
    operation: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    send(
        router,
        public_request(id, operation, token, Body::from(body.to_string())),
    )
    .await
}
async fn public_sign(router: &Router, id: &str) -> Value {
    let (status, pair) = public_call(
        router,
        id,
        "sign-in",
        None,
        json!({"login":"synthetic_user","password":PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pair["access_token"].as_str().unwrap().len(), 102);
    assert_eq!(pair["refresh_token"].as_str().unwrap().len(), 102);
    assert_eq!(pair["expires_at"], "950");
    assert_eq!(pair["token_type"], "Bearer");
    pair
}
fn all_history(f: &Fixture) -> Vec<Vec<u8>> {
    let mut result = wal(f);
    for (id, _) in &f.credentials {
        result.push(fs::read(f.path.join("registry").join(id).join("data/redo.wal")).unwrap());
    }
    result
}
pub(super) async fn delayed(
    router: &Router,
    id: &str,
    operation: &str,
    token: Option<&str>,
) -> (
    tokio::sync::oneshot::Sender<Bytes>,
    tokio::task::JoinHandle<Response>,
) {
    let (release, wait) = tokio::sync::oneshot::channel();
    let (entered, observed) = tokio::sync::oneshot::channel();
    let body = Body::from_stream(futures_util::stream::once(async move {
        entered.send(()).unwrap();
        Ok::<Bytes, std::io::Error>(wait.await.unwrap())
    }));
    let request = public_request(id, operation, token, body);
    let router = router.clone();
    let pending = tokio::spawn(async move { router.oneshot(request).await.unwrap() });
    tokio::time::timeout(Duration::from_secs(2), observed)
        .await
        .unwrap()
        .unwrap();
    (release, pending)
}

#[tokio::test]
async fn closed_legacy_unknown_projects_refuse_before_body_clock_or_rate_map_work() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for compact in [false, true] {
        let f = fixture_with_wal(compact);
        let router = routes_app(f.app.clone());
        let (id, key) = &f.credentials[0];
        f.clock.store(u64::MAX, Ordering::SeqCst);
        for version in [3, 4, 5] {
            {
                let mut root = f.app.root.lock().await;
                if version == 4 {
                    root.enable_row_policy_catalog(id, key).unwrap();
                }
                if version == 5 {
                    root.enable_public_admission_catalog(id, key).unwrap();
                }
            }
            let before = all_history(&f);
            for operation in ["sign-in", "refresh", "logout", "me", "password"] {
                for project in [
                    id.as_str(),
                    "00000000000000000000000000000000",
                    "invalid-project",
                ] {
                    let polls = Arc::new(AtomicU64::new(0));
                    let watched = polls.clone();
                    let body = Body::from_stream(futures_util::stream::poll_fn(move |_| {
                        watched.fetch_add(1, Ordering::SeqCst);
                        std::task::Poll::Ready(Some(Ok::<Bytes, std::io::Error>(
                            Bytes::from_static(b"private"),
                        )))
                    }));
                    let access = "x".repeat(102);
                    let result = send(
                        &router,
                        public_request(
                            project,
                            operation,
                            matches!(operation, "me" | "password").then_some(access.as_str()),
                            body,
                        ),
                    )
                    .await;
                    assert_eq!(
                        result,
                        (StatusCode::UNAUTHORIZED, json!({"code":"access_denied"}))
                    );
                    assert_eq!(polls.load(Ordering::SeqCst), 0);
                }
            }
            assert_eq!(all_history(&f), before);
            assert!(f.app.projects.lock().unwrap().0.is_empty());
            assert_eq!(f.app.workers.available_permits(), 4);
        }
    }
}

#[tokio::test]
async fn public_session_routes_preserve_original_refresh_logout_scope_and_service_authority() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for compact in [false, true] {
        let f = fixture_with_wal(compact);
        enable(&f, 0).await;
        enable(&f, 1).await;
        let router = routes_app(f.app.clone());
        let (id, key) = &f.credentials[0];
        let before = all_history(&f);
        for login in ["synthetic_user", "missing_user"] {
            assert_eq!(
                public_call(
                    &router,
                    id,
                    "sign-in",
                    None,
                    json!({"login":login,"password":"wrong-synthetic-password"})
                )
                .await,
                (StatusCode::UNAUTHORIZED, json!({"code":"access_denied"}))
            );
        }
        assert_eq!(all_history(&f), before);
        let pair = public_sign(&router, id).await;
        let access = pair["access_token"].as_str().unwrap();
        let (status, me) = public_call(&router, id, "me", Some(access), json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(me["login"], "synthetic_user");
        assert_eq!(me["id"].as_str().unwrap().len(), 32);
        assert_eq!(me["credential_epoch"], "1");
        assert_eq!(me["disabled"], false);
        for token in [
            key.as_str(),
            MASTER,
            pair["refresh_token"].as_str().unwrap(),
        ] {
            assert_eq!(
                public_call(&router, id, "me", Some(token), json!({}))
                    .await
                    .0,
                StatusCode::UNAUTHORIZED
            );
        }
        assert_eq!(
            public_call(&router, &f.credentials[1].0, "me", Some(access), json!({}))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            public_call(
                &router,
                &f.credentials[1].0,
                "refresh",
                None,
                json!({"refresh_token":pair["refresh_token"]})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        for path in [
            format!("/v1/projects/{id}/sql"),
            format!("/v1/projects/{id}/auth/users"),
            format!("/v1/projects/{id}/keys/rotate"),
        ] {
            assert_eq!(
                send(&router, request(&path, access, Body::from("{}")))
                    .await
                    .0,
                StatusCode::UNAUTHORIZED
            );
        }
        let current = f.app.root.lock().await.rotate_project_key(id).unwrap();
        assert_eq!(
            public_call(&router, id, "me", Some(access), json!({}))
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            call(&router, id, key, "me", json!({"access_token":access}))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(
                &router,
                id,
                &current.api_key,
                "me",
                json!({"access_token":access})
            )
            .await
            .0,
            StatusCode::OK
        );
        let (status, next) = public_call(
            &router,
            id,
            "refresh",
            None,
            json!({"refresh_token":pair["refresh_token"]}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            public_call(&router, id, "me", Some(access), json!({}))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            public_call(
                &router,
                id,
                "refresh",
                None,
                json!({"refresh_token":pair["refresh_token"]})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            public_call(
                &router,
                id,
                "me",
                Some(next["access_token"].as_str().unwrap()),
                json!({})
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(
            public_call(
                &router,
                id,
                "logout",
                None,
                json!({"refresh_token":next["refresh_token"]})
            )
            .await,
            (StatusCode::OK, json!({"logged_out":true}))
        );
        assert_eq!(
            public_call(
                &router,
                id,
                "me",
                Some(next["access_token"].as_str().unwrap()),
                json!({})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        let after = all_history(&f);
        assert_eq!(after[1..], before[1..]);
    }
}

#[tokio::test]
async fn public_headers_json_fields_and_size_bounds_refuse_without_secret_echo_or_writes() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    enable(&f, 0).await;
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let before = all_history(&f);
    for operation in ["sign-in", "refresh", "logout"] {
        let result = public_call(&router, id, operation, Some(key), json!({})).await;
        assert_eq!(
            result,
            (StatusCode::UNAUTHORIZED, json!({"code":"access_denied"}))
        );
    }
    for token in [None, Some("short"), Some("Bearer synthetic-private-token")] {
        assert_eq!(
            public_call(&router, id, "me", token, json!({})).await,
            (StatusCode::UNAUTHORIZED, json!({"code":"access_denied"}))
        );
    }
    let invalid = "x".repeat(102);
    let mut request = public_request(id, "me", Some(&invalid), Body::from("{}"));
    request.headers_mut().append(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer synthetic-private-duplicate"),
    );
    assert_eq!(
        send(&router, request).await,
        (StatusCode::UNAUTHORIZED, json!({"code":"access_denied"}))
    );
    for (operation, body) in [
        (
            "sign-in",
            b"{\"login\":\"synthetic_user\",\"password\":\"synthetic-private-body\",\"now\":50}"
                .as_slice(),
        ),
        (
            "sign-in",
            b"{\"login\":\"x\",\"login\":\"y\",\"password\":\"synthetic-private-body\"}",
        ),
        (
            "refresh",
            b"{\"refresh_token\":\"private\",\"refresh_token\":\"duplicate\"}",
        ),
        ("logout", b"{\"refresh_token\":\"private\",\"now\":50}"),
        ("me", b"{\"access_token\":\"synthetic-private-body\"}"),
        ("me", b"[]"),
    ] {
        let result = send(
            &router,
            public_request(
                id,
                operation,
                (operation == "me").then_some(invalid.as_str()),
                Body::from(body),
            ),
        )
        .await;
        assert_eq!(
            result,
            (StatusCode::BAD_REQUEST, json!({"code":"invalid_json"}))
        );
        assert!(!result.1.to_string().contains("synthetic-private-body"));
    }
    let mut request = public_request(id, "sign-in", None, Body::empty());
    request.headers_mut().remove(header::CONTENT_TYPE);
    assert_eq!(
        send(&router, request).await.0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(
        send(
            &router,
            public_request(id, "sign-in", None, Body::from(vec![b' '; 4097]))
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        send(
            &router,
            public_request(id, "sign-in", None, Body::from(vec![b' '; 4096]))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let mut request = public_request(id, "sign-in", None, Body::from("{}"));
    request.extensions_mut().remove::<ConnectInfo<SocketAddr>>();
    assert_eq!(
        send(&router, request).await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(all_history(&f), before);
    assert_eq!(f.app.workers.available_permits(), 4);
}

#[tokio::test]
async fn public_body_wait_rechecks_current_flag_epoch_and_trusted_time_inside_original_native_owner()
 {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for compact in [false, true] {
        let f = fixture_with_wal(compact);
        enable(&f, 0).await;
        let router = routes_app(f.app.clone());
        let (id, key) = &f.credentials[0];
        let (release, pending) = delayed(&router, id, "sign-in", None).await;
        {
            let mut root = f.app.root.lock().await;
            let receipt = root.public_admission(id, key).unwrap();
            root.set_public_admission(id, key, receipt.revision, false)
                .unwrap();
        }
        let shut = all_history(&f);
        f.clock.store(u64::MAX, Ordering::SeqCst);
        release
            .send(Bytes::from(
                json!({"login":"synthetic_user","password":PASSWORD}).to_string(),
            ))
            .unwrap();
        assert_eq!(pending.await.unwrap().status(), StatusCode::UNAUTHORIZED);
        assert_eq!(all_history(&f), shut);
        f.clock.store(50, Ordering::SeqCst);
        {
            let mut root = f.app.root.lock().await;
            let receipt = root.public_admission(id, key).unwrap();
            root.set_public_admission(id, key, receipt.revision, true)
                .unwrap();
        }
        let pair = public_sign(&router, id).await;
        let access = pair["access_token"].as_str().unwrap();
        let (release, pending) = delayed(&router, id, "me", Some(access)).await;
        f.app
            .root
            .lock()
            .await
            .set_disabled(id, key, "synthetic_user", true)
            .unwrap();
        let disabled = all_history(&f);
        release.send(Bytes::from_static(b"{}")).unwrap();
        assert_eq!(pending.await.unwrap().status(), StatusCode::UNAUTHORIZED);
        assert_eq!(all_history(&f), disabled);
        f.app
            .root
            .lock()
            .await
            .set_disabled(id, key, "synthetic_user", false)
            .unwrap();
        let next = public_sign(&router, id).await;
        let (release, pending) = delayed(
            &router,
            id,
            "me",
            Some(next["access_token"].as_str().unwrap()),
        )
        .await;
        f.clock.store(100, Ordering::SeqCst);
        release.send(Bytes::from_static(b"{}")).unwrap();
        assert_eq!(pending.await.unwrap().status(), StatusCode::OK);
        let observed = all_history(&f);
        f.clock.store(99, Ordering::SeqCst);
        assert_eq!(
            public_call(
                &router,
                id,
                "me",
                Some(next["access_token"].as_str().unwrap()),
                json!({})
            )
            .await,
            (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"code":"trusted_clock_unavailable"})
            )
        );
        assert_eq!(all_history(&f), observed);
    }
}

#[tokio::test]
async fn public_slow_bodies_share_original_workers_cancel_cleanly_and_time_out_without_wal_work() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    enable(&f, 0).await;
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let before = all_history(&f);
    let mut tasks = Vec::new();
    for _ in 0..4 {
        tasks.push(tokio::spawn(router.clone().oneshot(public_request(
            id,
            "sign-in",
            None,
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
        call(
            &router,
            id,
            key,
            "sign-in",
            json!({"login":"synthetic_user","password":PASSWORD})
        )
        .await,
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"code":"workers_busy"})
        )
    );
    let health = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    for task in tasks {
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
    let body = Body::from_stream(futures_util::stream::pending::<Result<Bytes, std::io::Error>>());
    assert_eq!(
        send(&router, public_request(id, "sign-in", None, body)).await,
        (StatusCode::REQUEST_TIMEOUT, json!({"code":"body_timeout"}))
    );
    assert_eq!(all_history(&f), before);
    assert_eq!(f.app.workers.available_permits(), 4);
}

#[tokio::test]
async fn public_unknown_project_flood_cannot_fill_project_map_and_real_peer_limits_ignore_forwarding()
 {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let before = all_history(&f);
    for i in 0..140u32 {
        let id = format!("{i:032x}");
        let mut request = public_request(&id, "sign-in", None, Body::empty());
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([10, 0, 0, i as u8], 1234))));
        assert_eq!(send(&router, request).await.0, StatusCode::UNAUTHORIZED);
    }
    assert!(f.app.projects.lock().unwrap().0.is_empty());
    enable(&f, 0).await;
    let after = all_history(&f);
    let (id, key) = &f.credentials[0];
    for _ in 0..29 {
        assert_eq!(
            public_call(&router, id, "sign-in", None, json!({})).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        call(&router, id, key, "sign-in", json!({})).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        public_call(&router, id, "sign-in", None, json!({})).await,
        (
            StatusCode::TOO_MANY_REQUESTS,
            json!({"code":"account_rate_limit"})
        )
    );
    assert_eq!(f.app.projects.lock().unwrap().0.len(), 1);
    assert_eq!(all_history(&f), after);
    assert_eq!(after[1..], before[1..]);
    let peer_f = fixture();
    let peer_router = routes_app(peer_f.app.clone());
    let unknown = "00000000000000000000000000000000";
    for i in 0..121 {
        let mut request = public_request(unknown, "sign-in", None, Body::empty());
        request.headers_mut().insert(
            "x-forwarded-for",
            HeaderValue::from_str(&format!("10.1.0.{}", i)).unwrap(),
        );
        let expected = if i < 120 {
            StatusCode::UNAUTHORIZED
        } else {
            StatusCode::TOO_MANY_REQUESTS
        };
        assert_eq!(send(&peer_router, request).await.0, expected);
    }
}

#[tokio::test]
async fn public_refresh_race_has_one_winner_and_body_wait_does_not_block_health_or_owner_progress()
{
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    enable(&f, 0).await;
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let pair = public_sign(&router, id).await;
    let body = json!({"refresh_token":pair["refresh_token"]});
    let (a, b) = tokio::join!(
        public_call(&router, id, "refresh", None, body.clone()),
        public_call(&router, id, "refresh", None, body)
    );
    let mut statuses = [a.0, b.0];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::UNAUTHORIZED]);
    let next = if a.0 == StatusCode::OK { a.1 } else { b.1 };
    let (release, pending) = delayed(
        &router,
        id,
        "me",
        Some(next["access_token"].as_str().unwrap()),
    )
    .await;
    let mut owner = f.app.root.lock().await;
    release.send(Bytes::from_static(b"{}")).unwrap();
    let health = tokio::time::timeout(
        Duration::from_secs(2),
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
    assert_eq!(health.status(), StatusCode::OK);
    let receipt = owner.public_admission(id, key).unwrap();
    owner
        .set_public_admission(id, key, receipt.revision, false)
        .unwrap();
    let closed = all_history(&f);
    drop(owner);
    assert_eq!(pending.await.unwrap().status(), StatusCode::UNAUTHORIZED);
    assert_eq!(all_history(&f), closed);
}

#[tokio::test]
async fn closing_after_body_admission_refuses_before_a_failed_server_clock_is_called() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    enable(&f, 0).await;
    let reads = Arc::new(AtomicU64::new(0));
    let watched = reads.clone();
    let mut app = f.app.clone();
    app.clock = Arc::new(move || {
        watched.fetch_add(1, Ordering::SeqCst);
        Err(Error::Config("synthetic unavailable clock"))
    });
    let router = routes_app(app);
    let (id, key) = &f.credentials[0];
    let (release, pending) = delayed(&router, id, "sign-in", None).await;
    {
        let mut owner = f.app.root.lock().await;
        let receipt = owner.public_admission(id, key).unwrap();
        owner
            .set_public_admission(id, key, receipt.revision, false)
            .unwrap();
    }
    let before = all_history(&f);
    release
        .send(Bytes::from(
            json!({"login":"synthetic_user","password":PASSWORD}).to_string(),
        ))
        .unwrap();
    assert_eq!(pending.await.unwrap().status(), StatusCode::UNAUTHORIZED);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(all_history(&f), before);
}

#[tokio::test]
async fn existing_private_account_json_also_requires_objects_instead_of_serde_struct_sequences() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let before = all_history(&f);
    for (operation, body) in [
        ("sign-in", json!(["synthetic_user", PASSWORD])),
        ("refresh", json!(["synthetic-private-token"])),
        ("logout", json!(["synthetic-private-token"])),
        ("me", json!(["synthetic-private-token"])),
    ] {
        assert_eq!(
            call(&router, id, key, operation, body).await,
            (StatusCode::BAD_REQUEST, json!({"code":"invalid_json"}))
        );
    }
    assert_eq!(all_history(&f), before);
}
