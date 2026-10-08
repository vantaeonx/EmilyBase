use super::*;
fn public_wal(f: &Fixture, index: usize) -> Vec<u8> {
    fs::read(
        f.path
            .join("registry")
            .join(&f.credentials[index].0)
            .join("data/redo.wal"),
    )
    .unwrap()
}
async fn transfer(
    router: &Router,
    id: &str,
    key: &str,
    operation: &str,
    input: Value,
) -> (StatusCode, Value) {
    send(
        router,
        request(
            &format!("/v1/projects/{id}/tables/{operation}"),
            key,
            Body::from(input.to_string()),
        ),
    )
    .await
}
#[tokio::test]
async fn scoped_transfer_preserves_private_history_and_denies_master_sibling_and_session_keys() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let (other, other_key) = &f.credentials[1];
    let pair = sign(&router, id, key).await;
    let before = wal(&f);
    let source = public_wal(&f, 0);
    f.clock.store(0, Ordering::SeqCst);
    let (status, mut document) = transfer(&router, id, key, "export", json!({"table":"t"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(document["rows"].as_array().unwrap().len(), 1);
    for wrong in [
        MASTER,
        other_key,
        pair["access_token"].as_str().unwrap(),
        pair["refresh_token"].as_str().unwrap(),
    ] {
        assert_eq!(
            transfer(
                &router,
                id,
                wrong,
                "import",
                json!({"bad":"synthetic-private"})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    document["schema"]["name"] = json!("copied");
    let (status, report) = transfer(&router, other, other_key, "import", document.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["transfer"]["rows"], 1);
    assert!(report["transaction"].as_u64().unwrap() > 1);
    let after = public_wal(&f, 1);
    assert_eq!(
        transfer(&router, other, other_key, "import", document.clone())
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(public_wal(&f, 1), after);
    assert_eq!(
        transfer(
            &router,
            other,
            other_key,
            "export",
            json!({"table":"copied"})
        )
        .await,
        (StatusCode::OK, document)
    );
    assert_eq!(
        transfer(&router, id, key, "export", json!({"table":"copied"}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(public_wal(&f, 0), source);
    assert_eq!(wal(&f), before);
}
#[tokio::test]
async fn transfer_rechecks_current_key_after_body_wait_and_refuses_invalid_bodies_without_private_writes()
 {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for operation in ["export", "import"] {
        let f = fixture();
        let router = routes_app(f.app.clone());
        let (id, key) = &f.credentials[0];
        let private_before = wal(&f);
        let public_before = public_wal(&f, 0);
        let (_, mut document) = transfer(&router, id, key, "export", json!({"table":"t"})).await;
        document["schema"]["name"] = json!("never_published");
        let payload = if operation == "export" {
            json!({"table":"t"})
        } else {
            document
        };
        let (release, wait) = tokio::sync::oneshot::channel::<Bytes>();
        let body = Body::from_stream(futures_util::stream::once(async move {
            Ok::<_, std::io::Error>(wait.await.unwrap())
        }));
        let path = format!("/v1/projects/{id}/tables/{operation}");
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
        let response = pending.await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let current = rotated["api_key"].as_str().unwrap();
        for input in [
            "{}".to_string(),
            "{\"table\":\"t\",\"table\":\"t\"}".into(),
            "synthetic-private-value".into(),
            " ".repeat(crate::http::MAX_BODY + 1),
        ] {
            let (status, error) = send(&router, request(&path, current, Body::from(input))).await;
            assert!(matches!(
                status,
                StatusCode::BAD_REQUEST | StatusCode::PAYLOAD_TOO_LARGE
            ));
            assert!(!error.to_string().contains("synthetic-private"));
        }
        assert_eq!(
            transfer(
                &router,
                id,
                current,
                "export",
                json!({"table":"never_published"})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(public_wal(&f, 0), public_before);
        assert_eq!(wal(&f), private_before);
    }
}
