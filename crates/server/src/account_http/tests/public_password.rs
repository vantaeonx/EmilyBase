use super::public_sessions::{delayed, enable, public_request};
use super::*;

const NEW: &str = "synthetic-replacement-password";
async fn sign(f: &Fixture) -> Value {
    f.app
        .root
        .lock()
        .await
        .public_sign_in(
            &f.credentials[0].0,
            "synthetic_user",
            PASSWORD.as_bytes(),
            50,
        )
        .map(|s| json!({"access":s.access.expose(),"refresh":s.refresh.expose()}))
        .unwrap()
}
fn change(current: &str, replacement: &str) -> Value {
    json!({"current_password":current,"replacement_password":replacement})
}
async fn call(router: &Router, id: &str, access: Option<&str>, body: Value) -> (StatusCode, Value) {
    send(
        router,
        public_request(id, "password", access, Body::from(body.to_string())),
    )
    .await
}
fn history(f: &Fixture) -> Vec<Vec<u8>> {
    let mut bytes = wal(f);
    for (id, _) in &f.credentials {
        bytes.push(fs::read(f.path.join("registry").join(id).join("data/redo.wal")).unwrap());
    }
    bytes
}

#[tokio::test]
async fn public_password_changes_only_authenticated_owner_and_revokes_every_old_family() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for compact in [false, true] {
        let f = fixture_with_wal(compact);
        enable(&f, 0).await;
        let router = routes_app(f.app.clone());
        let (id, key) = &f.credentials[0];
        let pair = sign(&f).await;
        let another = sign(&f).await;
        f.app
            .root
            .lock()
            .await
            .create_user(id, key, "second_user", PASSWORD.as_bytes())
            .unwrap();
        let second = f
            .app
            .root
            .lock()
            .await
            .public_sign_in(id, "second_user", PASSWORD.as_bytes(), 50)
            .unwrap();
        let before = history(&f);
        let (status, info) =
            call(&router, id, pair["access"].as_str(), change(PASSWORD, NEW)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(info["login"], "synthetic_user");
        assert_eq!(info["credential_epoch"], "2");
        assert_eq!(info.as_object().unwrap().len(), 4);
        let mut root = f.app.root.lock().await;
        for p in [&pair, &another] {
            assert!(
                root.public_user(id, p["access"].as_str().unwrap(), 50)
                    .is_err()
            );
            assert!(
                root.public_refresh_session(id, p["refresh"].as_str().unwrap(), 50)
                    .is_err()
            );
        }
        assert_eq!(
            root.public_user(id, second.access.expose(), 50)
                .unwrap()
                .login,
            "second_user"
        );
        assert!(
            root.public_sign_in(id, "synthetic_user", PASSWORD.as_bytes(), 50)
                .is_err()
        );
        assert!(
            root.public_sign_in(id, "synthetic_user", NEW.as_bytes(), 50)
                .is_ok()
        );
        assert!(
            root.sign_in(
                &f.credentials[1].0,
                &f.credentials[1].1,
                "synthetic_user",
                PASSWORD.as_bytes(),
                50
            )
            .is_ok()
        );
        drop(root);
        // Sibling sign-in is itself a deliberate private commit; public WALs stay exact.
        let after = history(&f);
        assert_eq!(&after[2..], &before[2..]);
        for secret in [
            PASSWORD,
            NEW,
            pair["access"].as_str().unwrap(),
            pair["refresh"].as_str().unwrap(),
        ] {
            assert!(!info.to_string().contains(secret));
        }
    }
}

#[tokio::test]
async fn password_route_refuses_identity_overrides_wrong_purpose_and_bounded_malformed_inputs() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    enable(&f, 0).await;
    enable(&f, 1).await;
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let pair = sign(&f).await;
    let token = pair["access"].as_str().unwrap();
    let before = history(&f);
    for access in [
        None,
        Some(MASTER),
        Some(key),
        pair["refresh"].as_str(),
        Some("x"),
    ] {
        assert_eq!(
            call(&router, id, access, change(PASSWORD, NEW)).await,
            (StatusCode::UNAUTHORIZED, json!({"code":"access_denied"}))
        );
    }
    for body in [
        json!({"login":"second_user","current_password":PASSWORD,"replacement_password":NEW}),
        json!({"current_password":PASSWORD,"replacement_password":NEW,"now":50}),
        json!([PASSWORD, NEW]),
        json!({"current_password":PASSWORD}),
        json!({"current_password":1,"replacement_password":NEW}),
    ] {
        assert_eq!(
            call(&router, id, Some(token), body).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    for (body, expected) in [
        (change("wrong", NEW), StatusCode::UNAUTHORIZED),
        (change("", NEW), StatusCode::BAD_REQUEST),
        (change(PASSWORD, ""), StatusCode::BAD_REQUEST),
        (change(PASSWORD, &"x".repeat(1025)), StatusCode::BAD_REQUEST),
    ] {
        let result = call(&router, id, Some(token), body).await;
        assert_eq!(result.0, expected);
        assert_eq!(result.1.as_object().unwrap().len(), 1);
    }
    assert_eq!(
        call(
            &router,
            &f.credentials[1].0,
            Some(token),
            change(PASSWORD, NEW)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    for (body, expected) in [
        (b"{\"current_password\":\"a\",\"current_password\":\"b\",\"replacement_password\":\"c\"}".to_vec(), StatusCode::BAD_REQUEST),
        (vec![b' ';4096], StatusCode::BAD_REQUEST),
        (vec![b' ';4097], StatusCode::PAYLOAD_TOO_LARGE),
    ] {
        let status = send(
            &router,
            public_request(id, "password", Some(token), Body::from(body)),
        )
        .await
        .0;
        assert_eq!(status, expected);
    }
    assert_eq!(history(&f), before);
    assert_eq!(f.app.workers.available_permits(), 4);
}

#[tokio::test]
async fn password_body_wait_rechecks_closed_flag_and_revoked_access_before_password_work() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for closed in [true, false] {
        let f = fixture();
        enable(&f, 0).await;
        let router = routes_app(f.app.clone());
        let (id, key) = &f.credentials[0];
        let pair = sign(&f).await;
        let (release, pending) = delayed(&router, id, "password", pair["access"].as_str()).await;
        {
            let mut root = f.app.root.lock().await;
            if closed {
                let receipt = root.public_admission(id, key).unwrap();
                root.set_public_admission(id, key, receipt.revision, false)
                    .unwrap();
            } else {
                root.set_disabled(id, key, "synthetic_user", true).unwrap();
            }
        }
        let before = history(&f);
        if closed {
            f.clock.store(u64::MAX, Ordering::SeqCst);
        }
        release
            .send(Bytes::from(change(PASSWORD, NEW).to_string()))
            .unwrap();
        let response = pending.await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(history(&f), before);
    }
}

#[tokio::test]
async fn competing_password_changes_with_same_access_have_one_epoch_winner() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for compact in [false, true] {
        let f = fixture_with_wal(compact);
        enable(&f, 0).await;
        let router = routes_app(f.app.clone());
        let (id, _) = &f.credentials[0];
        let pair = sign(&f).await;
        let access = pair["access"].as_str();
        let (a, b) = tokio::join!(
            call(&router, id, access, change(PASSWORD, "synthetic-first")),
            call(&router, id, access, change(PASSWORD, "synthetic-second"))
        );
        let mut statuses = [a.0, b.0];
        statuses.sort();
        assert_eq!(statuses, [StatusCode::OK, StatusCode::UNAUTHORIZED]);
        let expected = if a.0 == StatusCode::OK {
            "synthetic-first"
        } else {
            "synthetic-second"
        };
        let mut root = f.app.root.lock().await;
        let fresh = root
            .public_sign_in(id, "synthetic_user", expected.as_bytes(), 50)
            .unwrap();
        assert_eq!(
            root.public_user(id, fresh.access.expose(), 50)
                .unwrap()
                .credential_epoch,
            2
        );
    }
}

#[test]
fn pure_password_request_grammar_requires_two_secret_fields_and_an_object() {
    use requests::{SessionRequest, validate_session_request};
    for bytes in [
        br#"["a","b"]"#.as_slice(),
        br#"{"login":"other","current_password":"a","replacement_password":"b"}"#,
        br#"{"current_password":"a","replacement_password":"b","replacement_password":"c"}"#,
        br#"{"current_password":1,"replacement_password":"b"}"#,
    ] {
        assert!(validate_session_request(SessionRequest::Password, bytes).is_err());
    }
    assert!(
        validate_session_request(
            SessionRequest::Password,
            br#"{"current_password":"a","replacement_password":"b"}"#
        )
        .is_ok()
    );
}

#[tokio::test]
async fn competing_refresh_and_password_change_preserve_one_current_credential_transition() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for compact in [false, true] {
        for refresh_first in [false, true] {
            let f = fixture_with_wal(compact);
            enable(&f, 0).await;
            let router = routes_app(f.app.clone());
            let id = &f.credentials[0].0;
            let pair = sign(&f).await;
            let refresh = send(
                &router,
                public_request(
                    id,
                    "refresh",
                    None,
                    Body::from(json!({"refresh_token":pair["refresh"]}).to_string()),
                ),
            );
            let password = call(&router, id, pair["access"].as_str(), change(PASSWORD, NEW));
            let (changed, refreshed) = if refresh_first {
                let (refreshed, changed) = tokio::join!(refresh, password);
                (changed, refreshed)
            } else {
                tokio::join!(password, refresh)
            };
            let mut statuses = [changed.0, refreshed.0];
            statuses.sort();
            assert_eq!(statuses, [StatusCode::OK, StatusCode::UNAUTHORIZED]);
            let mut root = f.app.root.lock().await;
            if changed.0 == StatusCode::OK {
                assert_eq!(changed.1["credential_epoch"], "2");
                assert!(
                    root.public_sign_in(id, "synthetic_user", NEW.as_bytes(), 50)
                        .is_ok()
                );
                assert!(
                    root.public_sign_in(id, "synthetic_user", PASSWORD.as_bytes(), 50)
                        .is_err()
                );
            } else {
                assert_eq!(
                    root.public_user(id, refreshed.1["access_token"].as_str().unwrap(), 50)
                        .unwrap()
                        .credential_epoch,
                    1
                );
                assert!(
                    root.public_sign_in(id, "synthetic_user", PASSWORD.as_bytes(), 50)
                        .is_ok()
                );
                assert!(
                    root.public_sign_in(id, "synthetic_user", NEW.as_bytes(), 50)
                        .is_err()
                );
            }
            assert!(
                root.public_user(id, pair["access"].as_str().unwrap(), 50)
                    .is_err()
            );
        }
    }
}
