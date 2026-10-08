#[allow(dead_code)]
mod support;
use emilybase_auth::{accounts::AccountStore, password::PasswordPool};
use emilybase_server::{ProjectStore, restore_account_bundle_bytes};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
const MASTER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
fn schema() -> Value {
    json!({"name":"temporary","columns":[{"name":"flag","data_type":"boolean","nullable":true},{"name":"key","data_type":"text","nullable":false}],"primary_key":1})
}
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    id: String,
    key: String,
    private: Option<PathBuf>,
}
fn fixture(private: bool, compact: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let registry = dir.path().join("registry");
    let mut store = ProjectStore::open(&registry).unwrap();
    let created = store.create("synthetic-schema").unwrap();
    store
        .authorize(&created.project.id, &created.api_key)
        .unwrap()
        .execute(
            "CREATE TABLE t(id INT PRIMARY KEY);INSERT INTO t VALUES(1)",
            &[],
        )
        .unwrap();
    if compact {
        emilybase_transactions::Database::open(registry.join(&created.project.id).join("data"))
            .unwrap()
            .compact()
            .unwrap();
    }
    let root = if private {
        let pool = PasswordPool::new(1).unwrap();
        let mut account = AccountStore::create(
            dir.path().join("private"),
            &created.project.id,
            pool.clone(),
        )
        .unwrap();
        if compact {
            account.compact().unwrap();
        }
        let image = store.capture_account_bundle(&mut [account]).unwrap();
        let target = dir.path().join("root");
        restore_account_bundle_bytes(&image, &target, pool, 0).unwrap();
        target
    } else {
        registry
    };
    drop(store);
    let private_path = private.then(|| {
        root.join("private")
            .join(&created.project.id)
            .join("redo.wal")
    });
    Fixture {
        _dir: dir,
        root,
        id: created.project.id,
        key: created.api_key,
        private: private_path,
    }
}
fn start(root: &Path, private: bool) -> support::Server {
    if private {
        support::Server::start_account(root, MASTER)
    } else {
        support::Server::start(root, MASTER)
    }
}
fn call(
    server: &support::Server,
    f: &Fixture,
    method: &str,
    suffix: &str,
    input: &Value,
) -> (u16, Value) {
    support::call(
        server.address,
        method,
        &format!("/v1/projects/{}/tables{suffix}", f.id),
        &f.key,
        input,
    )
    .unwrap()
}
#[test]
fn real_tcp_schema_create_and_drop_ack_kills_preserve_ids_rows_and_private_wal() {
    for private in [false, true] {
        for compact in [false, true] {
            let f = fixture(private, compact);
            let before = f.private.as_ref().map(|p| fs::read(p).unwrap());
            let server = start(&f.root, private);
            assert_eq!(
                call(&server, &f, "GET", "", &json!({})).1["tables"][0]["id"],
                "1"
            );
            let (status, created) = call(&server, &f, "POST", "/create", &schema());
            assert_eq!(status, 200);
            assert_eq!(created["table"]["id"], "2");
            let first = server.kill();
            let server = start(&f.root, private);
            assert_eq!(
                call(
                    &server,
                    &f,
                    "POST",
                    "/schema",
                    &json!({"table":"temporary"})
                ),
                (200, schema())
            );
            assert_eq!(
                support::call(
                    server.address,
                    "POST",
                    &format!("/v1/projects/{}/sql", f.id),
                    &f.key,
                    &json!({"sql":"INSERT INTO temporary VALUES(true,'synthetic-key')"})
                )
                .unwrap()
                .0,
                200
            );
            let (status, dropped) =
                call(&server, &f, "POST", "/drop", &json!({"table":"temporary"}));
            assert_eq!(status, 200);
            assert!(
                dropped["transaction"]
                    .as_str()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
                    > 2
            );
            let second = server.kill();
            let server = start(&f.root, private);
            assert_eq!(
                call(&server, &f, "GET", "", &json!({})).1["tables"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                call(
                    &server,
                    &f,
                    "POST",
                    "/schema",
                    &json!({"table":"temporary"})
                )
                .0,
                400
            );
            let (status, recreated) = call(&server, &f, "POST", "/create", &schema());
            assert_eq!(status, 200);
            assert_eq!(recreated["table"]["id"], "3");
            let (status, empty) = support::call(
                server.address,
                "POST",
                &format!("/v1/projects/{}/sql", f.id),
                &f.key,
                &json!({"sql":"SELECT * FROM temporary"}),
            )
            .unwrap();
            assert_eq!(status, 200);
            assert!(empty["results"][0]["rows"].as_array().unwrap().is_empty());
            let third = server.stop();
            if let Some(path) = &f.private {
                assert_eq!(fs::read(path).unwrap(), before.unwrap());
            }
            for log in [&first, &second, &third] {
                for secret in [MASTER, &f.id, &f.key, "temporary", "synthetic-key"] {
                    assert!(!log.contains(secret));
                }
            }
        }
    }
}
