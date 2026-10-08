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
