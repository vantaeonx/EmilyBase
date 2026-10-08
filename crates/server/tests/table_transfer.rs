#[allow(dead_code)]
mod support;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use emilybase_server::{ProjectStore, router};
use serde_json::{Value, json};
use std::fs;
use std::net::SocketAddr;
use tower::ServiceExt;
const MASTER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
// Keep file-owner assertions outside the sibling case's fork/exec window.
static CASES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
async fn call(app: &axum::Router, path: &str, key: &str, input: Vec<u8>) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("authorization", format!("Bearer {key}"))
        .header("content-type", "application/json")
        .body(Body::from(input))
        .unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()));
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["pragma"], "no-cache");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
fn fixture(
    root: &std::path::Path,
    compact: bool,
) -> (ProjectStore, String, String, String, String) {
    let mut store = ProjectStore::open(root).unwrap();
    let a = store.create("synthetic-source").unwrap();
    let b = store.create("synthetic-copy").unwrap();
    store.authorize(&a.project.id,&a.api_key).unwrap().execute("CREATE TABLE t(id INT PRIMARY KEY,v TEXT);INSERT INTO t VALUES(2,'synthetic-界');INSERT INTO t VALUES(1,'synthetic-Привет')",&[]).unwrap();
    let data = root.join(&a.project.id).join("data");
    if compact {
        emilybase_transactions::Database::open(data)
            .unwrap()
            .compact()
            .unwrap();
    }
    (store, a.project.id, a.api_key, b.project.id, b.api_key)
}
#[tokio::test]
async fn legacy_http_transfer_is_scoped_atomic_bounded_and_preserves_source_wal() {
    let _case = CASES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("registry");
    let (store, a, ka, b, kb) = fixture(&root, false);
    let sql = "CREATE TABLE huge(id INT PRIMARY KEY,v TEXT);".to_string()
        + &(0..32)
            .map(|i| format!("INSERT INTO huge VALUES({i},$1);"))
            .collect::<String>();
    store
        .authorize(&a, &ka)
        .unwrap()
        .execute(
            &sql,
            &[emilybase_catalog::Value::Text(
                "synthetic-private-large".repeat(95),
            )],
        )
        .unwrap();
    let app = router(store, MASTER).unwrap();
    let export = format!("/v1/projects/{a}/tables/export");
    let import = format!("/v1/projects/{b}/tables/import");
    let source = fs::read(root.join(&a).join("data/redo.wal")).unwrap();
    assert_eq!(
        call(
            &app,
            &export,
            &ka,
            serde_json::to_vec(&json!({"table":"huge"})).unwrap()
        )
        .await,
        (StatusCode::BAD_REQUEST, json!({"code":"transfer_rejected"}))
    );
    let (status, data) = call(
        &app,
        &export,
        &ka,
        serde_json::to_vec(&json!({"table":"t"})).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(data["rows"][0][0]["value"], 1);
    for key in [MASTER, &ka] {
        assert_eq!(
            call(&app, &import, key, b"invalid-private-input".to_vec())
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (status, report) = call(&app, &import, &kb, serde_json::to_vec(&data).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["transaction"], 2);
    assert_eq!(report["transfer"]["rows"], 2);
    let target = fs::read(root.join(&b).join("data/redo.wal")).unwrap();
    for input in [
        serde_json::to_vec(&data).unwrap(),
        b"{}".to_vec(),
        vec![b' '; 65537],
    ] {
        let (status, _) = call(&app, &import, &kb, input).await;
        assert!(matches!(
            status,
            StatusCode::BAD_REQUEST | StatusCode::PAYLOAD_TOO_LARGE
        ));
        assert_eq!(
            fs::read(root.join(&b).join("data/redo.wal")).unwrap(),
            target
        );
    }
    assert_eq!(
        fs::read(root.join(&a).join("data/redo.wal")).unwrap(),
        source
    );
    drop(app);
    let store = ProjectStore::open(root).unwrap();
    assert_eq!(
        store
            .authorize(&b, &kb)
            .unwrap()
            .execute("SELECT * FROM t", &[])
            .unwrap()
            .results[0]
            .rows
            .len(),
        2
    );
}
#[test]
fn real_legacy_tcp_import_ack_survives_kill_and_reopen_on_both_wals() {
    let _case = CASES.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    for compact in [false, true] {
        let root = dir.path().join(format!("registry-{compact}"));
        let (store, a, ka, b, kb) = fixture(&root, compact);
        drop(store);
        if compact {
            emilybase_transactions::Database::open(root.join(&b).join("data"))
                .unwrap()
                .compact()
                .unwrap();
        }
        let source = fs::read(root.join(&a).join("data/redo.wal")).unwrap();
        let server = support::Server::start(&root, MASTER);
        let (status, data) = support::call(
            server.address,
            "POST",
            &format!("/v1/projects/{a}/tables/export"),
            &ka,
            &json!({"table":"t"}),
        )
        .unwrap();
        assert_eq!(status, 200);
        let (status, report) = support::call(
            server.address,
            "POST",
            &format!("/v1/projects/{b}/tables/import"),
            &kb,
            &data,
        )
        .unwrap();
        assert_eq!(status, 200);
        assert_eq!(report["transaction"], 2);
        let first = server.kill();
        let server = support::Server::start(&root, MASTER);
        let (status, reopened) = support::call(
            server.address,
            "POST",
            &format!("/v1/projects/{b}/tables/export"),
            &kb,
            &json!({"table":"t"}),
        )
        .unwrap();
        assert_eq!(status, 200);
        assert_eq!(reopened, data);
        let second = server.stop();
        assert_eq!(
            fs::read(root.join(&a).join("data/redo.wal")).unwrap(),
            source
        );
        for secret in [&a, &ka, &b, &kb, MASTER, "synthetic-界", "synthetic-Привет"] {
            assert!(!first.contains(secret));
            assert!(!second.contains(secret));
        }
    }
}
