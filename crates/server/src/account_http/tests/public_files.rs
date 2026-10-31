//! Real v2 roots behind the access-only router, with raw TCP and restart checks.
use super::{
    public_sessions::{delayed, public_request},
    *,
};
use crate::{
    AccountFileArchiveReader, AccountFileUpload, FileReadRequest, validate_file_read_request,
};
use emilybase_auth::{
    accounts::IssuedSession,
    file_policy::{CreateRule, Definition, Rule},
};
use emilybase_files::{FileId, FileQuota, NativeFileRoot, initialize_file_root};
use emilybase_object_storage::ObjectId;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;

const A: FileId = FileId::from_bytes([31; 16]);
const B: FileId = FileId::from_bytes([33; 16]);
const ABSENT: FileId = FileId::from_bytes([35; 16]);
const PAYLOAD: &[u8] = b"\x00\xffsynthetic\r\n\x01";
async fn install(f: &Fixture, index: usize, read: Rule) {
    let (id, key) = &f.credentials[index];
    let mut root = f.app.root.lock().await;
    let expected = root
        .project_file_policy(id, key)
        .unwrap()
        .map_or(0, |p| p.revision);
    root.install_project_file_policy(
        id,
        key,
        expected,
        &serde_json::to_vec(&Definition {
            version: 1,
            read,
            create: CreateRule::SelfOwned {},
            rename: Rule::Owner {},
            remove: Rule::Owner {},
        })
        .unwrap(),
    )
    .unwrap();
}
async fn files(compact: bool) -> (Fixture, [IssuedSession; 3]) {
    let mut f = fixture_with_wal(compact);
    fs::set_permissions(f._dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let directory = f._dir.path().join("source-files");
    fs::create_dir(&directory).unwrap();
    let mut native = Vec::new();
    let mut projects = f.credentials.iter().map(|(id, _)| id).collect::<Vec<_>>();
    projects.sort();
    for id in projects {
        let path = directory.join(id);
        initialize_file_root(
            &path,
            id.parse().unwrap(),
            FileQuota::new(128, 65536).unwrap(),
        )
        .unwrap();
        if compact {
            emilybase_transactions::Database::open(path.join("metadata"))
                .unwrap()
                .compact()
                .unwrap();
        }
        native.push(NativeFileRoot::open(path, id.parse().unwrap()).unwrap());
    }
    let bytes = {
        let mut root = f.app.root.lock().await;
        let snapshot = crate::capture_account_file_components(&mut root, &mut native).unwrap();
        let mut bytes = Vec::new();
        AccountFileArchiveReader::from_snapshot(&snapshot)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    };
    drop(native);
    crate::restore_account_file_archive_bytes_at(
        &bytes,
        &fs::File::open(f._dir.path()).unwrap(),
        "v2",
        PasswordPool::new(1).unwrap(),
        50,
    )
    .unwrap();
    f.path = f._dir.path().join("v2");
    let clock = f.clock.clone();
    f.app = make_app(
        AccountRoot::open_v2(&f.path, PasswordPool::new(1).unwrap()).unwrap(),
        MASTER,
        Arc::new(move || Ok(clock.load(Ordering::SeqCst))),
    )
    .unwrap();
    for (id, key) in &f.credentials {
        let mut root = f.app.root.lock().await;
        root.enable_row_policy_catalog(id, key).unwrap();
        root.enable_public_admission_catalog(id, key).unwrap();
        root.enable_file_policy_catalog(id, key).unwrap();
        let a = root.public_admission(id, key).unwrap();
        root.set_public_admission(id, key, a.revision, true)
            .unwrap();
    }
    install(&f, 0, Rule::Owner {}).await;
    install(&f, 1, Rule::Authenticated {}).await;
    let pair = {
        let (id, key) = &f.credentials[0];
        let (other, other_key) = &f.credentials[1];
        let mut root = f.app.root.lock().await;
        let alice = root
            .sign_in(id, key, "synthetic_user", PASSWORD.as_bytes(), 50)
            .unwrap();
        root.create_user(id, key, "synthetic_bob", PASSWORD.as_bytes())
            .unwrap();
        let bob = root
            .sign_in(id, key, "synthetic_bob", PASSWORD.as_bytes(), 50)
            .unwrap();
        let foreign = root
            .sign_in(other, other_key, "synthetic_user", PASSWORD.as_bytes(), 50)
            .unwrap();
        for (session, file, object, name, payload) in [
            (&alice, A, 32, "alice-visible", PAYLOAD),
            (&bob, B, 34, "bob-hidden", b"bob-private".as_slice()),
        ] {
            root.public_publish_file(
                id,
                session.access.expose(),
                50,
                AccountFileUpload {
                    id: file,
                    object: ObjectId::from_bytes([object; 16]),
                    name,
                    payload,
                },
            )
            .unwrap();
        }
        [alice, bob, foreign]
    };
    (f, pair)
}
fn packet(file: FileId, read: bool) -> Value {
    if read {
        json!({"id":file.to_string(),"maximum_bytes":crate::MAX_ACCOUNT_FILE_READ_BYTES})
    } else {
        json!({"id":file.to_string()})
    }
}
async fn reply(router: &Router, request: Request) -> (StatusCode, axum::http::HeaderMap, Bytes) {
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()[header::PRAGMA], "no-cache");
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, crate::MAX_ACCOUNT_FILE_READ_BYTES + 1024)
        .await
        .unwrap();
    (parts.status, parts.headers, bytes)
}
async fn op(
    router: &Router,
    id: &str,
    access: &str,
    kind: &str,
    body: Value,
) -> (StatusCode, axum::http::HeaderMap, Bytes) {
    reply(
        router,
        public_request(
            id,
            &format!("files/{kind}"),
            Some(access),
            Body::from(body.to_string()),
        ),
    )
    .await
}
fn json_body(bytes: &Bytes) -> Value {
    serde_json::from_slice(bytes).unwrap()
}
fn histories(f: &Fixture) -> Vec<Vec<u8>> {
    history_at(&f.path, &f.credentials)
}
fn history_at(path: &std::path::Path, credentials: &[(String, String)]) -> Vec<Vec<u8>> {
    let mut paths = vec![path.join("root.json")];
    for (id, _) in credentials {
        paths.extend([
            path.join("registry").join(id).join("project.json"),
            path.join("private").join(id).join("redo.wal"),
            path.join("registry").join(id).join("data/redo.wal"),
            path.join("files").join(id).join("metadata/redo.wal"),
        ]);
        paths.extend(
            fs::read_dir(path.join("files").join(id).join("objects"))
                .unwrap()
                .map(|p| p.unwrap().path())
                .filter(|p| p.extension().is_some_and(|e| e == "object")),
        );
    }
    paths.sort();
    paths.iter().map(|p| fs::read(p).unwrap()).collect()
}

#[test]
fn strict_read_grammar_rejects_unknown_duplicate_positional_and_noncanonical_inputs() {
    use FileReadRequest::*;
    for (kind, bytes) in [
        (List, b"{}".as_slice()),
        (Info, format!("{{\"id\":\"{A}\"}}").as_bytes()),
        (
            Read,
            format!("{{\"id\":\"{A}\",\"maximum_bytes\":0}}").as_bytes(),
        ),
    ] {
        assert!(validate_file_read_request(kind, bytes).is_ok());
    }
    for (kind, text) in [
        (List, "[]".into()),
        (List, "null".into()),
        (List, "{}{}".into()),
        (List, "{\"owner\":1}".into()),
        (Info, format!("[\"{A}\"]")),
        (Info, format!("{{\"id\":\"{A}\",\"id\":\"{A}\"}}")),
        (Info, "{\"id\":\"../escape\"}".into()),
        (Info, "{\"id\":\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"}".into()),
        (Info, "{\"id\":1}".into()),
        (Info, "{}".into()),
        (Read, format!("{{\"id\":\"{A}\"}}")),
        (Read, format!("{{\"id\":\"{A}\",\"maximum_bytes\":-1}}")),
        (Read, format!("{{\"id\":\"{A}\",\"maximum_bytes\":1.0}}")),
        (Read, format!("{{\"id\":\"{A}\",\"maximum_bytes\":\"1\"}}")),
        (
            Read,
            format!("{{\"id\":\"{A}\",\"maximum_bytes\":8388609}}"),
        ),
        (
            Read,
            format!("{{\"id\":\"{A}\",\"maximum_bytes\":0,\"owner\":\"caller\"}}"),
        ),
    ] {
        assert!(validate_file_read_request(kind, text.as_bytes()).is_err());
    }
    assert!(validate_file_read_request(List, &[b' '; 4097]).is_err());
    assert!(validate_file_read_request(Info, &[0xff]).is_err());
}

#[tokio::test]
async fn current_users_observe_only_visible_metadata_and_complete_binary_without_internal_ids() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for compact in [false, true] {
        let (f, [a, b, foreign]) = files(compact).await;
        let router = routes_app(f.app.clone());
        let (id, key) = &f.credentials[0];
        let revision = f
            .app
            .root
            .lock()
            .await
            .project_file_info(id, key, A)
            .unwrap()
            .unwrap()
            .revision()
            .to_string();
        let before = histories(&f);
        let (status, headers, bytes) = op(&router, id, a.access.expose(), "list", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "application/json");
        let list = json_body(&bytes);
        assert_eq!(list["files"].as_array().unwrap().len(), 1);
        let meta = &list["files"][0];
        assert_eq!(meta["id"], A.to_string());
        assert_eq!(meta["name"], "alice-visible");
        assert_eq!(meta["payload_bytes"], PAYLOAD.len());
        assert_eq!(meta["revision"], revision);
        assert_eq!(meta.as_object().unwrap().len(), 4);
        let info = op(&router, id, a.access.expose(), "info", packet(A, false)).await;
        assert_eq!(json_body(&info.2), json!({"file":meta}));
        for file in [B, ABSENT] {
            let info = op(&router, id, a.access.expose(), "info", packet(file, false)).await;
            assert_eq!(
                (info.0, json_body(&info.2)),
                (StatusCode::OK, json!({"file":null}))
            );
            let read = op(&router, id, a.access.expose(), "read", packet(file, true)).await;
            assert_eq!(
                (read.0, json_body(&read.2)),
                (StatusCode::FORBIDDEN, json!({"code":"file_read_rejected"}))
            );
        }
        let read = op(&router, id, a.access.expose(), "read", packet(A, true)).await;
        assert_eq!(read.0, StatusCode::OK);
        assert_eq!(read.1[header::CONTENT_TYPE], "application/octet-stream");
        assert_eq!(read.1[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert!(!read.1.contains_key(header::CONTENT_DISPOSITION));
        assert_eq!(read.2.as_ref(), PAYLOAD);
        assert_eq!(
            op(&router, id, b.access.expose(), "read", packet(A, true))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        for token in [
            key.as_str(),
            MASTER,
            a.refresh.expose(),
            foreign.access.expose(),
        ] {
            assert_eq!(
                op(&router, id, token, "read", packet(A, true)).await.0,
                StatusCode::UNAUTHORIZED
            );
        }
        assert_eq!(histories(&f), before);
        install(&f, 0, Rule::Authenticated {}).await;
        assert_eq!(
            json_body(
                &op(&router, id, b.access.expose(), "list", json!({}))
                    .await
                    .2
            )["files"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        install(&f, 0, Rule::Deny {}).await;
        assert_eq!(
            json_body(
                &op(&router, id, a.access.expose(), "list", json!({}))
                    .await
                    .2
            ),
            json!({"files":[]})
        );
    }
}

#[tokio::test]
async fn malformed_headers_and_closed_projects_refuse_without_polling_a_request_body() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let (f, [a, _, _]) = files(false).await;
    let router = routes_app(f.app.clone());
    let (id, key) = &f.credentials[0];
    for kind in ["list", "info", "read"] {
        for variant in 0..5 {
            let polls = Arc::new(AtomicU64::new(0));
            let watched = polls.clone();
            let body = Body::from_stream(futures_util::stream::poll_fn(move |_| {
                watched.fetch_add(1, Ordering::SeqCst);
                std::task::Poll::Ready(Some(Ok::<Bytes, std::io::Error>(Bytes::from_static(
                    b"private",
                ))))
            }));
            let project = if variant == 4 {
                "00000000000000000000000000000000"
            } else {
                id
            };
            let mut req = public_request(
                project,
                &format!("files/{kind}"),
                Some(a.access.expose()),
                body,
            );
            match variant {
                0 => {
                    req.headers_mut().remove(header::AUTHORIZATION);
                }
                1 => {
                    req.headers_mut()
                        .insert(header::AUTHORIZATION, "Bearer short".parse().unwrap());
                }
                2 => {
                    req.headers_mut().append(
                        header::AUTHORIZATION,
                        format!("Bearer {}", a.access.expose()).parse().unwrap(),
                    );
                }
                3 => {
                    let mut root = f.app.root.lock().await;
                    let flag = root.public_admission(id, key).unwrap();
                    root.set_public_admission(id, key, flag.revision, false)
                        .unwrap();
                }
                _ => (),
            }
            let result = reply(&router, req).await;
            assert_eq!(result.0, StatusCode::UNAUTHORIZED);
            assert_eq!(polls.load(Ordering::SeqCst), 0);
            if variant == 3 {
                let mut root = f.app.root.lock().await;
                let flag = root.public_admission(id, key).unwrap();
                root.set_public_admission(id, key, flag.revision, true)
                    .unwrap();
            }
        }
    }
    assert_eq!(f.app.workers.available_permits(), 4);
}

#[tokio::test]
async fn strict_bounded_body_errors_and_download_limits_never_emit_a_partial_payload() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let (f, [a, _, _]) = files(true).await;
    let router = routes_app(f.app.clone());
    let (id, _) = &f.credentials[0];
    let before = histories(&f);
    for (text, status, code) in [
        (
            "[]".into(),
            StatusCode::BAD_REQUEST,
            "file_request_rejected",
        ),
        (
            format!("{{\"id\":\"{A}\",\"maximum_bytes\":1,\"owner\":\"caller\"}}"),
            StatusCode::BAD_REQUEST,
            "file_request_rejected",
        ),
        (
            format!("{{\"id\":\"{A}\",\"maximum_bytes\":8388609}}"),
            StatusCode::BAD_REQUEST,
            "file_request_rejected",
        ),
        (
            " ".repeat(4097),
            StatusCode::PAYLOAD_TOO_LARGE,
            "body_limit",
        ),
        (
            format!("{{\"id\":\"{A}\",\"maximum_bytes\":1}}"),
            StatusCode::PAYLOAD_TOO_LARGE,
            "file_read_limit",
        ),
    ] {
        let result = reply(
            &router,
            public_request(id, "files/read", Some(a.access.expose()), Body::from(text)),
        )
        .await;
        assert_eq!(
            (result.0, json_body(&result.2)),
            (status, json!({"code":code}))
        );
        assert_eq!(result.1[header::CONTENT_TYPE], "application/json");
    }
    for variant in 0..3 {
        let mut req = public_request(
            id,
            "files/read",
            Some(a.access.expose()),
            Body::from(packet(A, true).to_string()),
        );
        match variant {
            0 => {
                req.headers_mut().remove(header::CONTENT_TYPE);
            }
            1 => {
                req.headers_mut()
                    .insert(header::CONTENT_TYPE, "text/plain".parse().unwrap());
            }
            _ => {
                req.headers_mut()
                    .append(header::CONTENT_TYPE, "application/json".parse().unwrap());
            }
        }
        assert_eq!(
            reply(&router, req).await.0,
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
    }
    let mut req = public_request(
        id,
        "files/read",
        Some(a.access.expose()),
        Body::from(packet(A, true).to_string()),
    );
    req.headers_mut()
        .insert(header::RANGE, "bytes=0-0".parse().unwrap());
    let result = reply(&router, req).await;
    assert_eq!(result.0, StatusCode::OK);
    assert_eq!(result.2.as_ref(), PAYLOAD);
    assert!(!result.1.contains_key(header::CONTENT_RANGE));
    assert_eq!(histories(&f), before);
}

#[tokio::test]
async fn current_admission_session_and_installed_policy_are_rechecked_after_body_and_root_waits() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for compact in [false, true] {
        for change in 0..6 {
            let (f, [a, _, _]) = files(compact).await;
            let (id, key) = &f.credentials[0];
            let mut app = f.app.clone();
            if change == 0 {
                app.clock = Arc::new(|| panic!("closed admission consulted trusted time"));
            }
            let router = routes_app(app);
            let (release, pending) =
                delayed(&router, id, "files/read", Some(a.access.expose())).await;
            let mut root = f.app.root.lock().await;
            release
                .send(Bytes::from(packet(A, true).to_string()))
                .unwrap();
            // The body was entered before the owner was taken; either root/body
            // scheduling order must still consume the following current state.
            match change {
                0 => {
                    let flag = root.public_admission(id, key).unwrap();
                    root.set_public_admission(id, key, flag.revision, false)
                        .unwrap();
                }
                1 => root
                    .logout_session(id, key, a.refresh.expose(), 50)
                    .unwrap(),
                2 => {
                    root.refresh_session(id, key, a.refresh.expose(), 50)
                        .unwrap();
                }
                3 => {
                    root.set_disabled(id, key, "synthetic_user", true).unwrap();
                    root.set_disabled(id, key, "synthetic_user", false).unwrap();
                }
                4 => {
                    root.change_password(
                        id,
                        key,
                        "synthetic_user",
                        PASSWORD.as_bytes(),
                        b"synthetic-new-password",
                    )
                    .unwrap();
                }
                _ => {
                    let old = root.project_file_policy(id, key).unwrap().unwrap();
                    let policy = serde_json::to_vec(&Definition {
                        version: 1,
                        read: Rule::Deny {},
                        create: CreateRule::Deny {},
                        rename: Rule::Deny {},
                        remove: Rule::Deny {},
                    })
                    .unwrap();
                    root.install_project_file_policy(id, key, old.revision, &policy)
                        .unwrap();
                }
            }
            drop(root);
            let response = tokio::time::timeout(Duration::from_secs(5), pending)
                .await
                .unwrap()
                .unwrap();
            let status = response.status();
            let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
            assert_eq!(
                status,
                if change == 0 || change == 5 {
                    StatusCode::FORBIDDEN
                } else {
                    StatusCode::UNAUTHORIZED
                }
            );
            assert!(!bytes.windows(PAYLOAD.len()).any(|b| b == PAYLOAD));
            assert_eq!(f.app.workers.available_permits(), 4);
        }
    }
}

#[tokio::test]
async fn actual_source_replacements_during_body_wait_refuse_without_payload_or_wal_changes() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for target in 0..4 {
        let (f, [a, _, _]) = files(true).await;
        let (id, _) = &f.credentials[0];
        let router = routes_app(f.app.clone());
        let before = histories(&f);
        let (release, pending) = delayed(&router, id, "files/read", Some(a.access.expose())).await;
        let selected = match target {
            0 => f.path.join("root.json"),
            1 => f.path.join("private").join(id).join("redo.wal"),
            2 => f.path.join("files").join(id).join("metadata/redo.wal"),
            _ => f
                .path
                .join("files")
                .join(id)
                .join("objects")
                .join(format!("{}.object", ObjectId::from_bytes([32; 16]))),
        };
        let saved = f._dir.path().join("original");
        fs::rename(&selected, &saved).unwrap();
        fs::copy(&saved, &selected).unwrap();
        if target == 3 {
            let mut bytes = fs::read(&selected).unwrap();
            *bytes.last_mut().unwrap() ^= 1;
            fs::write(&selected, bytes).unwrap();
        }
        release
            .send(Bytes::from(packet(A, true).to_string()))
            .unwrap();
        let response = pending.await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        assert!(!bytes.windows(PAYLOAD.len()).any(|b| b == PAYLOAD));
        fs::remove_file(&selected).unwrap();
        fs::rename(&saved, &selected).unwrap();
        assert_eq!(histories(&f), before);
    }
}

#[tokio::test]
async fn stalled_bodies_share_original_worker_budget_and_cancellation_releases_all_permits() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let (f, [a, _, _]) = files(false).await;
    let (id, _) = &f.credentials[0];
    let router = routes_app(f.app.clone());
    let mut waits = Vec::new();
    for _ in 0..4 {
        waits.push(delayed(&router, id, "files/read", Some(a.access.expose())).await);
    }
    assert_eq!(
        op(&router, id, a.access.expose(), "list", json!({}))
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    for (release, pending) in waits {
        pending.abort();
        assert!(pending.await.unwrap_err().is_cancelled());
        drop(release);
    }
    assert_eq!(f.app.workers.available_permits(), 4);
    assert_eq!(
        op(&router, id, a.access.expose(), "read", packet(A, true))
            .await
            .2
            .as_ref(),
        PAYLOAD
    );
    f.app
        .projects
        .lock()
        .unwrap()
        .0
        .insert(id.clone(), (Instant::now(), PRIVATE_ATTEMPTS));
    assert_eq!(
        op(&router, id, a.access.expose(), "list", json!({}))
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(f.app.workers.available_permits(), 4);
}

async fn tcp(
    address: SocketAddr,
    id: &str,
    access: &str,
    kind: &str,
    body: &[u8],
) -> (u16, String, Vec<u8>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream.write_all(format!("POST /v1/projects/{id}/user/files/{kind} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {access}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await.unwrap();
    stream.write_all(body).await.unwrap();
    let mut bytes = Vec::new();
    stream
        .take(10 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .await
        .unwrap();
    let split = bytes.windows(4).position(|b| b == b"\r\n\r\n").unwrap();
    let headers = String::from_utf8(bytes[..split].to_vec()).unwrap();
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, headers, bytes[split + 4..].to_vec())
}
#[tokio::test]
async fn real_tcp_binary_empty_large_denial_and_graceful_restart_preserve_native_histories() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for compact in [false, true] {
        let (f, [a, _, _]) = files(compact).await;
        let (id, key) = &f.credentials[0];
        let big = vec![0xf3; 128 * 1024];
        let empty = FileId::from_bytes([41; 16]);
        let large = FileId::from_bytes([43; 16]);
        {
            let mut root = f.app.root.lock().await;
            let q = root.project_file_quota(id, key).unwrap();
            root.set_project_file_quota(id, key, q, FileQuota::new(128, 1024 * 1024).unwrap())
                .unwrap();
            for (file, object, payload) in
                [(empty, 42, b"".as_slice()), (large, 44, big.as_slice())]
            {
                root.public_publish_file(
                    id,
                    a.access.expose(),
                    50,
                    AccountFileUpload {
                        id: file,
                        object: ObjectId::from_bytes([object; 16]),
                        name: "tcp-file",
                        payload,
                    },
                )
                .unwrap();
            }
        }
        let before = histories(&f);
        let Fixture {
            _dir,
            path,
            credentials,
            app,
            clock,
        } = f;
        let (id, _) = &credentials[0];
        let mut initial = Some(app);
        for restart in [false, true] {
            let app = if restart {
                let now = clock.clone();
                make_app(
                    AccountRoot::open_v2(&path, PasswordPool::new(1).unwrap()).unwrap(),
                    MASTER,
                    Arc::new(move || Ok(now.load(Ordering::SeqCst))),
                )
                .unwrap()
            } else {
                initial.take().unwrap()
            };
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let (send, receive) = tokio::sync::oneshot::channel();
            let task = tokio::spawn(crate::serve(listener, routes_app(app), async {
                let _ = receive.await;
            }));
            for (file, payload) in [
                (A, PAYLOAD),
                (empty, b"".as_slice()),
                (large, big.as_slice()),
            ] {
                let (status, headers, bytes) = tokio::time::timeout(
                    Duration::from_secs(10),
                    tcp(
                        address,
                        id,
                        a.access.expose(),
                        "read",
                        packet(file, true).to_string().as_bytes(),
                    ),
                )
                .await
                .unwrap();
                assert_eq!(status, 200);
                assert_eq!(bytes, payload);
                let headers = headers.to_ascii_lowercase();
                assert!(headers.contains("cache-control: no-store"));
                assert!(headers.contains("x-content-type-options: nosniff"));
                assert!(headers.contains("application/octet-stream"));
            }
            assert_eq!(
                tcp(
                    address,
                    id,
                    a.access.expose(),
                    "read",
                    packet(B, true).to_string().as_bytes()
                )
                .await
                .0,
                403
            );
            send.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(history_at(&path, &credentials), before);
        }
        let mut root = AccountRoot::open_v2(&path, PasswordPool::new(1).unwrap()).unwrap();
        assert_eq!(
            root.public_read_file(id, a.access.expose(), 50, A, 64)
                .unwrap()
                .payload(),
            PAYLOAD
        );
        drop(root);
        drop(_dir);
    }
}

#[tokio::test]
async fn timed_out_body_returns_no_payload_and_releases_the_original_worker() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let (f, [a, _, _]) = files(false).await;
    let (id, _) = &f.credentials[0];
    let router = routes_app(f.app.clone());
    let before = histories(&f);
    let body = Body::from_stream(futures_util::stream::pending::<Result<Bytes, std::io::Error>>());
    let result = tokio::time::timeout(
        Duration::from_secs(8),
        reply(
            &router,
            public_request(id, "files/read", Some(a.access.expose()), body),
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        (result.0, json_body(&result.2)),
        (StatusCode::REQUEST_TIMEOUT, json!({"code":"body_timeout"}))
    );
    assert_eq!(f.app.workers.available_permits(), 4);
    assert_eq!(histories(&f), before);
}

#[tokio::test]
async fn v1_user_roots_do_not_provision_files_or_upgrade_catalogs_implicitly() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    let f = fixture();
    public_sessions::enable(&f, 0).await;
    let (id, key) = &f.credentials[0];
    let access = f
        .app
        .root
        .lock()
        .await
        .sign_in(id, key, "synthetic_user", PASSWORD.as_bytes(), 50)
        .unwrap();
    let before = wal(&f);
    let router = routes_app(f.app.clone());
    for (kind, body) in [
        ("list", json!({})),
        ("info", packet(A, false)),
        ("read", packet(A, true)),
    ] {
        assert_eq!(
            op(&router, id, access.access.expose(), kind, body).await.0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(wal(&f), before);
    assert!(!f.path.join("files").exists());
    assert!(f.app.root.lock().await.projects().is_ok());
}

#[tokio::test]
async fn verified_copy_serves_files_only_after_explicit_admission_and_fresh_login() {
    let _serial = durability::PROCESS_TESTS.lock().await;
    for compact in [false, true] {
        let (f, [a, _, _]) = files(compact).await;
        let (id, key) = &f.credentials[0];
        let before = histories(&f);
        let bytes = {
            let mut root = f.app.root.lock().await;
            let snapshot = root.capture_v2().unwrap();
            let mut bytes = Vec::new();
            AccountFileArchiveReader::from_snapshot(&snapshot)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            bytes
        };
        crate::restore_account_file_archive_bytes_at(
            &bytes,
            &fs::File::open(f._dir.path()).unwrap(),
            "http-copy",
            PasswordPool::new(1).unwrap(),
            50,
        )
        .unwrap();
        let root = AccountRoot::open_v2(
            f._dir.path().join("http-copy"),
            PasswordPool::new(1).unwrap(),
        )
        .unwrap();
        let app = make_app(root, MASTER, Arc::new(|| Ok(50))).unwrap();
        let router = routes_app(app.clone());
        assert_eq!(
            op(&router, id, a.access.expose(), "read", packet(A, true))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        let fresh = {
            let mut root = app.root.lock().await;
            let flag = root.public_admission(id, key).unwrap();
            assert!(!flag.enabled);
            root.set_public_admission(id, key, flag.revision, true)
                .unwrap();
            root.public_sign_in(id, "synthetic_user", PASSWORD.as_bytes(), 50)
                .unwrap()
        };
        assert_eq!(
            op(&router, id, a.access.expose(), "read", packet(A, true))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            op(&router, id, fresh.access.expose(), "read", packet(A, true))
                .await
                .2
                .as_ref(),
            PAYLOAD
        );
        assert_eq!(histories(&f), before);
    }
}
