use super::*;
async fn operation(router: &Router, id: &str, key: &str, input: Value) -> (StatusCode, Value) {
    send(
        router,
        request(
            &format!("/v1/projects/{id}/migrations/apply"),
            key,
            Body::from(input.to_string()),
        ),
    )
    .await
}
async fn list(router: &Router, id: &str, key: &str) -> (StatusCode, Value) {
    let mut r = request(&format!("/v1/projects/{id}/migrations"), key, Body::empty());
    *r.method_mut() = axum::http::Method::GET;
    send(router, r).await
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
async fn migrations_use_current_service_scope_and_concurrent_exact_retries_commit_once_without_private_clock_work()
 {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let (other, other_key) = &f.credentials[1];
    let pair = sign(&router, id, key).await;
    let before = wal(&f);
    let sibling = public(&f, 1);
    f.clock.store(0, Ordering::SeqCst);
    let input = json!({"version":1,"label":"initial","sql":"CREATE TABLE created(id INT PRIMARY KEY); INSERT INTO created SELECT * FROM t"});
    for wrong in [
        MASTER,
        other_key.as_str(),
        pair["access_token"].as_str().unwrap(),
        pair["refresh_token"].as_str().unwrap(),
    ] {
        assert_eq!(
            operation(&router, id, wrong, input.clone()).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(list(&router, id, wrong).await.0, StatusCode::UNAUTHORIZED);
    }
    let (first, second) = tokio::join!(
        operation(&router, id, key, input.clone()),
        operation(&router, id, key, input.clone())
    );
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(second.0, StatusCode::OK);
    assert_ne!(first.1["already_applied"], second.1["already_applied"]);
    assert_eq!(first.1["receipt"], second.1["receipt"]);
    let original = public(&f, 0);
    let (status, listed) = list(&router, id, key).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed, json!({"migrations":[first.1["receipt"].clone()]}));
    assert_eq!(
        list(&router, other, other_key).await.1,
        json!({"migrations":[]})
    );
    let changed = json!({"version":1,"label":"initial","sql":"DROP TABLE t"});
    assert_eq!(
        operation(&router, id, key, changed).await,
        (
            StatusCode::BAD_REQUEST,
            json!({"code":"migration_rejected"})
        )
    );
    assert_eq!(public(&f, 0), original);
    assert_eq!(public(&f, 1), sibling);
    assert_eq!(wal(&f), before);
}

#[tokio::test]
async fn migration_body_wait_rechecks_rotation_before_decode_and_invalid_requests_never_write_history()
 {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let before = wal(&f);
    let original = public(&f, 0);
    let (release, wait) = tokio::sync::oneshot::channel::<Bytes>();
    let body = Body::from_stream(futures_util::stream::once(async move {
        Ok::<_, std::io::Error>(wait.await.unwrap())
    }));
    let path = format!("/v1/projects/{id}/migrations/apply");
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
    release
        .send(Bytes::from_static(b"synthetic-sensitive-invalid-json"))
        .unwrap();
    let denied = pending.await.unwrap().unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(denied.headers()[header::CACHE_CONTROL], "no-store");
    let key = rotated["api_key"].as_str().unwrap();
    for input in [
        "{}".to_string(),
        "synthetic-sensitive-invalid-json".into(),
        " ".repeat(65537),
        r#"{"version":1,"label":"a","sql":"DROP TABLE t","path":"other"}"#.into(),
    ] {
        let (status, error) = send(&router, request(&path, key, Body::from(input))).await;
        assert!(matches!(
            status,
            StatusCode::BAD_REQUEST | StatusCode::PAYLOAD_TOO_LARGE
        ));
        assert!(!error.to_string().contains("sensitive"));
    }
    assert_eq!(list(&router, id, key).await.1, json!({"migrations":[]}));
    assert_eq!(wal(&f), before);
    assert_eq!(public(&f, 0), original);
}
