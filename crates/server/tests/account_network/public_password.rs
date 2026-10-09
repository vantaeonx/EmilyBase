use super::public_sessions::{login, user};
use super::*;
use std::io::Write;
const NEW: &str = "synthetic-replacement-password";
fn change(current: &str, replacement: &str) -> Json {
    json!({"current_password":current,"replacement_password":replacement})
}
fn sign(server: &support::Server, id: &str, password: &str) -> (u16, Json) {
    user(
        server,
        id,
        "sign-in",
        None,
        json!({"login":"synthetic_user","password":password}),
    )
}

#[test]
fn public_password_received_and_lost_changes_recover_revocation_and_verified_copy_on_both_wals() {
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
        let before = fs::read(&public).unwrap();
        let server = support::Server::start_account(&f.root, MASTER);
        let first = login(&server, &f.id);
        let another = login(&server, &f.id);
        let access = first["access_token"].as_str().unwrap();
        let (status, info) = user(
            &server,
            &f.id,
            "password",
            Some(access),
            change(PASSWORD, NEW),
        );
        assert_eq!(status, 200);
        assert_eq!(info["credential_epoch"], "2");
        let mut secrets = vec![
            MASTER,
            &f.key,
            &f.id,
            PASSWORD,
            NEW,
            access,
            first["refresh_token"].as_str().unwrap(),
            another["access_token"].as_str().unwrap(),
            another["refresh_token"].as_str().unwrap(),
        ];
        clean_log(&server.kill(), &secrets);
        let server = support::Server::start_account(&f.root, MASTER);
        for pair in [&first, &another] {
            assert_eq!(
                user(
                    &server,
                    &f.id,
                    "me",
                    pair["access_token"].as_str(),
                    json!({})
                )
                .0,
                401
            );
            assert_eq!(
                user(
                    &server,
                    &f.id,
                    "refresh",
                    None,
                    json!({"refresh_token":pair["refresh_token"]})
                )
                .0,
                401
            );
        }
        assert_eq!(sign(&server, &f.id, PASSWORD).0, 401);
        let (status, fresh) = sign(&server, &f.id, NEW);
        assert_eq!(status, 200);
        secrets.push(fresh["access_token"].as_str().unwrap());
        secrets.push(fresh["refresh_token"].as_str().unwrap());
        assert_eq!(
            user(
                &server,
                &f.id,
                "me",
                fresh["access_token"].as_str(),
                json!({})
            )
            .1["credential_epoch"],
            "2"
        );
        clean_log(&server.stop(), &secrets);
        let image = capture_account_bundle_root(&f.root, pool()).unwrap();
        let copy = f._dir.path().join("password-copy");
        restore_account_bundle_bytes(&image, &copy, pool(), 0).unwrap();
        let mut root = emilybase_server::AccountRoot::open(&copy, pool()).unwrap();
        let closed = root.public_admission(&f.id, &f.key).unwrap();
        assert!(!closed.enabled);
        root.set_public_admission(&f.id, &f.key, closed.revision, true)
            .unwrap();
        drop(root);
        let clone = support::Server::start_account(&copy, MASTER);
        assert_eq!(
            user(
                &clone,
                &f.id,
                "me",
                fresh["access_token"].as_str(),
                json!({})
            )
            .0,
            401
        );
        assert_eq!(sign(&clone, &f.id, PASSWORD).0, 401);
        let (status, copied) = sign(&clone, &f.id, NEW);
        assert_eq!(status, 200);
        assert_eq!(
            user(
                &clone,
                &f.id,
                "me",
                copied["access_token"].as_str(),
                json!({})
            )
            .1["credential_epoch"],
            "2"
        );
        clean_log(
            &clone.stop(),
            &[
                MASTER,
                &f.key,
                &f.id,
                PASSWORD,
                NEW,
                copied["access_token"].as_str().unwrap(),
                copied["refresh_token"].as_str().unwrap(),
            ],
        );
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(
            user(
                &server,
                &f.id,
                "me",
                fresh["access_token"].as_str(),
                json!({})
            )
            .0,
            200
        );
        assert_eq!(fs::read(&public).unwrap(), before);
        clean_log(&server.stop(), &secrets);
        // Send a second change without ever reading this socket's response.
        let server = support::Server::start_account(&f.root, MASTER);
        let private = f.root.join("private").join(&f.id).join("redo.wal");
        let private_before = fs::read(&private).unwrap();
        let final_password = "synthetic-final-password";
        let bytes = serde_json::to_vec(&change(NEW, final_password)).unwrap();
        let mut unread = support::socket(server.address).unwrap();
        unread
            .write_all(
                support::headers(
                    "POST",
                    &format!("/v1/projects/{}/user/password", f.id),
                    fresh["access_token"].as_str().unwrap(),
                    bytes.len(),
                )
                .as_bytes(),
            )
            .unwrap();
        unread.write_all(&bytes).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while fs::read(&private).unwrap() == private_before {
            assert!(
                std::time::Instant::now() < deadline,
                "unread password operation did not progress"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // A changed private WAL alone could be only the clock-floor commit.
        // A received new-password sign-in proves the complete change finished.
        let (status, observed) = sign(&server, &f.id, final_password);
        assert_eq!(status, 200);
        secrets.push(final_password);
        secrets.push(observed["access_token"].as_str().unwrap());
        secrets.push(observed["refresh_token"].as_str().unwrap());
        clean_log(&server.kill(), &secrets);
        drop(unread);
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(sign(&server, &f.id, NEW).0, 401);
        assert_eq!(
            user(
                &server,
                &f.id,
                "me",
                fresh["access_token"].as_str(),
                json!({})
            )
            .0,
            401
        );
        let (status, final_pair) = sign(&server, &f.id, final_password);
        assert_eq!(status, 200);
        assert_eq!(
            user(
                &server,
                &f.id,
                "me",
                final_pair["access_token"].as_str(),
                json!({})
            )
            .1["credential_epoch"],
            "3"
        );
        secrets.push(final_pair["access_token"].as_str().unwrap());
        secrets.push(final_pair["refresh_token"].as_str().unwrap());
        assert_eq!(fs::read(&public).unwrap(), before);
        clean_log(&server.stop(), &secrets);
    }
}
