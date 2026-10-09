use super::public_sessions::{delayed, enable, public_request};
use super::user_data::{OWN, integer, prepare, public, row};
use super::*;
const DENY: &[u8] = br#"{"version":1,"select":{"kind":"deny"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}}"#;

async fn op(
    router: &Router,
    id: &str,
    access: &str,
    kind: &str,
    body: Value,
) -> (StatusCode, Value) {
    send(
        router,
        public_request(
            id,
            &format!("rows/{kind}"),
            Some(access),
            Body::from(body.to_string()),
        ),
    )
    .await
}
fn packet(owner: &[u8], pk: i64, amount: i64) -> Value {
    json!({"table":"owned","operations":[{"op":"insert","row":row(pk,owner,amount)}]})
}
async fn start(f: &Fixture) -> [emilybase_auth::accounts::IssuedSession; 2] {
    let pair = prepare(f).await;
    enable(f, 0).await;
    pair
}

#[tokio::test]
async fn public_owned_rows_pages_and_atomic_late_refusal_keep_hidden_users_and_unrelated_histories()
{
    let _serial = durability::PROCESS_TESTS.lock().await;
    for compact in [false, true] {
        let f = fixture_with_wal(compact);
        let [a, b] = start(&f).await;
        let router = routes_app(f.app.clone());
        let (id, _) = &f.credentials[0];
        let private = wal(&f);
        let sibling = fs::read(
            f.path
                .join("registry")
                .join(&f.credentials[1].0)
                .join("data/redo.wal"),
        )
        .unwrap();
        let write = json!({"table":"owned","operations":[{"op":"insert","row":row(1,&a.metadata.user,10)},{"op":"update","key":integer(1),"row":row(1,&a.metadata.user,20)}]});
        let (status, receipt) = op(&router, id, a.access.expose(), "write", write).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(receipt["changed"], 2);
        assert!(receipt["transaction"].is_string());
        assert_eq!(
            op(
                &router,
                id,
                b.access.expose(),
                "write",
                packet(&b.metadata.user, 2, 30)
            )
            .await
            .0,
            StatusCode::OK
        );
        let before = public(&f);
        assert_eq!(
            op(
                &router,
                id,
                a.access.expose(),
                "get",
                json!({"table":"owned","key":integer(1)})
            )
            .await,
            (StatusCode::OK, json!({"row":row(1,&a.metadata.user,20)}))
        );
        for pk in [1, 999] {
            assert_eq!(
                op(
                    &router,
                    id,
                    b.access.expose(),
                    "get",
                    json!({"table":"owned","key":integer(pk)})
                )
                .await,
                (StatusCode::OK, json!({"row":null}))
            );
        }
        assert_eq!(
            op(
                &router,
                id,
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
        assert_eq!(
            op(
                &router,
                id,
                b.access.expose(),
                "page",
                json!({"table":"owned","limit":128,"after":null})
            )
            .await,
            (
                StatusCode::OK,
                json!({"rows":[row(2,&b.metadata.user,30)],"next":null})
            )
        );
        let denied = json!({"table":"owned","operations":[{"op":"insert","row":row(3,&a.metadata.user,1)},{"op":"update","key":integer(1),"row":row(1,&b.metadata.user,99)}]});
        assert_eq!(
            op(&router, id, a.access.expose(), "write", denied).await,
            (StatusCode::FORBIDDEN, json!({"code":"user_row_rejected"}))
        );
        assert_eq!(public(&f), before);
        for body in [
            packet(&a.metadata.user, 1, 88),
            json!({"table":"owned","operations":[{"op":"delete","key":integer(2)}]}),
        ] {
            assert_eq!(
                op(&router, id, a.access.expose(), "write", body).await.0,
                StatusCode::FORBIDDEN
            );
            assert_eq!(public(&f), before);
        }
        assert_eq!(op(&router,id,a.access.expose(),"write",json!({"table":"owned","operations":[{"op":"update","key":integer(1),"row":row(1,&a.metadata.user,40)},{"op":"delete","key":integer(1)}]})).await.0,StatusCode::OK);
        assert_eq!(
            op(
                &router,
                id,
                a.access.expose(),
                "get",
                json!({"table":"owned","key":integer(1)})
            )
            .await,
            (StatusCode::OK, json!({"row":null}))
        );
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
async fn public_row_credentials_precede_data_metadata_and_never_gain_service_or_ledger_authority() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let [a, _] = start(&f).await;
    enable(&f, 1).await;
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    let before = public(&f);
    let private = wal(&f);
    let foreign = {
        let (other, key) = &f.credentials[1];
        f.app
            .root
            .lock()
            .await
            .sign_in(other, key, "synthetic_user", PASSWORD.as_bytes(), 50)
            .unwrap()
    };
    for token in [
        key.as_str(),
        MASTER,
        a.refresh.expose(),
        foreign.access.expose(),
        &"x".repeat(102),
    ] {
        assert_eq!(
            op(
                &router,
                id,
                token,
                "get",
                json!({"table":"owned","key":integer(1)})
            )
            .await,
            (StatusCode::UNAUTHORIZED, json!({"code":"access_denied"}))
        );
    }
    let data =
        emilybase_transactions::Database::open(f.path.join("registry").join(id).join("data"))
            .unwrap();
    for table in ["owned", "missing"] {
        assert_eq!(
            op(
                &router,
                id,
                &"x".repeat(102),
                "get",
                json!({"table":table,"key":integer(1)})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    drop(data);
    assert_eq!(public(&f), before);
    assert_eq!(
        op(
            &router,
            id,
            a.access.expose(),
            "get",
            json!({"table":"t","key":integer(1)})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        op(
            &router,
            id,
            a.access.expose(),
            "get",
            json!({"table":emilybase_migrations::LEDGER_TABLE,"key":integer(1)})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut request = public_request(
        id,
        "rows/get",
        None,
        Body::from(json!({"table":"owned","key":integer(1)}).to_string()),
    );
    request.headers_mut().insert(
        crate::USER_ACCESS_HEADER,
        a.access.expose().parse().unwrap(),
    );
    assert_eq!(send(&router, request).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        call(
            &router,
            id,
            a.access.expose(),
            "rows/get",
            json!({"table":"owned","key":integer(1)})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(public(&f), before);
    assert_eq!(wal(&f)[0], private[0]);
}

#[tokio::test]
async fn public_row_body_wait_rechecks_current_policy_flag_and_credential_before_whole_packet_commit()
 {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for compact in [false, true] {
        let f = fixture_with_wal(compact);
        let [a, _] = start(&f).await;
        let router = routes_app(f.app.clone());
        let (id, key) = &f.credentials[0];
        assert_eq!(
            op(
                &router,
                id,
                a.access.expose(),
                "write",
                packet(&a.metadata.user, 1, 10)
            )
            .await
            .0,
            StatusCode::OK
        );
        let before = public(&f);
        let (release, pending) = delayed(&router, id, "rows/write", Some(a.access.expose())).await;
        {
            let mut root = f.app.root.lock().await;
            let current = root.row_policy_receipts(id, key).unwrap().remove(0);
            root.install_row_policy(id, key, "owned", current.revision, DENY)
                .unwrap();
        }
        let denied = wal(&f);
        release
            .send(Bytes::from(packet(&a.metadata.user, 2, 20).to_string()))
            .unwrap();
        assert_eq!(pending.await.unwrap().status(), StatusCode::FORBIDDEN);
        assert_eq!(public(&f), before);
        assert_eq!(wal(&f), denied);
        {
            let mut root = f.app.root.lock().await;
            let current = root.row_policy_receipts(id, key).unwrap().remove(0);
            root.install_row_policy(id, key, "owned", current.revision, OWN)
                .unwrap();
        }
        let (release, pending) = delayed(&router, id, "rows/write", Some(a.access.expose())).await;
        {
            let mut root = f.app.root.lock().await;
            let current = root.public_admission(id, key).unwrap();
            root.set_public_admission(id, key, current.revision, false)
                .unwrap();
        }
        let shut = wal(&f);
        release
            .send(Bytes::from_static(b"invalid-private-body"))
            .unwrap();
        assert_eq!(pending.await.unwrap().status(), StatusCode::UNAUTHORIZED);
        assert_eq!(public(&f), before);
        assert_eq!(wal(&f), shut);
        {
            let mut root = f.app.root.lock().await;
            let current = root.public_admission(id, key).unwrap();
            root.set_public_admission(id, key, current.revision, true)
                .unwrap();
        }
        let (release, pending) = delayed(&router, id, "rows/write", Some(a.access.expose())).await;
        f.app
            .root
            .lock()
            .await
            .set_disabled(id, key, "synthetic_user", true)
            .unwrap();
        let disabled = wal(&f);
        release
            .send(Bytes::from(packet(&a.metadata.user, 2, 20).to_string()))
            .unwrap();
        assert_eq!(pending.await.unwrap().status(), StatusCode::UNAUTHORIZED);
        assert_eq!(public(&f), before);
        assert_eq!(wal(&f), disabled);
    }
}

#[tokio::test]
async fn public_row_wire_preserves_signed_limits_and_rejects_outer_arrays_bad_types_duplicates_and_bounds()
 {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    let [a, _] = start(&f).await;
    let router = routes_app(f.app.clone());
    let (id, _) = &f.credentials[0];
    assert_eq!(
        op(
            &router,
            id,
            a.access.expose(),
            "write",
            packet(&a.metadata.user, i64::MIN, i64::MAX)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        op(
            &router,
            id,
            a.access.expose(),
            "get",
            json!({"table":"owned","key":integer(i64::MIN)})
        )
        .await,
        (
            StatusCode::OK,
            json!({"row":row(i64::MIN,&a.metadata.user,i64::MAX)})
        )
    );
    let before = public(&f);
    let private = wal(&f);
    for (kind, body) in [
        ("get", json!(["owned", integer(1)])),
        ("page", json!(["owned", null, 1])),
        ("write", json!(["owned",[{"op":"delete","key":integer(1)}]])),
        (
            "get",
            json!({"table":"owned","key":{"type":"integer","value":1}}),
        ),
        (
            "get",
            json!({"table":"owned","key":{"type":"integer","value":"9223372036854775808"}}),
        ),
        (
            "get",
            json!({"table":"owned","key":{"type":"text","value":"private"}}),
        ),
        ("page", json!({"table":"owned","limit":0})),
        ("page", json!({"table":"owned","limit":129})),
        ("write", json!({"table":"owned","operations":[]})),
        (
            "write",
            json!({"table":"owned","operations":vec![json!({"op":"delete","key":integer(1)});257]}),
        ),
    ] {
        assert_eq!(
            op(&router, id, a.access.expose(), kind, body).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    for (bytes, expected) in [
        (
            br#"{"table":"owned","table":"private","key":{"type":"integer","value":"1"}}"#.to_vec(),
            StatusCode::BAD_REQUEST,
        ),
        (vec![b' '; 65_536], StatusCode::BAD_REQUEST),
        (vec![b' '; 65_537], StatusCode::PAYLOAD_TOO_LARGE),
    ] {
        let result = send(
            &router,
            public_request(id, "rows/get", Some(a.access.expose()), Body::from(bytes)),
        )
        .await;
        assert_eq!(result.0, expected);
        assert!(!result.1.to_string().contains("private"));
    }
    let mut request = public_request(id, "rows/get", Some(a.access.expose()), Body::from("{}"));
    request.headers_mut().append(
        header::AUTHORIZATION,
        format!("Bearer {}", a.access.expose()).parse().unwrap(),
    );
    assert_eq!(send(&router, request).await.0, StatusCode::UNAUTHORIZED);
    let mut request = public_request(id, "rows/get", Some(a.access.expose()), Body::from("{}"));
    request.headers_mut().remove(header::CONTENT_TYPE);
    assert_eq!(
        send(&router, request).await.0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(public(&f), before);
    assert_eq!(wal(&f), private);
}

#[tokio::test]
async fn concurrent_public_row_writes_have_one_durable_winner_and_foreign_user_cannot_observe_it() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for compact in [false, true] {
        let f = fixture_with_wal(compact);
        let [a, b] = start(&f).await;
        let router = routes_app(f.app.clone());
        let (id, _) = &f.credentials[0];
        let private = wal(&f);
        let (first, second) = tokio::join!(
            op(
                &router,
                id,
                a.access.expose(),
                "write",
                packet(&a.metadata.user, 1, 10)
            ),
            op(
                &router,
                id,
                b.access.expose(),
                "write",
                packet(&b.metadata.user, 1, 20)
            )
        );
        let mut codes = [first.0, second.0];
        codes.sort();
        assert_eq!(codes, [StatusCode::OK, StatusCode::FORBIDDEN]);
        let (winner, loser, amount) = if first.0 == StatusCode::OK {
            (&a, &b, 10)
        } else {
            (&b, &a, 20)
        };
        assert_eq!(
            op(
                &router,
                id,
                winner.access.expose(),
                "get",
                json!({"table":"owned","key":integer(1)})
            )
            .await,
            (
                StatusCode::OK,
                json!({"row":row(1,&winner.metadata.user,amount)})
            )
        );
        assert_eq!(
            op(
                &router,
                id,
                loser.access.expose(),
                "get",
                json!({"table":"owned","key":integer(1)})
            )
            .await,
            (StatusCode::OK, json!({"row":null}))
        );
        assert_eq!(wal(&f), private);
    }
}
