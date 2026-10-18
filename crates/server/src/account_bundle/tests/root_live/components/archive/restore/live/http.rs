use super::*;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn call(
    address: std::net::SocketAddr,
    route: &str,
    key: &str,
    sql: &str,
) -> (u16, serde_json::Value) {
    let body = serde_json::to_vec(&serde_json::json!({"sql":sql})).unwrap();
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let header = format!(
        "POST {route} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(header.as_bytes()).await.unwrap();
    socket.write_all(&body).await.unwrap();
    let mut bytes = Vec::new();
    socket.take(65536).read_to_end(&mut bytes).await.unwrap();
    let split = bytes.windows(4).position(|b| b == b"\r\n\r\n").unwrap();
    let status = std::str::from_utf8(&bytes[..split])
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    (status, serde_json::from_slice(&bytes[split + 4..]).unwrap())
}
#[test]
fn explicitly_admitted_v2_root_serves_real_scoped_http_and_recovers_after_graceful_restart() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, compact);
        let (id, key) = &s.root.credentials[0];
        let root_manifest = manifest(&path);
        let master = "a".repeat(64);
        for restart in [false, true] {
            let root = AccountRoot::open_v2(&path, pool()).unwrap();
            let app = crate::account_router(root, &master).unwrap();
            runtime.block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let (send, receive) = tokio::sync::oneshot::channel();
                let task = tokio::spawn(crate::serve(listener, app, async {
                    let _ = receive.await;
                }));
                let route = format!("/v1/projects/{id}/sql");
                tokio::time::timeout(Duration::from_secs(10), async {
                    assert_eq!(
                        call(address, &route, &master, "SELECT v FROM t WHERE id=1")
                            .await
                            .0,
                        401
                    );
                    if !restart {
                        assert_eq!(
                            call(address, &route, key, "UPDATE t SET v=222 WHERE id=1")
                                .await
                                .0,
                            200
                        );
                    }
                    let (status, value) =
                        call(address, &route, key, "SELECT v FROM t WHERE id=1").await;
                    assert_eq!(status, 200);
                    assert_eq!(value["results"][0]["rows"][0][0]["value"], 222);
                    assert!(
                        NativeFileRoot::open(path.join("files").join(id), id.parse().unwrap())
                            .is_err()
                    );
                })
                .await
                .unwrap();
                send.send(()).unwrap();
                tokio::time::timeout(Duration::from_secs(10), task)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
            });
            assert_eq!(manifest(&path), root_manifest);
        }
        let root = AccountRoot::open_v2(&path, pool()).unwrap();
        drop(root);
        verify_source(&s);
    }
}
