// Shared native fixtures provide methods for several independent test binaries.
#[allow(dead_code)]
mod support;
use emilybase_auth::{accounts::AccountStore, password::PasswordPool};
use emilybase_catalog::Value;
use emilybase_server::{ProjectStore, capture_account_bundle_root, restore_account_bundle_bytes};
use emilybase_transactions::Database;
use serde_json::{Value as Json, json};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

static CASES: Mutex<()> = Mutex::new(());
#[path = "account_network/public_rows.rs"]
mod public_rows;
#[path = "account_network/public_sessions.rs"]
mod public_sessions;
const MASTER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PASSWORD: &str = "synthetic-password";
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    id: String,
    key: String,
}
fn pool() -> PasswordPool {
    PasswordPool::new(1).unwrap()
}
fn fixture(compact: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = ProjectStore::open(dir.path().join("registry")).unwrap();
    let project = registry.create("synthetic-private-project").unwrap();
    registry
        .authorize(&project.project.id, &project.api_key)
        .unwrap()
        .execute(
            "CREATE TABLE t(id INT PRIMARY KEY,v TEXT);INSERT INTO t VALUES(1,'synthetic-row')",
            &[],
        )
        .unwrap();
    let mut account =
        AccountStore::create(dir.path().join("private"), &project.project.id, pool()).unwrap();
    account
        .create_user("synthetic_user", PASSWORD.as_bytes())
        .unwrap();
    if compact {
        account.compact().unwrap();
        Database::open(
            dir.path()
                .join("registry")
                .join(&project.project.id)
                .join("data"),
        )
        .unwrap()
        .compact()
        .unwrap();
    }
    let image = registry.capture_account_bundle(&mut [account]).unwrap();
    let root = dir.path().join("root");
    restore_account_bundle_bytes(&image, &root, pool(), 0).unwrap();
    Fixture {
        _dir: dir,
        root,
        id: project.project.id,
        key: project.api_key,
    }
}
fn auth(server: &support::Server, id: &str, key: &str, operation: &str, body: Json) -> (u16, Json) {
    support::call(
        server.address,
        "POST",
        &format!("/v1/projects/{id}/auth/{operation}"),
        key,
        &body,
    )
    .unwrap()
}
fn sign(server: &support::Server, id: &str, key: &str) -> Json {
    let (status, pair) = auth(
        server,
        id,
        key,
        "sign-in",
        json!({"login":"synthetic_user","password":PASSWORD}),
    );
    assert_eq!(status, 200);
    assert_eq!(pair["access_token"].as_str().unwrap().len(), 102);
    assert_eq!(pair["refresh_token"].as_str().unwrap().len(), 102);
    assert!(pair["expires_at"].as_str().unwrap().parse::<u64>().is_ok());
    pair
}
fn clean_log(log: &str, secrets: &[&str]) {
    for secret in secrets {
        assert!(
            !log.contains(secret),
            "private value appeared in native log"
        );
    }
}
fn command() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_emilybase-server"));
    for key in [
        "EMILYBASE_MASTER_KEY",
        "EMILYBASE_MASTER_KEY_FILE",
        "EMILYBASE_DATA_DIR",
        "EMILYBASE_ACCOUNT_ROOT",
        "EMILYBASE_LISTEN",
    ] {
        c.env_remove(key);
    }
    c
}

#[test]
fn mode_configuration_fails_before_creating_paths_and_never_echoes_private_values() {
    let _case = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("missing-private-root");
    let legacy = dir.path().join("missing-registry");
    for invalid in ["both", "missing", "secret", "listen"] {
        let mut c = command();
        c.env("EMILYBASE_ACCOUNT_ROOT", &root)
            .env("EMILYBASE_MASTER_KEY", MASTER)
            .env("EMILYBASE_LISTEN", "127.0.0.1:0");
        match invalid {
            "both" => {
                c.env("EMILYBASE_DATA_DIR", &legacy);
            }
            "secret" => {
                c.env("EMILYBASE_MASTER_KEY", "synthetic-private-invalid-master");
            }
            "listen" => {
                c.env("EMILYBASE_LISTEN", "synthetic-private-invalid-address");
            }
            _ => {}
        }
        let out = c.output().unwrap();
        assert!(!out.status.success());
        assert!(!root.exists());
        assert!(!legacy.exists());
        let log = String::from_utf8(out.stdout).unwrap();
        assert!(log.contains("startup_failed"));
        assert!(out.stderr.is_empty());
        clean_log(
            &log,
            &[
                MASTER,
                "synthetic-private-invalid-master",
                "synthetic-private-invalid-address",
                root.to_str().unwrap(),
                legacy.to_str().unwrap(),
            ],
        );
    }
}

#[test]
fn actual_http_ack_kills_refresh_race_restart_and_restored_clone_preserve_distinct_authority() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let f = fixture(compact);
        let server = support::Server::start_account(&f.root, MASTER);
        let first = sign(&server, &f.id, &f.key);
        assert_eq!(
            auth(
                &server,
                &f.id,
                &f.key,
                "me",
                json!({"access_token":first["access_token"]})
            )
            .0,
            200
        );
        let address = server.address;
        let path = format!("/v1/projects/{}/auth/refresh", f.id);
        let body = json!({"refresh_token":first["refresh_token"]});
        let mut results = std::thread::scope(|scope| {
            let a = scope.spawn(|| support::call(address, "POST", &path, &f.key, &body).unwrap());
            let b = scope.spawn(|| support::call(address, "POST", &path, &f.key, &body).unwrap());
            vec![a.join().unwrap(), b.join().unwrap()]
        });
        let mut statuses = results.iter().map(|r| r.0).collect::<Vec<_>>();
        statuses.sort();
        assert_eq!(statuses, vec![200, 401]);
        let next = results
            .swap_remove(results.iter().position(|r| r.0 == 200).unwrap())
            .1;
        let log = server.kill();
        clean_log(
            &log,
            &[
                MASTER,
                &f.key,
                &f.id,
                PASSWORD,
                "synthetic_user",
                first["access_token"].as_str().unwrap(),
                first["refresh_token"].as_str().unwrap(),
                next["access_token"].as_str().unwrap(),
                next["refresh_token"].as_str().unwrap(),
            ],
        );
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(
            auth(
                &server,
                &f.id,
                &f.key,
                "me",
                json!({"access_token":next["access_token"]})
            )
            .0,
            200
        );
        assert_eq!(
            auth(
                &server,
                &f.id,
                &f.key,
                "me",
                json!({"access_token":first["access_token"]})
            )
            .0,
            401
        );
        let (status, rotated) = support::call(
            server.address,
            "POST",
            &format!("/v1/projects/{}/keys/rotate", f.id),
            MASTER,
            &Json::Null,
        )
        .unwrap();
        assert_eq!(status, 200);
        let key = rotated["api_key"].as_str().unwrap();
        assert_eq!(
            auth(
                &server,
                &f.id,
                &f.key,
                "me",
                json!({"access_token":next["access_token"]})
            )
            .0,
            401
        );
        assert_eq!(
            auth(
                &server,
                &f.id,
                key,
                "me",
                json!({"access_token":next["access_token"]})
            )
            .0,
            200
        );
        assert_eq!(
            support::call(
                server.address,
                "POST",
                &format!("/v1/projects/{}/sql", f.id),
                next["access_token"].as_str().unwrap(),
                &json!({"sql":"SELECT * FROM t"})
            )
            .unwrap()
            .0,
            401
        );
        assert_eq!(
            support::call(
                server.address,
                "POST",
                &format!("/v1/projects/{}/sql", f.id),
                key,
                &json!({"sql":"INSERT INTO t VALUES(2,'synthetic-later-row')"})
            )
            .unwrap()
            .0,
            200
        );
        let log = server.kill();
        clean_log(
            &log,
            &[
                MASTER,
                &f.key,
                key,
                &f.id,
                PASSWORD,
                "synthetic_user",
                "synthetic-later-row",
                next["access_token"].as_str().unwrap(),
            ],
        );
        let wal = f.root.join("private").join(&f.id).join("redo.wal");
        let before = fs::read(&wal).unwrap();
        let image = capture_account_bundle_root(&f.root, pool()).unwrap();
        let clone = f._dir.path().join("clone");
        restore_account_bundle_bytes(&image, &clone, pool(), 0).unwrap();
        assert_eq!(fs::read(&wal).unwrap(), before);
        let source = support::Server::start_account(&f.root, MASTER);
        let copy = support::Server::start_account(&clone, MASTER);
        assert_eq!(
            auth(
                &source,
                &f.id,
                key,
                "me",
                json!({"access_token":next["access_token"]})
            )
            .0,
            200
        );
        assert_eq!(
            auth(
                &copy,
                &f.id,
                key,
                "me",
                json!({"access_token":next["access_token"]})
            )
            .0,
            401
        );
        assert_eq!(
            auth(
                &copy,
                &f.id,
                key,
                "refresh",
                json!({"refresh_token":next["refresh_token"]})
            )
            .0,
            401
        );
        let fresh = sign(&copy, &f.id, key);
        assert_eq!(
            auth(
                &source,
                &f.id,
                key,
                "me",
                json!({"access_token":fresh["access_token"]})
            )
            .0,
            401
        );
        for server in [&source, &copy] {
            let (status, report) = support::call(
                server.address,
                "POST",
                &format!("/v1/projects/{}/sql", f.id),
                key,
                &json!({"sql":"SELECT * FROM t ORDER BY id"}),
            )
            .unwrap();
            assert_eq!(status, 200);
            assert_eq!(report["results"][0]["rows"].as_array().unwrap().len(), 2);
        }
        assert_eq!(
            auth(
                &copy,
                &f.id,
                key,
                "logout",
                json!({"refresh_token":fresh["refresh_token"]})
            )
            .0,
            200
        );
        let log = copy.stop();
        clean_log(
            &log,
            &[
                MASTER,
                key,
                &f.id,
                PASSWORD,
                fresh["access_token"].as_str().unwrap(),
                fresh["refresh_token"].as_str().unwrap(),
            ],
        );
        let copy = support::Server::start_account(&clone, MASTER);
        assert_eq!(
            auth(
                &copy,
                &f.id,
                key,
                "me",
                json!({"access_token":fresh["access_token"]})
            )
            .0,
            401
        );
        copy.stop();
        source.stop();
        let db = Database::open(clone.join("registry").join(&f.id).join("data")).unwrap();
        assert_eq!(
            db.view().unwrap().scan("t", 10).unwrap()[1][1],
            Value::Text("synthetic-later-row".into())
        );
    }
}

#[test]
fn private_wal_corruption_fails_whole_root_startup_without_overwrite_or_secret_log() {
    let _case = CASES.lock().unwrap();
    let f = fixture(false);
    let path = f.root.join("private").join(&f.id).join("redo.wal");
    let mut bad = fs::read(&path).unwrap();
    let original = bad.clone();
    bad[0] ^= 1;
    fs::write(&path, &bad).unwrap();
    let output = command()
        .env("EMILYBASE_ACCOUNT_ROOT", &f.root)
        .env("EMILYBASE_MASTER_KEY", MASTER)
        .env("EMILYBASE_LISTEN", "127.0.0.1:0")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read(&path).unwrap(), bad);
    clean_log(
        &String::from_utf8(output.stdout).unwrap(),
        &[MASTER, &f.id, &f.key, PASSWORD, f.root.to_str().unwrap()],
    );
    fs::write(path, original).unwrap();
    let server = support::Server::start_account(&f.root, MASTER);
    sign(&server, &f.id, &f.key);
    server.stop();
}

#[test]
fn fresh_empty_root_can_be_provisioned_using_only_the_operator_and_private_http_protocol() {
    let _case = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("new-root");
    emilybase_server::initialize_account_root(&root, "synthetic-first-project", pool(), 0).unwrap();
    let server = support::Server::start_account(&root, MASTER);
    let (status, projects) =
        support::call(server.address, "GET", "/v1/projects", MASTER, &Json::Null).unwrap();
    assert_eq!(status, 200);
    assert_eq!(projects.as_array().unwrap().len(), 1);
    let id = projects[0]["id"].as_str().unwrap();
    let (status, created) = support::call(
        server.address,
        "POST",
        &format!("/v1/projects/{id}/keys/rotate"),
        MASTER,
        &Json::Null,
    )
    .unwrap();
    assert_eq!(status, 200);
    let key = created["api_key"].as_str().unwrap();
    assert_eq!(
        auth(
            &server,
            id,
            key,
            "sign-in",
            json!({"login":"synthetic_user","password":PASSWORD})
        )
        .0,
        401
    );
    assert_eq!(
        auth(
            &server,
            id,
            key,
            "users",
            json!({"login":"synthetic_user","password":PASSWORD})
        )
        .0,
        201
    );
    let pair = sign(&server, id, key);
    assert_eq!(
        auth(
            &server,
            id,
            key,
            "me",
            json!({"access_token":pair["access_token"]})
        )
        .0,
        200
    );
    assert_eq!(
        support::call(
            server.address,
            "POST",
            &format!("/v1/projects/{id}/sql"),
            key,
            &json!({"sql":"CREATE TABLE t(id INT PRIMARY KEY);INSERT INTO t VALUES(1)"})
        )
        .unwrap()
        .0,
        200
    );
    let log = server.stop();
    clean_log(
        &log,
        &[
            MASTER,
            id,
            key,
            PASSWORD,
            "synthetic-first-project",
            "synthetic_user",
            pair["access_token"].as_str().unwrap(),
            pair["refresh_token"].as_str().unwrap(),
        ],
    );
    let report = emilybase_server::inspect_account_bundle_root(&root, pool()).unwrap();
    assert_eq!(report.registry.projects[0].rows, 1);
    assert_eq!(report.private_accounts[0].inventory.accounts, 1);
    assert_eq!(report.private_accounts[0].inventory.session_families, 1);
    let server = support::Server::start_account(&root, MASTER);
    assert_eq!(
        auth(
            &server,
            id,
            key,
            "me",
            json!({"access_token":pair["access_token"]})
        )
        .0,
        200
    );
    server.stop();
}

#[test]
fn credential_mutation_acks_survive_three_kills_without_reviving_old_sessions_on_both_wals() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let f = fixture(compact);
        let replacement = "synthetic-replacement-界\0-password";
        let server = support::Server::start_account(&f.root, MASTER);
        let old = sign(&server, &f.id, &f.key);
        let (status, user) = auth(
            &server,
            &f.id,
            &f.key,
            "password",
            json!({"login":"synthetic_user","current_password":PASSWORD,"replacement_password":replacement}),
        );
        assert_eq!(status, 200);
        assert_eq!(user["credential_epoch"], "2");
        let log = server.kill();
        clean_log(
            &log,
            &[
                MASTER,
                &f.id,
                &f.key,
                PASSWORD,
                replacement,
                old["access_token"].as_str().unwrap(),
                old["refresh_token"].as_str().unwrap(),
            ],
        );
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(
            auth(
                &server,
                &f.id,
                &f.key,
                "sign-in",
                json!({"login":"synthetic_user","password":PASSWORD})
            )
            .0,
            401
        );
        assert_eq!(
            auth(
                &server,
                &f.id,
                &f.key,
                "me",
                json!({"access_token":old["access_token"]})
            )
            .0,
            401
        );
        let (status, fresh) = auth(
            &server,
            &f.id,
            &f.key,
            "sign-in",
            json!({"login":"synthetic_user","password":replacement}),
        );
        assert_eq!(status, 200);
        let (status, user) = auth(
            &server,
            &f.id,
            &f.key,
            "disabled",
            json!({"login":"synthetic_user","disabled":true}),
        );
        assert_eq!(status, 200);
        assert_eq!(user["credential_epoch"], "3");
        let log = server.kill();
        clean_log(
            &log,
            &[
                MASTER,
                &f.id,
                &f.key,
                PASSWORD,
                replacement,
                fresh["access_token"].as_str().unwrap(),
                fresh["refresh_token"].as_str().unwrap(),
            ],
        );
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(
            auth(
                &server,
                &f.id,
                &f.key,
                "sign-in",
                json!({"login":"synthetic_user","password":replacement})
            )
            .0,
            401
        );
        for (operation, field) in [("me", "access_token"), ("refresh", "refresh_token")] {
            assert_eq!(
                auth(
                    &server,
                    &f.id,
                    &f.key,
                    operation,
                    json!({field:fresh[field]})
                )
                .0,
                401
            );
        }
        let (status, user) = auth(
            &server,
            &f.id,
            &f.key,
            "disabled",
            json!({"login":"synthetic_user","disabled":false}),
        );
        assert_eq!(status, 200);
        assert_eq!(user["credential_epoch"], "4");
        let log = server.kill();
        clean_log(&log, &[MASTER, &f.id, &f.key, PASSWORD, replacement]);
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(
            auth(
                &server,
                &f.id,
                &f.key,
                "sign-in",
                json!({"login":"synthetic_user","password":replacement})
            )
            .0,
            200
        );
        for pair in [&old, &fresh] {
            for (operation, field) in [("me", "access_token"), ("refresh", "refresh_token")] {
                assert_eq!(
                    auth(
                        &server,
                        &f.id,
                        &f.key,
                        operation,
                        json!({field:pair[field]})
                    )
                    .0,
                    401
                );
            }
        }
        let log = server.stop();
        clean_log(
            &log,
            &[
                MASTER,
                &f.id,
                &f.key,
                PASSWORD,
                replacement,
                "synthetic_user",
            ],
        );
    }
}

#[test]
fn actual_tcp_prune_race_ack_kill_keeps_removal_and_live_authority_on_both_wals() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let f = fixture(compact);
        let server = support::Server::start_account(&f.root, MASTER);
        let revoked = sign(&server, &f.id, &f.key);
        let stale = sign(&server, &f.id, &f.key);
        assert_eq!(
            auth(
                &server,
                &f.id,
                &f.key,
                "logout",
                json!({"refresh_token":revoked["refresh_token"]})
            )
            .0,
            200
        );
        assert_eq!(auth(&server,&f.id,&f.key,"password",json!({"login":"synthetic_user","current_password":PASSWORD,"replacement_password":PASSWORD})).0,200);
        let active = sign(&server, &f.id, &f.key);
        let public_wal =
            fs::read(f.root.join("registry").join(&f.id).join("data/redo.wal")).unwrap();
        let address = server.address;
        let path = format!("/v1/projects/{}/auth/sessions/prune", f.id);
        let body = json!({"limit":128});
        let mut counts = std::thread::scope(|scope| {
            let a = scope.spawn(|| support::call(address, "POST", &path, &f.key, &body).unwrap());
            let b = scope.spawn(|| support::call(address, "POST", &path, &f.key, &body).unwrap());
            [a.join().unwrap(), b.join().unwrap()]
                .into_iter()
                .map(|(status, response)| {
                    assert_eq!(status, 200);
                    response["removed"].as_u64().unwrap()
                })
                .collect::<Vec<_>>()
        });
        counts.sort();
        assert_eq!(counts, vec![0, 2]);
        let log = server.kill();
        let report = emilybase_server::inspect_account_bundle_root(&f.root, pool()).unwrap();
        assert_eq!(report.private_accounts[0].inventory.session_families, 1);
        assert_eq!(
            report.private_accounts[0].inventory.database.wal_version,
            if compact { 2 } else { 1 }
        );
        clean_log(
            &log,
            &[
                MASTER,
                &f.id,
                &f.key,
                PASSWORD,
                revoked["access_token"].as_str().unwrap(),
                revoked["refresh_token"].as_str().unwrap(),
                stale["access_token"].as_str().unwrap(),
                stale["refresh_token"].as_str().unwrap(),
                active["access_token"].as_str().unwrap(),
                active["refresh_token"].as_str().unwrap(),
            ],
        );
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(
            auth(
                &server,
                &f.id,
                &f.key,
                "sessions/prune",
                json!({"limit":128})
            ),
            (200, json!({"removed":0}))
        );
        for pair in [&revoked, &stale] {
            for (operation, field) in [("me", "access_token"), ("refresh", "refresh_token")] {
                assert_eq!(
                    auth(
                        &server,
                        &f.id,
                        &f.key,
                        operation,
                        json!({field:pair[field]})
                    )
                    .0,
                    401
                );
            }
        }
        assert_eq!(
            auth(
                &server,
                &f.id,
                &f.key,
                "me",
                json!({"access_token":active["access_token"]})
            )
            .0,
            200
        );
        let (status, next) = auth(
            &server,
            &f.id,
            &f.key,
            "refresh",
            json!({"refresh_token":active["refresh_token"]}),
        );
        assert_eq!(status, 200);
        assert_eq!(
            fs::read(f.root.join("registry").join(&f.id).join("data/redo.wal")).unwrap(),
            public_wal
        );
        let log = server.stop();
        clean_log(
            &log,
            &[
                MASTER,
                &f.id,
                &f.key,
                PASSWORD,
                next["access_token"].as_str().unwrap(),
                next["refresh_token"].as_str().unwrap(),
            ],
        );
        let report = emilybase_server::inspect_account_bundle_root(&f.root, pool()).unwrap();
        assert_eq!(report.private_accounts[0].inventory.session_families, 1);
    }
}

#[test]
fn actual_tcp_user_pages_keep_exact_private_history_and_current_service_scope_across_restart() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let f = fixture(compact);
        let server = support::Server::start_account(&f.root, MASTER);
        for login in ["z_last", "a_first"] {
            assert_eq!(
                auth(
                    &server,
                    &f.id,
                    &f.key,
                    "users",
                    json!({"login":login,"password":PASSWORD})
                )
                .0,
                201
            );
        }
        let (status, disabled) = auth(
            &server,
            &f.id,
            &f.key,
            "disabled",
            json!({"login":"a_first","disabled":true}),
        );
        assert_eq!(status, 200);
        let private_path = f.root.join("private").join(&f.id).join("redo.wal");
        let public_path = f.root.join("registry").join(&f.id).join("data/redo.wal");
        let private_before = fs::read(&private_path).unwrap();
        let public_before = fs::read(&public_path).unwrap();
        let (status, first) = auth(&server, &f.id, &f.key, "users/list", json!({"limit":2}));
        assert_eq!(status, 200);
        assert_eq!(first["users"][0], disabled);
        assert_eq!(first["next_after"], "synthetic_user");
        let (status, last) = auth(
            &server,
            &f.id,
            &f.key,
            "users/list",
            json!({"limit":2,"after":first["next_after"]}),
        );
        assert_eq!(status, 200);
        assert_eq!(last["users"].as_array().unwrap().len(), 1);
        assert_eq!(last["users"][0]["login"], "z_last");
        assert_eq!(last["next_after"], Json::Null);
        assert_eq!(
            auth(&server, &f.id, MASTER, "users/list", json!({"limit":1})).0,
            401
        );
        let (status, rotated) = support::call(
            server.address,
            "POST",
            &format!("/v1/projects/{}/keys/rotate", f.id),
            MASTER,
            &json!({}),
        )
        .unwrap();
        assert_eq!(status, 200);
        let key = rotated["api_key"].as_str().unwrap();
        assert_eq!(
            auth(&server, &f.id, &f.key, "users/list", json!({"limit":1})).0,
            401
        );
        assert_eq!(fs::read(&private_path).unwrap(), private_before);
        assert_eq!(fs::read(&public_path).unwrap(), public_before);
        let log = server.stop();
        clean_log(
            &log,
            &[
                MASTER,
                &f.id,
                &f.key,
                key,
                PASSWORD,
                "synthetic_user",
                "a_first",
                "z_last",
            ],
        );
        let report = emilybase_server::inspect_account_bundle_root(&f.root, pool()).unwrap();
        assert_eq!(report.private_accounts[0].inventory.accounts, 3);
        assert_eq!(report.private_accounts[0].inventory.session_families, 0);
        assert_eq!(
            report.private_accounts[0].inventory.database.wal_version,
            if compact { 2 } else { 1 }
        );
        let server = support::Server::start_account(&f.root, MASTER);
        let (status, again) = auth(&server, &f.id, key, "users/list", json!({"limit":2}));
        assert_eq!(status, 200);
        assert_eq!(again, first);
        let (status, after) = auth(
            &server,
            &f.id,
            key,
            "users/list",
            json!({"limit":128,"after":"zzzz"}),
        );
        assert_eq!(status, 200);
        assert_eq!(after, json!({"users":[],"next_after":null}));
        assert_eq!(fs::read(&private_path).unwrap(), private_before);
        assert_eq!(fs::read(&public_path).unwrap(), public_before);
        let log = server.stop();
        clean_log(
            &log,
            &[
                MASTER,
                &f.id,
                &f.key,
                key,
                PASSWORD,
                "synthetic_user",
                "a_first",
                "z_last",
            ],
        );
    }
}

#[test]
fn private_root_tcp_transfer_ack_survives_kill_without_private_history_changes() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        let f = fixture(compact);
        let private_path = f.root.join("private").join(&f.id).join("redo.wal");
        let before = fs::read(&private_path).unwrap();
        let server = support::Server::start_account(&f.root, MASTER);
        let base = format!("/v1/projects/{}/tables", f.id);
        let (status, mut document) = support::call(
            server.address,
            "POST",
            &(base.clone() + "/export"),
            &f.key,
            &json!({"table":"t"}),
        )
        .unwrap();
        assert_eq!(status, 200);
        document["schema"]["name"] = json!("copied");
        assert_eq!(
            support::call(
                server.address,
                "POST",
                &(base.clone() + "/import"),
                MASTER,
                &document
            )
            .unwrap()
            .0,
            401
        );
        let (status, report) = support::call(
            server.address,
            "POST",
            &(base.clone() + "/import"),
            &f.key,
            &document,
        )
        .unwrap();
        assert_eq!(status, 200);
        assert_eq!(report["transfer"]["rows"], 1);
        let first = server.kill();
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(
            support::call(
                server.address,
                "POST",
                &(base.clone() + "/export"),
                &f.key,
                &json!({"table":"copied"})
            )
            .unwrap(),
            (200, document.clone())
        );
        let public_path = f.root.join("registry").join(&f.id).join("data/redo.wal");
        let public_before = fs::read(&public_path).unwrap();
        assert_eq!(
            support::call(
                server.address,
                "POST",
                &(base + "/import"),
                &f.key,
                &document
            )
            .unwrap()
            .0,
            400
        );
        let second = server.stop();
        assert_eq!(fs::read(&private_path).unwrap(), before);
        assert_eq!(fs::read(&public_path).unwrap(), public_before);
        for log in [&first, &second] {
            clean_log(log, &[MASTER, &f.id, &f.key, "synthetic-row"]);
        }
    }
}

#[test]
fn actual_tcp_insert_select_ack_kill_keeps_project_copy_and_private_history_on_both_routers() {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        for private in [false, true] {
            let f = fixture(compact);
            let private_path = f.root.join("private").join(&f.id).join("redo.wal");
            let private_before = fs::read(&private_path).unwrap();
            let public = f.root.join("registry").join(&f.id).join("data");
            let mut database = Database::open(&public).unwrap();
            let base = database.last_transaction();
            let public_before = database.committed_wal().unwrap();
            drop(database);
            let start = || {
                if private {
                    support::Server::start_account(&f.root, MASTER)
                } else {
                    support::Server::start(&f.root.join("registry"), MASTER)
                }
            };
            let server = start();
            let route = format!("/v1/projects/{}/sql", f.id);
            let script = json!({"sql":"CREATE TABLE copied(id INT PRIMARY KEY,v TEXT,note TEXT); INSERT INTO copied(id,v) SELECT * FROM t ORDER BY id; SELECT * FROM copied ORDER BY id"});
            let invalid = emilybase_auth::issue_key().unwrap();
            for key in [MASTER, invalid.as_str()] {
                assert_eq!(
                    support::call(server.address, "POST", &route, key, &script)
                        .unwrap()
                        .0,
                    401
                );
            }
            assert_eq!(fs::read(public.join("redo.wal")).unwrap(), public_before);
            let (status, report) =
                support::call(server.address, "POST", &route, &f.key, &script).unwrap();
            assert_eq!(status, 200);
            assert_eq!(report["transaction"], base + 1);
            assert_eq!(report["results"][1]["affected"], 1);
            assert_eq!(report["results"][2]["rows"][0].as_array().unwrap().len(), 3);
            let first = server.kill();
            let server = start();
            let (status, recovered) = support::call(
                server.address,
                "POST",
                &route,
                &f.key,
                &json!({"sql":"SELECT * FROM copied ORDER BY id"}),
            )
            .unwrap();
            assert_eq!(status, 200);
            assert_eq!(recovered["transaction"], base + 1);
            assert_eq!(recovered["results"][0], report["results"][2]);
            let public_before = fs::read(public.join("redo.wal")).unwrap();
            let (status,error)=support::call(server.address,"POST",&route,&f.key,&json!({"sql":"UPDATE copied SET v='partial'; INSERT INTO copied(id,v) SELECT * FROM t"})).unwrap();
            assert_eq!(status, 400);
            assert_eq!(error, json!({"code":"query_rejected"}));
            let second = server.stop();
            assert_eq!(fs::read(public.join("redo.wal")).unwrap(), public_before);
            assert_eq!(fs::read(&private_path).unwrap(), private_before);
            for log in [first, second] {
                clean_log(
                    &log,
                    &[MASTER, &f.id, &f.key, &invalid, "synthetic-row", PASSWORD],
                );
            }
        }
    }
}

#[test]
fn migration_tcp_ack_kills_exact_retries_and_concurrent_versions_preserve_both_modes_and_private_history()
 {
    let _case = CASES.lock().unwrap();
    for compact in [false, true] {
        for private in [false, true] {
            let f = fixture(compact);
            let private_path = f.root.join("private").join(&f.id).join("redo.wal");
            let before = fs::read(&private_path).unwrap();
            let public = f.root.join("registry").join(&f.id).join("data");
            let base = Database::open(&public).unwrap().last_transaction();
            let start = || {
                if private {
                    support::Server::start_account(&f.root, MASTER)
                } else {
                    support::Server::start(&f.root.join("registry"), MASTER)
                }
            };
            let route = format!("/v1/projects/{}/migrations", f.id);
            let apply = route.clone() + "/apply";
            let server = start();
            assert_eq!(
                support::call(server.address, "GET", &route, &f.key, &json!(null)).unwrap(),
                (200, json!({"migrations":[]}))
            );
            let initial = json!({"version":1,"label":"initial","sql":"CREATE TABLE copied(id INT PRIMARY KEY,v TEXT,note TEXT); INSERT INTO copied(id,v) SELECT * FROM t"});
            assert_eq!(
                support::call(server.address, "POST", &apply, MASTER, &initial)
                    .unwrap()
                    .0,
                401
            );
            let (status, first) =
                support::call(server.address, "POST", &apply, &f.key, &initial).unwrap();
            assert_eq!(status, 200);
            assert_eq!(first["already_applied"], false);
            assert_eq!(first["receipt"]["transaction"], (base + 1).to_string());
            let mut logs = vec![server.kill()];
            let server = start();
            let unchanged = fs::read(public.join("redo.wal")).unwrap();
            let (status, repeated) =
                support::call(server.address, "POST", &apply, &f.key, &initial).unwrap();
            assert_eq!(status, 200);
            assert_eq!(repeated["already_applied"], true);
            assert_eq!(repeated["receipt"], first["receipt"]);
            assert_eq!(fs::read(public.join("redo.wal")).unwrap(), unchanged);
            let next =
                json!({"version":2,"label":"next","sql":"UPDATE copied SET v='next' WHERE id=1"});
            let results = std::thread::scope(|scope| {
                let a = scope.spawn(|| {
                    support::call(server.address, "POST", &apply, &f.key, &next).unwrap()
                });
                let b = scope.spawn(|| {
                    support::call(server.address, "POST", &apply, &f.key, &next).unwrap()
                });
                [a.join().unwrap(), b.join().unwrap()]
            });
            assert!(results.iter().all(|r| r.0 == 200));
            assert_ne!(
                results[0].1["already_applied"],
                results[1].1["already_applied"]
            );
            assert_eq!(results[0].1["receipt"], results[1].1["receipt"]);
            assert_eq!(
                results[0].1["receipt"]["transaction"],
                (base + 2).to_string()
            );
            logs.push(server.kill());
            let server = start();
            let (status, listed) =
                support::call(server.address, "GET", &route, &f.key, &json!(null)).unwrap();
            assert_eq!(status, 200);
            assert_eq!(
                listed,
                json!({"migrations":[first["receipt"].clone(),results[0].1["receipt"].clone()]})
            );
            let unchanged = fs::read(public.join("redo.wal")).unwrap();
            for input in [
                json!({"version":2,"label":"changed","sql":"DROP TABLE copied"}),
                json!({"version":4,"label":"skipped","sql":"DROP TABLE copied"}),
                json!({"version":3,"label":"failed","sql":"UPDATE copied SET v='partial'; INSERT INTO copied(id,v) SELECT * FROM t"}),
            ] {
                assert_eq!(
                    support::call(server.address, "POST", &apply, &f.key, &input).unwrap(),
                    (400, json!({"code":"migration_rejected"}))
                );
            }
            logs.push(server.stop());
            assert_eq!(fs::read(public.join("redo.wal")).unwrap(), unchanged);
            assert_eq!(fs::read(&private_path).unwrap(), before);
            let database = Database::open(&public).unwrap();
            assert_eq!(emilybase_migrations::inspect(&database).unwrap().len(), 2);
            assert_eq!(database.last_transaction(), base + 2);
            assert_eq!(
                database
                    .view()
                    .unwrap()
                    .get("copied", &emilybase_catalog::Key::Integer(1))
                    .unwrap()
                    .unwrap()[1],
                Value::Text("next".into())
            );
            for log in logs {
                clean_log(&log, &[MASTER, &f.id, &f.key, "synthetic-row", PASSWORD]);
            }
        }
    }
}

#[test]
fn policy_http_received_ack_kills_recover_catalog_replacements_exact_retry_and_verified_clone_on_both_wals()
 {
    let _case = CASES.lock().unwrap();
    let deny = r#"{"version":1,"select":{"kind":"deny"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}}"#;
    for compact in [false, true] {
        let f = fixture(compact);
        let server = support::Server::start_account(&f.root, MASTER);
        let pair = sign(&server, &f.id, &f.key);
        for wrong in [
            MASTER,
            pair["access_token"].as_str().unwrap(),
            pair["refresh_token"].as_str().unwrap(),
        ] {
            assert_eq!(
                auth(&server, &f.id, wrong, "policies/enable", json!({})).0,
                401
            );
        }
        let original = fs::read(f.root.join("registry").join(&f.id).join("data/redo.wal")).unwrap();
        assert_eq!(
            auth(&server, &f.id, &f.key, "policies/enable", json!({})),
            (200, json!({"private_version":4}))
        );
        clean_log(&server.kill(), &[MASTER, &f.key, PASSWORD, deny]);
        let server = support::Server::start_account(&f.root, MASTER);
        let path = format!("/v1/projects/{}/auth/policies", f.id);
        assert_eq!(
            support::call(server.address, "GET", &path, &f.key, &json!({})).unwrap(),
            (200, json!({"policies":[]}))
        );
        let mut long = deny.to_owned();
        long.push_str(&" ".repeat(16_384 - long.len()));
        let request = json!({"table":"t","expected":"0","document":long});
        let (status, first) = auth(&server, &f.id, &f.key, "policies/install", request.clone());
        assert_eq!(status, 200);
        let private = f.root.join("private").join(&f.id).join("redo.wal");
        let before = fs::read(&private).unwrap();
        clean_log(&server.kill(), &[MASTER, &f.key, PASSWORD, deny]);
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(
            auth(&server, &f.id, &f.key, "policies/install", request),
            (200, first.clone())
        );
        assert_eq!(fs::read(&private).unwrap(), before);
        let next = json!({"table":"t","expected":first["receipt"]["revision"],"document":deny});
        let (status, replacement) = auth(&server, &f.id, &f.key, "policies/install", next.clone());
        assert_eq!(status, 200);
        assert_eq!(
            replacement["receipt"]["previous"],
            first["receipt"]["revision"]
        );
        clean_log(&server.kill(), &[MASTER, &f.key, PASSWORD, deny]);
        let server = support::Server::start_account(&f.root, MASTER);
        let before = fs::read(&private).unwrap();
        assert_eq!(
            auth(&server, &f.id, &f.key, "policies/install", next),
            (200, replacement.clone())
        );
        assert_eq!(fs::read(&private).unwrap(), before);
        assert_eq!(
            support::call(server.address, "GET", &path, &f.key, &json!({})).unwrap(),
            (200, json!({"policies":[replacement["receipt"].clone()]}))
        );
        // The write client never reads its response. A separate inspection proves
        // completion before the kill, then an explicit predecessor retry is exact.
        use std::io::Write;
        let unread_request = json!({"table":"t","expected":replacement["receipt"]["revision"],"document":format!("{deny} ")});
        let bytes = serde_json::to_vec(&unread_request).unwrap();
        let mut unread = support::socket(server.address).unwrap();
        unread
            .write_all(
                support::headers(
                    "POST",
                    &format!("/v1/projects/{}/auth/policies/install", f.id),
                    &f.key,
                    bytes.len(),
                )
                .as_bytes(),
            )
            .unwrap();
        unread.write_all(&bytes).unwrap();
        let prior = replacement["receipt"]["revision"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let bytes = fs::read(&private).unwrap();
            let report = emilybase_backup::encode(&bytes).ok().and_then(|archive| {
                emilybase_auth::accounts::inspect_private_account_backup_bytes(&archive, &f.id).ok()
            });
            if report.is_some_and(|r| r.database.last_transaction > prior) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "unread policy request did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let (status, observed) =
            support::call(server.address, "GET", &path, &f.key, &json!({})).unwrap();
        assert_eq!(status, 200);
        let replacement = json!({"receipt":observed["policies"][0].clone()});
        assert_eq!(replacement["receipt"]["previous"], prior.to_string());
        drop(unread);
        clean_log(&server.kill(), &[MASTER, &f.key, PASSWORD, deny]);
        let server = support::Server::start_account(&f.root, MASTER);
        let before = fs::read(&private).unwrap();
        assert_eq!(
            auth(&server, &f.id, &f.key, "policies/install", unread_request),
            (200, replacement.clone())
        );
        assert_eq!(fs::read(&private).unwrap(), before);
        assert_eq!(
            auth(
                &server,
                &f.id,
                &f.key,
                "me",
                json!({"access_token":pair["access_token"]})
            )
            .0,
            200
        );
        assert_eq!(
            fs::read(f.root.join("registry").join(&f.id).join("data/redo.wal")).unwrap(),
            original
        );
        let log = server.stop();
        clean_log(&log, &[MASTER, &f.key, PASSWORD, deny]);
        let image = capture_account_bundle_root(&f.root, pool()).unwrap();
        let clone = f._dir.path().join("policy-clone");
        restore_account_bundle_bytes(&image, &clone, pool(), 0).unwrap();
        let mut copied =
            AccountStore::open(clone.join("private").join(&f.id), &f.id, pool()).unwrap();
        assert_eq!(
            copied.row_policy_receipts().unwrap()[0]
                .revision
                .to_string(),
            replacement["receipt"]["revision"]
        );
        assert!(
            copied
                .verify_access(pair["access_token"].as_str().unwrap(), 0)
                .is_err()
        );
        drop(copied);
        let copied = support::Server::start_account(&clone, MASTER);
        assert_eq!(
            support::call(copied.address, "GET", &path, &f.key, &json!({})).unwrap(),
            (200, json!({"policies":[replacement["receipt"].clone()]}))
        );
        copied.stop();
    }
}

#[test]
fn user_data_http_received_and_unread_packet_results_recover_filter_and_restore_on_both_wals() {
    use std::io::Write;
    let _case = CASES.lock().unwrap();
    let own = br#"{"version":1,"select":{"kind":"owner","column":"owner"},"insert":{"kind":"owner","column":"owner"},"update_using":{"kind":"owner","column":"owner"},"update_check":{"kind":"owner","column":"owner"},"delete":{"kind":"owner","column":"owner"}}"#;
    for compact in [false, true] {
        let f = fixture(compact);
        let mut root = emilybase_server::AccountRoot::open(&f.root, pool()).unwrap();
        root.execute(
            &f.id,
            &f.key,
            "CREATE TABLE owned(id INT PRIMARY KEY,owner BYTES,amount INT)",
            &[],
        )
        .unwrap();
        root.enable_row_policy_catalog(&f.id, &f.key).unwrap();
        root.install_row_policy(&f.id, &f.key, "owned", 0, own)
            .unwrap();
        drop(root);
        let server = support::Server::start_account(&f.root, MASTER);
        let pair = sign(&server, &f.id, &f.key);
        let (status, metadata) = auth(
            &server,
            &f.id,
            &f.key,
            "me",
            json!({"access_token":pair["access_token"]}),
        );
        assert_eq!(status, 200);
        let text = metadata["id"].as_str().unwrap();
        let owner = (0..16)
            .map(|n| u8::from_str_radix(&text[n * 2..n * 2 + 2], 16).unwrap())
            .collect::<Vec<_>>();
        let integer = |n: i64| json!({"type":"integer","value":n.to_string()});
        let row = |pk: i64, amount: i64| json!([integer(pk),{"type":"bytes","value":owner},integer(amount)]);
        let call = |server: &support::Server, access: &str, op: &str, input: Json| {
            let bytes = serde_json::to_vec(&input).unwrap();
            let headers = support::headers(
                "POST",
                &format!("/v1/projects/{}/auth/rows/{op}", f.id),
                &f.key,
                bytes.len(),
            )
            .replace(
                "\r\n\r\n",
                &format!(
                    "\r\n{}: {access}\r\n\r\n",
                    emilybase_server::USER_ACCESS_HEADER
                ),
            );
            let mut socket = support::socket(server.address).unwrap();
            socket.write_all(headers.as_bytes()).unwrap();
            socket.write_all(&bytes).unwrap();
            support::read(socket).unwrap()
        };
        let access = pair["access_token"].as_str().unwrap();
        let packet = json!({"table":"owned","operations":[{"op":"insert","row":row(1,10)},{"op":"update","key":integer(1),"row":row(1,20)}]});
        let (status, first) = call(&server, access, "write", packet);
        assert_eq!(status, 200);
        assert_eq!(first["changed"], 2);
        let previous = first["transaction"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        let secrets = [
            MASTER,
            f.key.as_str(),
            PASSWORD,
            access,
            pair["refresh_token"].as_str().unwrap(),
        ];
        clean_log(&server.kill(), &secrets);
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(
            call(
                &server,
                access,
                "get",
                json!({"table":"owned","key":integer(1)})
            ),
            (200, json!({"row":row(1,20)}))
        );
        let unread_packet = json!({"table":"owned","operations":[{"op":"insert","row":row(2,20)},{"op":"update","key":integer(1),"row":row(1,30)}]});
        let bytes = serde_json::to_vec(&unread_packet).unwrap();
        let headers = support::headers(
            "POST",
            &format!("/v1/projects/{}/auth/rows/write", f.id),
            &f.key,
            bytes.len(),
        )
        .replace(
            "\r\n\r\n",
            &format!(
                "\r\n{}: {access}\r\n\r\n",
                emilybase_server::USER_ACCESS_HEADER
            ),
        );
        let mut unread = support::socket(server.address).unwrap();
        unread.write_all(headers.as_bytes()).unwrap();
        unread.write_all(&bytes).unwrap();
        let public = f.root.join("registry").join(&f.id).join("data/redo.wal");
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
                "unread user packet did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            call(
                &server,
                access,
                "get",
                json!({"table":"owned","key":integer(1)})
            ),
            (200, json!({"row":row(1,30)}))
        );
        assert_eq!(
            call(
                &server,
                access,
                "get",
                json!({"table":"owned","key":integer(2)})
            ),
            (200, json!({"row":row(2,20)}))
        );
        drop(unread);
        clean_log(&server.kill(), &secrets);
        let image = capture_account_bundle_root(&f.root, pool()).unwrap();
        let copied = f._dir.path().join("user-data-copy");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        restore_account_bundle_bytes(&image, &copied, pool(), now).unwrap();
        let copy = support::Server::start_account(&copied, MASTER);
        assert_eq!(
            call(
                &copy,
                access,
                "get",
                json!({"table":"owned","key":integer(1)})
            )
            .0,
            401
        );
        let fresh = sign(&copy, &f.id, &f.key);
        let copied_access = fresh["access_token"].as_str().unwrap();
        assert_eq!(
            call(
                &copy,
                copied_access,
                "page",
                json!({"table":"owned","after":null,"limit":1})
            ),
            (200, json!({"rows":[row(1,30)],"next":integer(1)}))
        );
        clean_log(
            &copy.stop(),
            &[MASTER, &f.key, PASSWORD, access, copied_access],
        );
        let server = support::Server::start_account(&f.root, MASTER);
        let before = fs::read(&public).unwrap();
        assert_eq!(
            call(&server, access, "write", unread_packet),
            (403, json!({"code":"user_row_rejected"}))
        );
        assert_eq!(fs::read(&public).unwrap(), before);
        let deletion = json!({"table":"owned","operations":[{"op":"delete","key":integer(1)},{"op":"delete","key":integer(2)}]});
        let (status, receipt) = call(&server, access, "write", deletion);
        assert_eq!(status, 200);
        assert_eq!(receipt["changed"], 2);
        clean_log(&server.kill(), &secrets);
        let server = support::Server::start_account(&f.root, MASTER);
        assert_eq!(
            call(
                &server,
                access,
                "page",
                json!({"table":"owned","after":null,"limit":128})
            ),
            (200, json!({"rows":[],"next":null}))
        );
        clean_log(&server.stop(), &secrets);
    }
}
