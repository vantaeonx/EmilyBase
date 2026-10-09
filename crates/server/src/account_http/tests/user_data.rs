use super::*;
use emilybase_auth::accounts::IssuedSession;
pub(super) const OWN: &[u8] = br#"{"version":1,"select":{"kind":"owner","column":"owner"},"insert":{"kind":"owner","column":"owner"},"update_using":{"kind":"owner","column":"owner"},"update_check":{"kind":"owner","column":"owner"},"delete":{"kind":"owner","column":"owner"}}"#;
pub(super) fn integer(n: i64) -> Value {
    json!({"type":"integer","value":n.to_string()})
}
pub(super) fn row(pk: i64, owner: &[u8], amount: i64) -> Value {
    json!([integer(pk),{"type":"bytes","value":owner},integer(amount)])
}
pub(super) async fn prepare(f: &Fixture) -> [IssuedSession; 2] {
    let (id, key) = &f.credentials[0];
    let mut root = f.app.root.lock().await;
    root.execute(
        id,
        key,
        "CREATE TABLE owned(id INT PRIMARY KEY,owner BYTES,amount INT)",
        &[],
    )
    .unwrap();
    root.create_user(id, key, "second_user", PASSWORD.as_bytes())
        .unwrap();
    root.enable_row_policy_catalog(id, key).unwrap();
    root.install_row_policy(id, key, "owned", 0, OWN).unwrap();
    [
        root.sign_in(id, key, "synthetic_user", PASSWORD.as_bytes(), 50)
            .unwrap(),
        root.sign_in(id, key, "second_user", PASSWORD.as_bytes(), 50)
            .unwrap(),
    ]
}
pub(super) fn public(f: &Fixture) -> Vec<u8> {
    fs::read(
        f.path
            .join("registry")
            .join(&f.credentials[0].0)
            .join("data/redo.wal"),
    )
    .unwrap()
}
fn user_request(id: &str, key: &str, access: &str, op: &str, body: Body) -> Request {
    let mut r = request(&format!("/v1/projects/{id}/auth/rows/{op}"), key, body);
    r.headers_mut()
        .insert(crate::USER_ACCESS_HEADER, access.parse().unwrap());
    r
}
async fn operation(
    router: &Router,
    id: &str,
    key: &str,
    access: &str,
    op: &str,
    input: Value,
) -> (StatusCode, Value) {
    send(
        router,
        user_request(id, key, access, op, Body::from(input.to_string())),
    )
    .await
}
#[tokio::test]
async fn user_data_http_owned_packets_hidden_reads_and_pages_preserve_scope_and_private_history_on_both_wals()
 {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for compact in [false, true] {
        let f = fixture_with_wal(compact);
        let [a, b] = prepare(&f).await;
        let router = routes_app(f.app.clone());
        let (id, key) = &f.credentials[0];
        let private = wal(&f);
        let sibling = fs::read(
            f.path
                .join("registry")
                .join(&f.credentials[1].0)
                .join("data/redo.wal"),
        )
        .unwrap();
        let packet = json!({"table":"owned","operations":[{"op":"insert","row":row(1,&a.metadata.user,10)},{"op":"update","key":integer(1),"row":row(1,&a.metadata.user,20)}]});
        let (status, receipt) =
            operation(&router, id, key, a.access.expose(), "write", packet).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(receipt["changed"], 2);
        assert!(
            receipt["transaction"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .is_ok()
        );
        let before = public(&f);
        assert_eq!(
            operation(
                &router,
                id,
                key,
                a.access.expose(),
                "get",
                json!({"table":"owned","key":integer(1)})
            )
            .await,
            (StatusCode::OK, json!({"row":row(1,&a.metadata.user,20)}))
        );
        for pk in [1, 999] {
            assert_eq!(
                operation(
                    &router,
                    id,
                    key,
                    b.access.expose(),
                    "get",
                    json!({"table":"owned","key":integer(pk)})
                )
                .await,
                (StatusCode::OK, json!({"row":null}))
            );
        }
        assert_eq!(
            operation(
                &router,
                id,
                key,
                b.access.expose(),
                "page",
                json!({"table":"owned","limit":128,"after":null})
            )
            .await,
            (StatusCode::OK, json!({"rows":[],"next":null}))
        );
        assert_eq!(
            operation(
                &router,
                id,
                key,
                a.access.expose(),
                "page",
                json!({"table":"owned","limit":1,"after":null})
            )
            .await,
            (
                StatusCode::OK,
                json!({"rows":[row(1,&a.metadata.user,20)],"next":null})
            )
        );
        let denied = json!({"table":"owned","operations":[{"op":"insert","row":row(2,&a.metadata.user,1)},{"op":"update","key":integer(1),"row":row(1,&b.metadata.user,99)}]});
        assert_eq!(
            operation(&router, id, key, a.access.expose(), "write", denied).await,
            (StatusCode::FORBIDDEN, json!({"code":"user_row_rejected"}))
        );
        for wrong in [MASTER, f.credentials[1].1.as_str(), a.access.expose()] {
            assert_eq!(
                operation(
                    &router,
                    id,
                    wrong,
                    a.access.expose(),
                    "get",
                    json!({"table":"owned","key":integer(1)})
                )
                .await
                .0,
                StatusCode::UNAUTHORIZED
            );
        }
        assert_eq!(
            operation(
                &router,
                id,
                key,
                a.refresh.expose(),
                "get",
                json!({"table":"owned","key":integer(1)})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            operation(
                &router,
                &f.credentials[1].0,
                &f.credentials[1].1,
                a.access.expose(),
                "get",
                json!({"table":"t","key":integer(1)})
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        assert_eq!(public(&f), before);
        assert_eq!(wal(&f), private);
        assert_eq!(
            fs::read(
                f.path
                    .join("registry")
                    .join(&f.credentials[1].0)
                    .join("data/redo.wal")
            )
            .unwrap(),
            sibling
        );
    }
}
#[tokio::test]
async fn user_data_http_rejects_missing_duplicate_oversized_headers_and_unknown_assertions_without_writes()
 {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let [a, _] = prepare(&f).await;
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let before = public(&f);
    let private = wal(&f);
    let path = format!("/v1/projects/{id}/auth/rows/get");
    let missing = request(&path, key, Body::empty());
    assert_eq!(send(&router, missing).await.0, StatusCode::UNAUTHORIZED);
    let mut duplicate = user_request(id, key, a.access.expose(), "get", Body::empty());
    duplicate.headers_mut().append(
        crate::USER_ACCESS_HEADER,
        a.access.expose().parse().unwrap(),
    );
    assert_eq!(send(&router, duplicate).await.0, StatusCode::UNAUTHORIZED);
    for text in [String::new(), "x".repeat(103), "x".repeat(102)] {
        assert_eq!(
            operation(
                &router,
                id,
                key,
                &text,
                "get",
                json!({"table":"owned","key":integer(1)})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let mut non_ascii = user_request(id, key, a.access.expose(), "get", Body::empty());
    non_ascii.headers_mut().insert(
        crate::USER_ACCESS_HEADER,
        axum::http::HeaderValue::from_bytes(&[0xff; 102]).unwrap(),
    );
    assert_eq!(send(&router, non_ascii).await.0, StatusCode::UNAUTHORIZED);
    for body in [
        json!({"table":"owned","limit":0}),
        json!({"table":"owned","limit":129}),
        json!({"table":"owned","limit":1,"time":50}),
        json!({"table":"owned","limit":1,"schema":{}}),
        json!({"table":"owned","limit":1,"project":id}),
    ] {
        assert_eq!(
            operation(&router, id, key, a.access.expose(), "page", body).await,
            (StatusCode::BAD_REQUEST, json!({"code":"user_row_rejected"}))
        );
    }
    assert_eq!(
        send(
            &router,
            user_request(
                id,
                key,
                a.access.expose(),
                "get",
                Body::from("x".repeat(crate::http::MAX_BODY + 1))
            )
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(public(&f), before);
    assert_eq!(wal(&f), private);
}
#[tokio::test]
async fn user_data_http_waiting_body_rechecks_current_key_policy_and_session_before_any_public_commit()
 {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for change in ["key", "policy", "session"] {
        let f = fixture();
        let [a, _] = prepare(&f).await;
        let router = routes_app(f.app.clone());
        let (id, key) = &f.credentials[0];
        let before = public(&f);
        let (release, wait) = tokio::sync::oneshot::channel::<Bytes>();
        let body = Body::from_stream(futures_util::stream::once(async move {
            Ok::<_, std::io::Error>(wait.await.unwrap())
        }));
        let pending = tokio::spawn(router.clone().oneshot(user_request(
            id,
            key,
            a.access.expose(),
            "write",
            body,
        )));
        tokio::time::timeout(Duration::from_secs(2), async {
            while f.app.workers.available_permits() != 3 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        if change == "key" {
            assert_eq!(
                send(
                    &router,
                    request(
                        &format!("/v1/projects/{id}/keys/rotate"),
                        MASTER,
                        Body::empty()
                    )
                )
                .await
                .0,
                StatusCode::OK
            );
        } else {
            let mut root = f.app.root.lock().await;
            if change == "session" {
                root.set_disabled(id, key, "synthetic_user", true).unwrap();
            } else {
                let revision = root.row_policy_receipts(id, key).unwrap()[0].revision;
                let deny = String::from_utf8(OWN.to_vec()).unwrap().replace(
                    "\"kind\":\"owner\",\"column\":\"owner\"",
                    "\"kind\":\"deny\"",
                );
                root.install_row_policy(id, key, "owned", revision, deny.as_bytes())
                    .unwrap();
            }
        }
        let private = wal(&f);
        let bytes = if change == "key" {
            Bytes::from_static(b"malformed-after-key-rotation")
        } else {
            Bytes::from(json!({"table":"owned","operations":[{"op":"insert","row":row(1,&a.metadata.user,10)}]}).to_string())
        };
        release.send(bytes).unwrap();
        let response = pending.await.unwrap().unwrap();
        assert_eq!(
            response.status(),
            if change == "policy" {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::UNAUTHORIZED
            }
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(public(&f), before);
        assert_eq!(wal(&f), private);
    }
}
#[tokio::test]
async fn user_data_http_oversized_filtered_response_is_inspection_error_and_smaller_page_is_read_only()
 {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let [a, _] = prepare(&f).await;
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    {
        let mut root = f.app.root.lock().await;
        root.execute(
            id,
            key,
            "CREATE TABLE large_owned(id INT PRIMARY KEY,owner BYTES,payload TEXT)",
            &[],
        )
        .unwrap();
        root.install_row_policy(id, key, "large_owned", 0, OWN)
            .unwrap();
        let row = |n| {
            vec![
                emilybase_catalog::Value::Integer(n),
                emilybase_catalog::Value::Bytes(a.metadata.user.to_vec()),
                emilybase_catalog::Value::Text("界".repeat(1024)),
            ]
        };
        root.user_table(
            id,
            key,
            "large_owned",
            a.access.expose(),
            50,
            crate::UserTableOperation::Write(
                (0..128).map(|n| crate::UserWrite::Insert(row(n))).collect(),
            ),
        )
        .unwrap();
    }
    let before = public(&f);
    let private = wal(&f);
    assert_eq!(
        operation(
            &router,
            id,
            key,
            a.access.expose(),
            "page",
            json!({"table":"large_owned","limit":128})
        )
        .await,
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"code":"user_row_outcome_requires_inspection"})
        )
    );
    let (status, page) = operation(
        &router,
        id,
        key,
        a.access.expose(),
        "page",
        json!({"table":"large_owned","limit":1}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["rows"][0][0], integer(0));
    assert_eq!(page["next"], integer(0));
    assert_eq!(public(&f), before);
    assert_eq!(wal(&f), private);
}
#[tokio::test]
async fn user_data_http_private_attempt_bound_applies_to_valid_filtered_reads() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let [a, _] = prepare(&f).await;
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let before = public(&f);
    let private = wal(&f);
    for _ in 0..PRIVATE_ATTEMPTS {
        assert_eq!(
            operation(
                &router,
                id,
                key,
                a.access.expose(),
                "get",
                json!({"table":"owned","key":integer(1)})
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert_eq!(
        operation(
            &router,
            id,
            key,
            a.access.expose(),
            "get",
            json!({"table":"owned","key":integer(1)})
        )
        .await,
        (
            StatusCode::TOO_MANY_REQUESTS,
            json!({"code":"account_rate_limit"})
        )
    );
    assert_eq!(public(&f), before);
    assert_eq!(wal(&f), private);
}
#[tokio::test]
async fn user_data_http_routes_do_not_exist_in_legacy_registry_mode() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = crate::ProjectStore::open(dir.path().join("registry")).unwrap();
    let project = registry.create("synthetic-legacy-project").unwrap();
    let router = crate::router(registry, MASTER).unwrap();
    for op in ["get", "page", "write"] {
        let response = router
            .clone()
            .oneshot(user_request(
                &project.project.id,
                &project.api_key,
                &"x".repeat(102),
                op,
                Body::from("{}"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn user_data_http_schema_specific_key_types_are_bad_requests_even_for_empty_tables() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let [a, _] = prepare(&f).await;
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let before = public(&f);
    let private = wal(&f);
    for (op, input) in [
        (
            "get",
            json!({"table":"owned","key":{"type":"text","value":"wrong-type"}}),
        ),
        (
            "page",
            json!({"table":"owned","after":{"type":"text","value":"wrong-type"},"limit":1}),
        ),
    ] {
        assert_eq!(
            operation(&router, id, key, a.access.expose(), op, input).await,
            (StatusCode::BAD_REQUEST, json!({"code":"user_row_rejected"}))
        );
    }
    assert_eq!(operation(&router,id,key,a.access.expose(),"write",json!({"table":"owned","operations":[{"op":"insert","row":row(2,&a.metadata.user,1)},{"op":"delete","key":{"type":"text","value":"wrong-type"}}]})).await,(StatusCode::BAD_REQUEST,json!({"code":"user_row_rejected"})));
    assert_eq!(public(&f), before);
    assert_eq!(wal(&f), private);
}
