use super::*;
fn schema() -> Value {
    json!({"name":"temporary","columns":[{"name":"flag","data_type":"boolean","nullable":true},{"name":"key","data_type":"text","nullable":false}],"primary_key":1})
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
            &format!("/v1/projects/{id}/tables/{op}"),
            key,
            Body::from(input.to_string()),
        ),
    )
    .await
}
async fn list(router: &Router, id: &str, key: &str) -> (StatusCode, Value) {
    let mut r = request(&format!("/v1/projects/{id}/tables"), key, Body::empty());
    *r.method_mut() = axum::http::Method::GET;
    send(router, r).await
}
fn public_wal(f: &Fixture, index: usize) -> Vec<u8> {
    fs::read(
        f.path
            .join("registry")
            .join(&f.credentials[index].0)
            .join("data/redo.wal"),
    )
    .unwrap()
}
#[tokio::test]
async fn typed_table_operations_are_scoped_durable_and_keep_private_history_exact() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let (other, other_key) = &f.credentials[1];
    let before = wal(&f);
    let sibling = public_wal(&f, 1);
    f.clock.store(0, Ordering::SeqCst);
    assert_eq!(
        list(&router, id, key).await,
        (
            StatusCode::OK,
            json!({"tables":[{"id":"1","name":"t","columns":1,"primary_key":"id"}]})
        )
    );
    for wrong in [MASTER, other_key] {
        assert_eq!(
            operation(&router, id, wrong, "create", schema()).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(list(&router, id, wrong).await.0, StatusCode::UNAUTHORIZED);
    }
    let (status, created) = operation(&router, id, key, "create", schema()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        created["table"],
        json!({"id":"2","name":"temporary","columns":2,"primary_key":"key"})
    );
    assert!(
        created["transaction"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 1
    );
    assert_eq!(
        operation(&router, id, key, "schema", json!({"table":"temporary"})).await,
        (StatusCode::OK, schema())
    );
    let unchanged = public_wal(&f, 0);
    assert_eq!(
        operation(&router, id, key, "create", schema()).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(public_wal(&f, 0), unchanged);
    assert_eq!(
        operation(
            &router,
            other,
            other_key,
            "schema",
            json!({"table":"temporary"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        operation(&router, id, key, "drop", json!({"table":"temporary"}))
            .await
            .0,
        StatusCode::OK
    );
    let (status, recreated) = operation(&router, id, key, "create", schema()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recreated["table"]["id"], "3");
    assert_eq!(
        list(&router, id, key).await.1["tables"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(public_wal(&f, 1), sibling);
    assert_eq!(wal(&f), before);
}
#[tokio::test]
async fn table_schema_requests_recheck_keys_after_body_wait_and_refuse_bad_input_without_writes() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for op in ["schema", "create", "drop"] {
        let f = fixture();
        let router = routes_app(f.app.clone());
        let (id, key) = &f.credentials[0];
        let before = wal(&f);
        let public = public_wal(&f, 0);
        let payload = if op == "create" {
            schema()
        } else {
            json!({"table":"t"})
        };
        let (release, wait) = tokio::sync::oneshot::channel::<Bytes>();
        let body = Body::from_stream(futures_util::stream::once(async move {
            Ok::<_, std::io::Error>(wait.await.unwrap())
        }));
        let path = format!("/v1/projects/{id}/tables/{op}");
        let pending = tokio::spawn(router.clone().oneshot(request(&path, key, body)));
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
            "synthetic-private-input".into(),
            "{\"table\":\"t\",\"table\":\"t\"}".into(),
            " ".repeat(crate::http::MAX_BODY + 1),
        ] {
            let (status, error) = send(&router, request(&path, key, Body::from(input))).await;
            assert!(matches!(
                status,
                StatusCode::BAD_REQUEST | StatusCode::PAYLOAD_TOO_LARGE
            ));
            assert!(!error.to_string().contains("synthetic-private"));
        }
        assert_eq!(
            list(&router, id, key).await.1["tables"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(public_wal(&f, 0), public);
        assert_eq!(wal(&f), before);
    }
}
