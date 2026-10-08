use super::*;
fn integer(v: i64) -> Value {
    json!({"type":"integer","value":v.to_string()})
}
async fn operation(
    router: &Router,
    id: &str,
    key: &str,
    op: &str,
    input: Value,
) -> (StatusCode, Value) {
    send(
        router,
        request(
            &format!("/v1/projects/{id}/tables/rows/{op}"),
            key,
            Body::from(input.to_string()),
        ),
    )
    .await
}
fn public(f: &Fixture, index: usize) -> Vec<u8> {
    fs::read(
        f.path
            .join("registry")
            .join(&f.credentials[index].0)
            .join("data/redo.wal"),
    )
    .unwrap()
}
#[tokio::test]
async fn typed_row_crud_is_scoped_and_preserves_private_history_without_clock_observation() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let (_, other_key) = &f.credentials[1];
    let (status, pair) = call(
        &router,
        id,
        key,
        "sign-in",
        json!({"login":"synthetic_user","password":PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for token in [
        pair["access_token"].as_str().unwrap(),
        pair["refresh_token"].as_str().unwrap(),
    ] {
        assert_eq!(
            operation(
                &router,
                id,
                token,
                "get",
                json!({"table":"t","key":integer(1)})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let before = wal(&f);
    let sibling = public(&f, 1);
    f.clock.store(0, Ordering::SeqCst);
    let point = json!({"table":"t","key":integer(i64::MAX)});
    let insert = json!({"table":"t","row":[integer(i64::MAX)]});
    for denied in [MASTER, other_key] {
        assert_eq!(
            operation(&router, id, denied, "insert", insert.clone())
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (a, b) = tokio::join!(
        operation(&router, id, key, "insert", insert.clone()),
        operation(&router, id, key, "insert", insert.clone())
    );
    let mut statuses = [a.0.as_u16(), b.0.as_u16()];
    statuses.sort_unstable();
    assert_eq!(statuses, [200, 400]);
    let (status, created) = if a.0 == StatusCode::OK { a } else { b };
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["key"], integer(i64::MAX));
    assert!(
        created["transaction"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 1
    );
    let committed = public(&f, 0);
    assert_eq!(
        operation(&router, id, key, "insert", insert).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(public(&f, 0), committed);
    assert_eq!(
        operation(&router, id, key, "get", point.clone()).await,
        (StatusCode::OK, json!({"row":[integer(i64::MAX)]}))
    );
    let page = operation(&router, id, key, "page", json!({"table":"t","limit":1})).await;
    assert_eq!(page.0, StatusCode::OK);
    assert_eq!(page.1["rows"], json!([[integer(1)]]));
    assert_eq!(page.1["next"], integer(1));
    let (status, next) = operation(
        &router,
        id,
        key,
        "page",
        json!({"table":"t","limit":1,"after":page.1["next"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(next["rows"], json!([[integer(i64::MAX)]]));
    assert!(next["next"].is_null());
    assert_eq!(public(&f, 0), committed);
    assert_eq!(
        operation(
            &router,
            id,
            key,
            "update",
            json!({"table":"t","key":integer(i64::MAX),"row":[integer(i64::MAX)]})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        operation(&router, id, key, "delete", point.clone()).await.0,
        StatusCode::OK
    );
    assert_eq!(
        operation(&router, id, key, "get", point.clone()).await,
        (StatusCode::OK, json!({"row":null}))
    );
    assert_eq!(
        operation(&router, id, key, "delete", point).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(public(&f, 1), sibling);
    assert_eq!(wal(&f), before);
}
#[tokio::test]
async fn every_row_route_rechecks_current_key_after_body_wait_and_bounds_bad_documents() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for op in ["get", "page", "insert", "update", "delete"] {
        let f = fixture();
        let router = routes_app(f.app.clone());
        let (id, key) = &f.credentials[0];
        let before = wal(&f);
        let public_before = public(&f, 0);
        let payload = match op {
            "page" => json!({"table":"t","limit":1}),
            "insert" => json!({"table":"t","row":[integer(2)]}),
            "update" => json!({"table":"t","key":integer(1),"row":[integer(1)]}),
            _ => json!({"table":"t","key":integer(1)}),
        };
        let (release, wait) = tokio::sync::oneshot::channel::<Bytes>();
        let body = Body::from_stream(futures_util::stream::once(async move {
            Ok::<_, std::io::Error>(wait.await.unwrap())
        }));
        let route = format!("/v1/projects/{id}/tables/rows/{op}");
        let pending = tokio::spawn(router.clone().oneshot(request(&route, key, body)));
        tokio::time::timeout(Duration::from_secs(2), async {
            while f.app.workers.available_permits() != 3 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
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
        release.send(Bytes::from(payload.to_string())).unwrap();
        let denied = pending.await.unwrap().unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(denied.headers()[header::CACHE_CONTROL], "no-store");
        let key = rotated["api_key"].as_str().unwrap();
        for input in [
            "{}".to_string(),
            "synthetic-private-payload".into(),
            "{\"table\":\"t\",\"table\":\"t\"}".into(),
            " ".repeat(crate::http::MAX_BODY + 1),
        ] {
            let (status, error) = send(&router, request(&route, key, Body::from(input))).await;
            assert!(matches!(
                status,
                StatusCode::BAD_REQUEST | StatusCode::PAYLOAD_TOO_LARGE
            ));
            assert!(!error.to_string().contains("synthetic-private"));
        }
        assert_eq!(public(&f, 0), public_before);
        assert_eq!(wal(&f), before);
    }
}
