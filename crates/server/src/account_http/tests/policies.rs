use super::*;
const DENY: &str = r#"{"version":1,"select":{"kind":"deny"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}}"#;
async fn operation(
    router: &Router,
    id: &str,
    key: &str,
    op: &str,
    input: Value,
) -> (StatusCode, Value) {
    call(router, id, key, &format!("policies/{op}"), input).await
}
async fn list(router: &Router, id: &str, key: &str) -> (StatusCode, Value) {
    let mut r = request(
        &format!("/v1/projects/{id}/auth/policies"),
        key,
        Body::empty(),
    );
    *r.method_mut() = axum::http::Method::GET;
    send(router, r).await
}
fn install(expected: &str, document: &str) -> Value {
    json!({"table":"t","expected":expected,"document":document})
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
async fn policy_http_explicit_migration_and_exact_install_retries_preserve_clock_public_data_and_sibling_scope_on_both_wals()
 {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for compact in [false, true] {
        let f = fixture_with_wal(compact);
        let router = routes_app(f.app.clone());
        let (id, key) = &f.credentials[0];
        let (other, other_key) = &f.credentials[1];
        let pair = sign(&router, id, key).await;
        let before = wal(&f);
        let original = public(&f, 0);
        let sibling = public(&f, 1);
        f.clock.store(0, Ordering::SeqCst);
        assert_eq!(
            list(&router, id, key).await,
            (
                StatusCode::CONFLICT,
                json!({"code":"policy_catalog_disabled"})
            )
        );
        for wrong in [
            MASTER,
            other_key.as_str(),
            pair["access_token"].as_str().unwrap(),
            pair["refresh_token"].as_str().unwrap(),
        ] {
            assert_eq!(list(&router, id, wrong).await.0, StatusCode::UNAUTHORIZED);
            for (op, body) in [("enable", json!({})), ("install", install("0", DENY))] {
                assert_eq!(
                    operation(&router, id, wrong, op, body).await.0,
                    StatusCode::UNAUTHORIZED
                );
            }
        }
        assert_eq!(wal(&f), before);
        assert_eq!(
            operation(&router, id, key, "enable", json!({})).await,
            (StatusCode::OK, json!({"private_version":4}))
        );
        let enabled = wal(&f);
        assert_eq!(
            operation(&router, id, key, "enable", json!({})).await.0,
            StatusCode::OK
        );
        assert_eq!(wal(&f), enabled);
        let mut long = DENY.to_owned();
        long.push_str(&" ".repeat(16_384 - long.len()));
        let (status, installed) = operation(&router, id, key, "install", install("0", &long)).await;
        assert_eq!(status, StatusCode::OK);
        let receipt = &installed["receipt"];
        assert!(receipt["table"].is_string());
        assert!(receipt["revision"].is_string());
        assert_eq!(receipt["previous"], "0");
        assert_eq!(receipt["sha256"].as_str().unwrap().len(), 64);
        let image = emilybase_backup::encode(&wal(&f)[0]).unwrap();
        let report =
            emilybase_auth::accounts::inspect_private_account_backup_bytes(&image, id).unwrap();
        assert_eq!(report.private_version, 4);
        assert_eq!(report.clock_floor, Some(50));
        assert_eq!(
            receipt["revision"],
            report.database.last_transaction.to_string()
        );
        let installed_wal = wal(&f);
        assert_eq!(
            operation(&router, id, key, "install", install("0", &long))
                .await
                .1,
            installed
        );
        assert_eq!(
            operation(
                &router,
                id,
                key,
                "install",
                install(receipt["revision"].as_str().unwrap(), &long)
            )
            .await
            .1,
            installed
        );
        assert_eq!(
            list(&router, id, key).await.1,
            json!({"policies":[receipt.clone()]})
        );
        assert_eq!(
            list(&router, other, other_key).await.0,
            StatusCode::CONFLICT
        );
        assert_eq!(wal(&f), installed_wal);
        assert_eq!(wal(&f)[1], before[1]);
        assert_eq!(public(&f, 0), original);
        assert_eq!(public(&f, 1), sibling);
    }
}
#[tokio::test]
async fn policy_http_waiting_body_rechecks_current_key_before_decode_for_enable_and_install() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for op in ["enable", "install"] {
        let f = fixture();
        let router = routes_app(f.app.clone());
        let (id, key) = &f.credentials[0];
        if op == "install" {
            assert_eq!(
                operation(&router, id, key, "enable", json!({})).await.0,
                StatusCode::OK
            );
        }
        let before = wal(&f);
        let original = public(&f, 0);
        let (release, wait) = tokio::sync::oneshot::channel::<Bytes>();
        let body = Body::from_stream(futures_util::stream::once(async move {
            Ok::<_, std::io::Error>(wait.await.unwrap())
        }));
        let path = format!("/v1/projects/{id}/auth/policies/{op}");
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
            .send(Bytes::from_static(
                b"synthetic-private-malformed-definition",
            ))
            .unwrap();
        let denied = pending.await.unwrap().unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(denied.headers()[header::CACHE_CONTROL], "no-store");
        let key = rotated["api_key"].as_str().unwrap();
        for bad in [
            json!({"now":100}),
            json!({"project":id}),
            json!({"schema":{}}),
        ] {
            assert_eq!(
                operation(&router, id, key, "enable", bad).await.0,
                StatusCode::BAD_REQUEST
            );
        }
        for bad in [
            install("00", DENY),
            install("0", "synthetic-private-definition"),
            json!({"table":"t","expected":"0","document":DENY,"schema":{}}),
        ] {
            assert_eq!(
                operation(&router, id, key, "install", bad).await.0,
                StatusCode::BAD_REQUEST
            );
        }
        let huge = send(
            &router,
            request(
                &path,
                key,
                Body::from(vec![b' '; crate::http::MAX_BODY + 1]),
            ),
        )
        .await;
        assert_eq!(huge.0, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(wal(&f), before);
        assert_eq!(public(&f, 0), original);
    }
}
#[tokio::test]
async fn policy_http_concurrent_exact_retries_share_receipt_and_changed_stale_revisions_never_write()
 {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    assert_eq!(
        operation(&router, id, key, "enable", json!({})).await.0,
        StatusCode::OK
    );
    let (a, b) = tokio::join!(
        operation(&router, id, key, "install", install("0", DENY)),
        operation(&router, id, key, "install", install("0", DENY))
    );
    assert_eq!(a.0, StatusCode::OK);
    assert_eq!(a, b);
    let revision = a.1["receipt"]["revision"].as_str().unwrap();
    let one = format!("{DENY} ");
    let two = format!("{DENY}\n");
    let (a, b) = tokio::join!(
        operation(&router, id, key, "install", install(revision, &one)),
        operation(&router, id, key, "install", install(revision, &two))
    );
    assert_eq!(
        [a.0, b.0].iter().filter(|s| **s == StatusCode::OK).count(),
        1
    );
    assert_eq!(
        [a.0, b.0]
            .iter()
            .filter(|s| **s == StatusCode::CONFLICT)
            .count(),
        1
    );
    let winner = if a.0 == StatusCode::OK { a.1 } else { b.1 };
    assert_eq!(winner["receipt"]["previous"], revision);
    let before = wal(&f);
    assert_eq!(
        operation(&router, id, key, "install", install("0", DENY)).await,
        (
            StatusCode::CONFLICT,
            json!({"code":"policy_revision_conflict"})
        )
    );
    assert_eq!(wal(&f), before);
    assert_eq!(
        list(&router, id, key).await.1,
        json!({"policies":[winner["receipt"].clone()]})
    );
}
#[tokio::test]
async fn policy_http_private_attempt_limit_applies_before_any_policy_write() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let before = wal(&f);
    for _ in 0..PRIVATE_ATTEMPTS {
        assert_eq!(list(&router, id, key).await.0, StatusCode::CONFLICT);
    }
    assert_eq!(
        operation(&router, id, key, "enable", json!({})).await,
        (
            StatusCode::TOO_MANY_REQUESTS,
            json!({"code":"account_rate_limit"})
        )
    );
    assert_eq!(wal(&f), before);
}
