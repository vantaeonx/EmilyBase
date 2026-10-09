use super::*;
use std::io::Write;

pub(super) fn user(
    server: &support::Server,
    id: &str,
    action: &str,
    access: Option<&str>,
    body: Json,
) -> (u16, Json) {
    let bytes = serde_json::to_vec(&body).unwrap();
    let authorization = access
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    let mut socket = support::socket(server.address).unwrap();
    write!(socket,"POST /v1/projects/{id}/user/{action} HTTP/1.1\r\nHost: localhost\r\n{authorization}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len()).unwrap();
    socket.write_all(&bytes).unwrap();
    support::read(socket).unwrap()
}
pub(super) fn login(server: &support::Server, id: &str) -> Json {
    let (status, pair) = user(
        server,
        id,
        "sign-in",
        None,
        json!({"login":"synthetic_user","password":PASSWORD}),
    );
    assert_eq!(status, 200);
    assert_eq!(pair["access_token"].as_str().unwrap().len(), 102);
    assert_eq!(pair["refresh_token"].as_str().unwrap().len(), 102);
    pair
}

#[test]
fn public_received_session_results_survive_three_kills_and_nonempty_clone_stays_closed_on_both_wals()
 {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let f = fixture(compact);
        let mut root = emilybase_server::AccountRoot::open(&f.root, pool()).unwrap();
        root.enable_row_policy_catalog(&f.id, &f.key).unwrap();
        let closed = root.enable_public_admission_catalog(&f.id, &f.key).unwrap();
        root.set_public_admission(&f.id, &f.key, closed.revision, true)
            .unwrap();
        drop(root);
        let public = f.root.join("registry").join(&f.id).join("data/redo.wal");
        let original = fs::read(&public).unwrap();
        let server = support::Server::start_account(&f.root, MASTER);
        let first = login(&server, &f.id);
        let access = first["access_token"].as_str().unwrap();
        assert_eq!(user(&server, &f.id, "me", Some(access), json!({})).0, 200);
        clean_log(
            &server.kill(),
            &[
                MASTER,
                &f.key,
                &f.id,
                PASSWORD,
                "synthetic_user",
                access,
                first["refresh_token"].as_str().unwrap(),
            ],
        );
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(user(&server, &f.id, "me", Some(access), json!({})).0, 200);
        let (status, next) = user(
            &server,
            &f.id,
            "refresh",
            None,
            json!({"refresh_token":first["refresh_token"]}),
        );
        assert_eq!(status, 200);
        let next_access = next["access_token"].as_str().unwrap();
        let next_refresh = next["refresh_token"].as_str().unwrap();
        clean_log(
            &server.kill(),
            &[
                MASTER,
                &f.key,
                &f.id,
                PASSWORD,
                access,
                next_access,
                next_refresh,
            ],
        );
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(user(&server, &f.id, "me", Some(access), json!({})).0, 401);
        assert_eq!(
            user(
                &server,
                &f.id,
                "refresh",
                None,
                json!({"refresh_token":first["refresh_token"]})
            )
            .0,
            401
        );
        assert_eq!(
            user(&server, &f.id, "me", Some(next_access), json!({})).0,
            200
        );
        assert_eq!(
            user(
                &server,
                &f.id,
                "logout",
                None,
                json!({"refresh_token":next_refresh})
            ),
            (200, json!({"logged_out":true}))
        );
        clean_log(
            &server.kill(),
            &[MASTER, &f.key, &f.id, PASSWORD, next_access, next_refresh],
        );
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(
            user(&server, &f.id, "me", Some(next_access), json!({})).0,
            401
        );
        assert_eq!(
            user(
                &server,
                &f.id,
                "refresh",
                None,
                json!({"refresh_token":next_refresh})
            )
            .0,
            401
        );
        let source = login(&server, &f.id);
        let source_access = source["access_token"].as_str().unwrap();
        assert_eq!(
            auth(
                &server,
                &f.id,
                source_access,
                "users/list",
                json!({"limit":1})
            )
            .0,
            401
        );
        clean_log(
            &server.stop(),
            &[MASTER, &f.key, &f.id, PASSWORD, source_access],
        );
        assert_eq!(fs::read(&public).unwrap(), original);
        let image = capture_account_bundle_root(&f.root, pool()).unwrap();
        let copy_path = f._dir.path().join("public-copy");
        restore_account_bundle_bytes(&image, &copy_path, pool(), 0).unwrap();
        let source_server = support::Server::start_account(&f.root, MASTER);
        let copy = support::Server::start_account(&copy_path, MASTER);
        assert_eq!(
            user(&source_server, &f.id, "me", Some(source_access), json!({})).0,
            200
        );
        assert_eq!(
            user(
                &copy,
                &f.id,
                "sign-in",
                None,
                json!({"login":"synthetic_user","password":PASSWORD})
            )
            .0,
            401
        );
        assert_eq!(
            user(&copy, &f.id, "me", Some(source_access), json!({})).0,
            401
        );
        clean_log(
            &copy.stop(),
            &[MASTER, &f.key, &f.id, PASSWORD, source_access],
        );
        let mut root = emilybase_server::AccountRoot::open(&copy_path, pool()).unwrap();
        let receipt = root.public_admission(&f.id, &f.key).unwrap();
        assert!(!receipt.enabled);
        root.set_public_admission(&f.id, &f.key, receipt.revision, true)
            .unwrap();
        drop(root);
        let copy = support::Server::start_account(&copy_path, MASTER);
        assert_eq!(
            user(&copy, &f.id, "me", Some(source_access), json!({})).0,
            401
        );
        let fresh = login(&copy, &f.id);
        assert_eq!(
            user(
                &copy,
                &f.id,
                "me",
                Some(fresh["access_token"].as_str().unwrap()),
                json!({})
            )
            .0,
            200
        );
        clean_log(
            &copy.stop(),
            &[
                MASTER,
                &f.key,
                &f.id,
                PASSWORD,
                source_access,
                fresh["access_token"].as_str().unwrap(),
                fresh["refresh_token"].as_str().unwrap(),
            ],
        );
        assert_eq!(
            user(&source_server, &f.id, "me", Some(source_access), json!({})).0,
            200
        );
        clean_log(
            &source_server.stop(),
            &[MASTER, &f.key, &f.id, PASSWORD, source_access],
        );
        assert_eq!(fs::read(&public).unwrap(), original);
    }
}
