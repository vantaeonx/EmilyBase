#[allow(dead_code)]
mod support;
use emilybase_auth::{accounts::AccountStore, password::PasswordPool};
use emilybase_server::{ProjectStore, restore_account_bundle_bytes};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
static CASES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
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
    let _serial = CASES.blocking_lock();
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

#[test]
fn real_tcp_row_insert_replace_delete_ack_kills_recover_exact_typed_values() {
    let _serial = CASES.blocking_lock();
    for private in [false, true] {
        for compact in [false, true] {
            let f = fixture(private, compact);
            let before = f.private.as_ref().map(|p| fs::read(p).unwrap());
            let key = json!({"type":"text","value":"synthetic-row-key'\n界"});
            let row = |flag| json!([{"type":"boolean","value":flag},key]);
            let point = json!({"table":"temporary","key":key});
            let server = start(&f.root, private);
            assert_eq!(call(&server, &f, "POST", "/create", &schema()).0, 200);
            let (status, inserted) = call(
                &server,
                &f,
                "POST",
                "/rows/insert",
                &json!({"table":"temporary","row":row(false)}),
            );
            assert_eq!(status, 200);
            assert_eq!(inserted["key"], key);
            let mut transaction = inserted["transaction"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap();
            let first = server.kill();
            let server = start(&f.root, private);
            assert_eq!(
                call(&server, &f, "POST", "/rows/get", &point),
                (200, json!({"row":row(false)}))
            );
            assert_eq!(
                call(
                    &server,
                    &f,
                    "POST",
                    "/rows/insert",
                    &json!({"table":"temporary","row":row(false)})
                )
                .0,
                400
            );
            let (status, updated) = call(
                &server,
                &f,
                "POST",
                "/rows/update",
                &json!({"table":"temporary","key":key,"row":row(true)}),
            );
            assert_eq!(status, 200);
            let next = updated["transaction"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap();
            assert!(next > transaction);
            transaction = next;
            let second = server.kill();
            let server = start(&f.root, private);
            assert_eq!(
                call(&server, &f, "POST", "/rows/get", &point),
                (200, json!({"row":row(true)}))
            );
            assert_eq!(
                call(
                    &server,
                    &f,
                    "POST",
                    "/rows/page",
                    &json!({"table":"temporary","limit":1})
                ),
                (200, json!({"rows":[row(true)],"next":null}))
            );
            let (status, deleted) = call(&server, &f, "POST", "/rows/delete", &point);
            assert_eq!(status, 200);
            assert!(
                deleted["transaction"]
                    .as_str()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
                    > transaction
            );
            let third = server.kill();
            let server = start(&f.root, private);
            assert_eq!(
                call(&server, &f, "POST", "/rows/get", &point),
                (200, json!({"row":null}))
            );
            assert_eq!(
                call(
                    &server,
                    &f,
                    "POST",
                    "/rows/page",
                    &json!({"table":"temporary","limit":1})
                ),
                (200, json!({"rows":[],"next":null}))
            );
            assert_eq!(call(&server, &f, "POST", "/rows/delete", &point).0, 400);
            let fourth = server.stop();
            if let Some(path) = &f.private {
                assert_eq!(fs::read(path).unwrap(), before.unwrap());
            }
            for log in [&first, &second, &third, &fourth] {
                for secret in [
                    MASTER,
                    &f.id,
                    &f.key,
                    "temporary",
                    "synthetic-row-key",
                    r"synthetic-row-key'\n",
                ] {
                    assert!(!log.contains(secret));
                }
            }
        }
    }
}

#[test]
fn real_tcp_batches_ack_atomically_and_late_rejections_survive_restart_on_both_modes_and_formats() {
    let _serial = CASES.blocking_lock();
    for private in [false, true] {
        for compact in [false, true] {
            let f = fixture(private, compact);
            let before = f.private.as_ref().map(|p| fs::read(p).unwrap());
            let server = start(&f.root, private);
            assert_eq!(call(&server, &f, "POST", "/create", &schema()).0, 200);
            let key = |text| json!({"type":"text","value":text});
            let row = |flag, text| json!([{"type":"boolean","value":flag},key(text)]);
            let payload = json!({"table":"temporary","operations":[{"op":"insert","row":row(false,"synthetic-batch-key")},{"op":"update","key":key("synthetic-batch-key"),"row":row(true,"synthetic-batch-key")},{"op":"insert","row":row(false,"synthetic-batch-gone")},{"op":"delete","key":key("synthetic-batch-gone")}]});
            let (status, changed) = call(&server, &f, "POST", "/rows/batch", &payload);
            assert_eq!(status, 200);
            assert_eq!(changed["changed"], 4);
            assert!(
                changed["transaction"]
                    .as_str()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
                    > 1
            );
            let first = server.kill();
            let server = start(&f.root, private);
            let point = json!({"table":"temporary","key":key("synthetic-batch-key")});
            assert_eq!(
                call(&server, &f, "POST", "/rows/get", &point),
                (200, json!({"row":row(true,"synthetic-batch-key")}))
            );
            assert_eq!(
                call(
                    &server,
                    &f,
                    "POST",
                    "/rows/get",
                    &json!({"table":"temporary","key":key("synthetic-batch-gone")})
                ),
                (200, json!({"row":null}))
            );
            let rejected = json!({"table":"temporary","operations":[{"op":"delete","key":key("synthetic-batch-key")},{"op":"delete","key":key("synthetic-batch-missing")}]});
            assert_eq!(call(&server, &f, "POST", "/rows/batch", &rejected).0, 400);
            let second = server.kill();
            let server = start(&f.root, private);
            assert_eq!(
                call(
                    &server,
                    &f,
                    "POST",
                    "/rows/page",
                    &json!({"table":"temporary","limit":1})
                ),
                (
                    200,
                    json!({"rows":[row(true,"synthetic-batch-key")],"next":null})
                )
            );
            let third = server.stop();
            if let Some(path) = &f.private {
                assert_eq!(fs::read(path).unwrap(), before.unwrap());
            }
            for log in [&first, &second, &third] {
                for secret in [
                    MASTER,
                    &f.id,
                    &f.key,
                    "temporary",
                    "synthetic-batch-key",
                    "synthetic-batch-gone",
                ] {
                    assert!(!log.contains(secret));
                }
            }
        }
    }
}

#[test]
fn simultaneous_tcp_batches_admit_or_refuse_without_losing_acknowledged_packets() {
    let _serial = CASES.blocking_lock();
    for private in [false, true] {
        for compact in [false, true] {
            let f = fixture(private, compact);
            let before = f.private.as_ref().map(|p| fs::read(p).unwrap());
            let server = start(&f.root, private);
            assert_eq!(call(&server, &f, "POST", "/create", &schema()).0, 200);
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(32));
            let clients = (0..32)
                .map(|actor| {
                    let barrier = barrier.clone();
                    let address = server.address;
                    let route = format!("/v1/projects/{}/tables/rows/batch", f.id);
                    let credential = f.key.clone();
                    std::thread::spawn(move || {
                        let key = |suffix| json!({"type":"text","value":format!("synthetic-load-{actor:02}-{suffix}")});
                        let row = |flag, suffix| json!([{"type":"boolean","value":flag},key(suffix)]);
                        let input = json!({"table":"temporary","operations":[
                            {"op":"insert","row":row(false,"a")},
                            {"op":"update","key":key("a"),"row":row(true,"a")},
                            {"op":"insert","row":row(false,"b")},
                            {"op":"insert","row":row(false,"discarded")},
                            {"op":"delete","key":key("discarded")}
                        ]});
                        barrier.wait();
                        let (status, reply) = support::call(address,"POST",&route,&credential,&input).unwrap();
                        match status {
                            200 => {
                                assert_eq!(reply["changed"],5);
                                assert!(reply["transaction"].as_str().unwrap().parse::<u64>().unwrap()>1);
                                Some(actor)
                            }
                            429 => {assert_eq!(reply["code"],"rate_limit");None}
                            503 => {assert_eq!(reply["code"],"workers_busy");None}
                            _ => panic!("unexpected synthetic pressure status {status}: {reply}"),
                        }
                    })
                })
                .collect::<Vec<_>>();
            // Join every client before checking failures, so the server outlives all calls.
            let joined = clients.into_iter().map(|t| t.join()).collect::<Vec<_>>();
            let acknowledged = joined
                .into_iter()
                .filter_map(|r| r.unwrap())
                .collect::<Vec<_>>();
            assert!(!acknowledged.is_empty());
            let logs = server.kill();
            let data = if private {
                f.root.join("registry").join(&f.id).join("data")
            } else {
                f.root.join(&f.id).join("data")
            };
            let db = emilybase_transactions::Database::open(data).unwrap();
            let view = db.view().unwrap();
            let actual = view
                .primary_rows("temporary", None, None)
                .unwrap()
                .map(|r| {
                    let r = r.unwrap();
                    let emilybase_catalog::Value::Text(key) = &r[1] else {
                        panic!("expected text key")
                    };
                    let emilybase_catalog::Value::Boolean(flag) = r[0] else {
                        panic!("expected boolean")
                    };
                    (key.clone(), flag)
                })
                .collect::<std::collections::BTreeMap<_, _>>();
            let expected = acknowledged
                .into_iter()
                .flat_map(|actor| {
                    [
                        (format!("synthetic-load-{actor:02}-a"), true),
                        (format!("synthetic-load-{actor:02}-b"), false),
                    ]
                })
                .collect::<std::collections::BTreeMap<_, _>>();
            assert_eq!(actual, expected);
            if let Some(path) = &f.private {
                assert_eq!(fs::read(path).unwrap(), before.unwrap());
            }
            for secret in [MASTER, &f.id, &f.key, "temporary", "synthetic-load-"] {
                assert!(!logs.contains(secret));
            }
        }
    }
}
