use super::public_sessions::{login, user};
use super::*;
use std::io::Write;
const OWN: &[u8] = br#"{"version":1,"select":{"kind":"owner","column":"owner"},"insert":{"kind":"owner","column":"owner"},"update_using":{"kind":"owner","column":"owner"},"update_check":{"kind":"owner","column":"owner"},"delete":{"kind":"owner","column":"owner"}}"#;
fn integer(n: i64) -> Json {
    json!({"type":"integer","value":n.to_string()})
}
fn row(pk: i64, owner: &[u8], n: i64) -> Json {
    json!([integer(pk),{"type":"bytes","value":owner},integer(n)])
}
fn call(server: &support::Server, id: &str, access: &str, kind: &str, body: Json) -> (u16, Json) {
    user(server, id, &format!("rows/{kind}"), Some(access), body)
}

#[test]
fn public_row_received_and_unread_packets_recover_current_policies_and_nonempty_copy_on_both_wals()
{
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let f = fixture(compact);
        let mut root = emilybase_server::AccountRoot::open(&f.root, pool()).unwrap();
        let owner = root.list_users(&f.id, &f.key, None, 1).unwrap().users[0].id;
        root.execute(
            &f.id,
            &f.key,
            "CREATE TABLE owned(id INT PRIMARY KEY,owner BYTES,n INT)",
            &[],
        )
        .unwrap();
        root.enable_row_policy_catalog(&f.id, &f.key).unwrap();
        root.install_row_policy(&f.id, &f.key, "owned", 0, OWN)
            .unwrap();
        let closed = root.enable_public_admission_catalog(&f.id, &f.key).unwrap();
        root.set_public_admission(&f.id, &f.key, closed.revision, true)
            .unwrap();
        drop(root);
        let server = support::Server::start_account(&f.root, MASTER);
        let session = login(&server, &f.id);
        let access = session["access_token"].as_str().unwrap();
        let refresh = session["refresh_token"].as_str().unwrap();
        let secrets = [
            MASTER,
            &f.key,
            &f.id,
            PASSWORD,
            "synthetic_user",
            access,
            refresh,
        ];
        let public = f.root.join("registry").join(&f.id).join("data/redo.wal");
        let first = json!({"table":"owned","operations":[{"op":"insert","row":row(1,&owner,10)},{"op":"update","key":integer(1),"row":row(1,&owner,20)}]});
        let (status, receipt) = call(&server, &f.id, access, "write", first);
        assert_eq!(status, 200);
        assert_eq!(receipt["changed"], 2);
        let previous = receipt["transaction"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        clean_log(&server.kill(), &secrets);
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(
            call(
                &server,
                &f.id,
                access,
                "get",
                json!({"table":"owned","key":integer(1)})
            ),
            (200, json!({"row":row(1,&owner,20)}))
        );
        let unread_packet = json!({"table":"owned","operations":[{"op":"insert","row":row(2,&owner,20)},{"op":"update","key":integer(1),"row":row(1,&owner,30)}]});
        let bytes = serde_json::to_vec(&unread_packet).unwrap();
        let mut unread = support::socket(server.address).unwrap();
        unread
            .write_all(
                support::headers(
                    "POST",
                    &format!("/v1/projects/{}/user/rows/write", f.id),
                    access,
                    bytes.len(),
                )
                .as_bytes(),
            )
            .unwrap();
        unread.write_all(&bytes).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let wal = fs::read(&public).unwrap();
            let report = emilybase_backup::encode(&wal)
                .ok()
                .and_then(|image| emilybase_backup::inspect_bytes(&image).ok());
            if report.is_some_and(|r| r.last_transaction > previous) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "unread public packet did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // Another received read confirms the whole native operation completed;
        // this client never reads the write response and must not blindly retry it.
        assert_eq!(
            call(
                &server,
                &f.id,
                access,
                "get",
                json!({"table":"owned","key":integer(1)})
            ),
            (200, json!({"row":row(1,&owner,30)}))
        );
        assert_eq!(
            call(
                &server,
                &f.id,
                access,
                "get",
                json!({"table":"owned","key":integer(2)})
            ),
            (200, json!({"row":row(2,&owner,20)}))
        );
        clean_log(&server.kill(), &secrets);
        drop(unread);
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(
            call(
                &server,
                &f.id,
                access,
                "page",
                json!({"table":"owned","limit":128,"after":null})
            ),
            (
                200,
                json!({"rows":[row(1,&owner,30),row(2,&owner,20)],"next":null})
            )
        );
        let before = fs::read(&public).unwrap();
        assert_eq!(call(&server, &f.id, access, "write", unread_packet).0, 403);
        assert_eq!(fs::read(&public).unwrap(), before);
        let last = json!({"table":"owned","operations":[{"op":"update","key":integer(1),"row":row(1,&owner,40)},{"op":"delete","key":integer(2)}]});
        assert_eq!(call(&server, &f.id, access, "write", last).0, 200);
        clean_log(&server.kill(), &secrets);
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(
            call(
                &server,
                &f.id,
                access,
                "get",
                json!({"table":"owned","key":integer(1)})
            ),
            (200, json!({"row":row(1,&owner,40)}))
        );
        assert_eq!(
            call(
                &server,
                &f.id,
                access,
                "get",
                json!({"table":"owned","key":integer(2)})
            ),
            (200, json!({"row":null}))
        );
        clean_log(&server.stop(), &secrets);
        let image = capture_account_bundle_root(&f.root, pool()).unwrap();
        let copy_path = f._dir.path().join("public-row-copy");
        restore_account_bundle_bytes(&image, &copy_path, pool(), 0).unwrap();
        let source = support::Server::start_account(&f.root, MASTER);
        let copy = support::Server::start_account(&copy_path, MASTER);
        assert_eq!(
            call(
                &copy,
                &f.id,
                access,
                "get",
                json!({"table":"owned","key":integer(1)})
            )
            .0,
            401
        );
        clean_log(&copy.stop(), &secrets);
        let mut root = emilybase_server::AccountRoot::open(&copy_path, pool()).unwrap();
        let receipt = root.public_admission(&f.id, &f.key).unwrap();
        assert!(!receipt.enabled);
        root.set_public_admission(&f.id, &f.key, receipt.revision, true)
            .unwrap();
        drop(root);
        let copy = support::Server::start_account(&copy_path, MASTER);
        assert_eq!(
            call(
                &copy,
                &f.id,
                access,
                "get",
                json!({"table":"owned","key":integer(1)})
            )
            .0,
            401
        );
        let fresh = login(&copy, &f.id);
        let fresh_access = fresh["access_token"].as_str().unwrap();
        assert_eq!(
            call(
                &copy,
                &f.id,
                fresh_access,
                "get",
                json!({"table":"owned","key":integer(1)})
            ),
            (200, json!({"row":row(1,&owner,40)}))
        );
        clean_log(
            &copy.stop(),
            &[
                MASTER,
                &f.key,
                &f.id,
                PASSWORD,
                access,
                refresh,
                fresh_access,
                fresh["refresh_token"].as_str().unwrap(),
            ],
        );
        assert_eq!(
            call(
                &source,
                &f.id,
                access,
                "get",
                json!({"table":"owned","key":integer(1)})
            ),
            (200, json!({"row":row(1,&owner,40)}))
        );
        clean_log(&source.stop(), &secrets);
    }
}
