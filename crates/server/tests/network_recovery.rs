mod support;
use emilybase_auth::issue_key;
use emilybase_catalog::{Key, Value};
use emilybase_server::{Error, ProjectStore};
use emilybase_transactions::Database;
use serde_json::json;
use std::collections::BTreeSet;
use std::io::Write;
use std::sync::{Arc, Barrier};
use std::time::Duration;

// Avoid fork/exec briefly inheriting unrelated test owners.
static PROCESS_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn killed_http_writer_recovers_every_received_response_and_complete_atomic_prefix() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    for compact in [false, true] {
        for threshold in [5, 20, 60] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("projects");
            let mut store = ProjectStore::open(&root).unwrap();
            let project = store.create("writer").unwrap();
            let sibling = store.create("untouched").unwrap();
            store
                .authorize(&project.project.id, &project.api_key)
                .unwrap()
                .execute(
                    "CREATE TABLE t(id INT PRIMARY KEY,value INT); INSERT INTO t VALUES(0,0)",
                    &[],
                )
                .unwrap();
            let data = root.join(&project.project.id).join("data");
            let sibling_data = root.join(&sibling.project.id).join("data/redo.wal");
            let untouched = std::fs::read(&sibling_data).unwrap();
            drop(store);
            let mut database = Database::open(&data).unwrap();
            if compact {
                database.compact().unwrap();
            }
            let base = database.last_transaction();
            drop(database);
            let master = issue_key().unwrap();
            let server = support::Server::start(&root, &master);
            let address = server.address;
            let path = format!("/v1/projects/{}/sql", project.project.id);
            let key = project.api_key.clone();
            let (send, receive) = std::sync::mpsc::channel();
            let writer = std::thread::spawn(move || {
                let mut acknowledgments = Vec::new();
                for id in 1..=100 {
                    let sql = "BEGIN; INSERT INTO t VALUES ($1,$1); UPDATE t SET value=$1 WHERE id=0; SELECT value FROM t WHERE id=0; COMMIT";
                    let payload = json!({"sql":sql,"parameters":[{"type":"integer","value":id}]});
                    let Ok((status, report)) =
                        support::call(address, "POST", &path, &key, &payload)
                    else {
                        break;
                    };
                    assert_eq!(status, 200);
                    assert_eq!(report["committed"], true);
                    let transaction = report["transaction"].as_u64().unwrap();
                    assert_eq!(transaction, base + id as u64);
                    assert_eq!(report["results"][2]["rows"][0][0]["value"], id);
                    acknowledgments.push(transaction);
                    if send.send(transaction).is_err() {
                        break;
                    }
                }
                acknowledgments
            });
            for _ in 0..threshold {
                receive.recv_timeout(Duration::from_secs(20)).unwrap();
            }
            let log = server.kill();
            let acknowledged = writer.join().unwrap();
            assert!(acknowledged.len() >= threshold);
            for private in [
                master.as_str(),
                project.api_key.as_str(),
                project.project.id.as_str(),
                "INSERT INTO",
            ] {
                assert!(!log.contains(private));
            }
            let mut database = Database::open(&data).unwrap();
            let committed = database.last_transaction() - base;
            assert!(committed >= acknowledged.len() as u64);
            assert_eq!(database.view().unwrap().row_count(), committed as usize + 1);
            assert_eq!(
                database
                    .view()
                    .unwrap()
                    .get("t", &Key::Integer(0))
                    .unwrap()
                    .unwrap()[1],
                Value::Integer(committed as i64)
            );
            for id in 1..=committed {
                assert_eq!(
                    database
                        .view()
                        .unwrap()
                        .get("t", &Key::Integer(id as i64))
                        .unwrap()
                        .unwrap(),
                    &vec![Value::Integer(id as i64), Value::Integer(id as i64)]
                );
            }
            for (index, transaction) in acknowledged.iter().enumerate() {
                assert_eq!(*transaction, base + index as u64 + 1);
            }
            let report =
                emilybase_query::execute(&mut database, "INSERT INTO t VALUES (-1,-1)", &[])
                    .unwrap();
            assert_eq!(report.transaction, base + committed + 1);
            drop(database);
            assert_eq!(std::fs::read(&sibling_data).unwrap(), untouched);
            let store = ProjectStore::open(&root).unwrap();
            assert_eq!(
                store
                    .authorize(&project.project.id, &project.api_key)
                    .unwrap()
                    .status()
                    .unwrap()
                    .rows,
                committed as usize + 2
            );
            assert_eq!(
                store
                    .authorize(&sibling.project.id, &sibling.api_key)
                    .unwrap()
                    .status()
                    .unwrap()
                    .rows,
                0
            );
        }
    }
}
#[test]
fn rollback_and_rejected_http_scripts_remain_absent_after_process_kill() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("projects");
    let mut store = ProjectStore::open(&root).unwrap();
    let project = store.create("rollback").unwrap();
    store
        .authorize(&project.project.id, &project.api_key)
        .unwrap()
        .execute("CREATE TABLE t(id INT PRIMARY KEY)", &[])
        .unwrap();
    drop(store);
    let wal = root.join(&project.project.id).join("data/redo.wal");
    let original = std::fs::read(&wal).unwrap();
    let server = support::Server::start(&root, &issue_key().unwrap());
    let path = format!("/v1/projects/{}/sql", project.project.id);
    for sql in [
        "BEGIN; INSERT INTO t VALUES (1); ROLLBACK",
        "INSERT INTO t VALUES (2); INSERT INTO t VALUES (2)",
    ] {
        let (status, report) = support::call(
            server.address,
            "POST",
            &path,
            &project.api_key,
            &json!({"sql":sql}),
        )
        .unwrap();
        if sql.starts_with("BEGIN") {
            assert_eq!(status, 200);
            assert_eq!(report["committed"], false);
        } else {
            assert_eq!(status, 400);
            assert_eq!(report, json!({"code":"query_rejected"}));
        }
    }
    server.kill();
    assert_eq!(std::fs::read(&wal).unwrap(), original);
    let store = ProjectStore::open(root).unwrap();
    let status = store
        .authorize(&project.project.id, &project.api_key)
        .unwrap()
        .status()
        .unwrap();
    assert_eq!(status.transaction, 2);
    assert_eq!(status.rows, 0);
}
#[test]
fn concurrent_tcp_projects_keep_unique_rows_atomic_ids_and_private_logs() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("projects");
    let mut store = ProjectStore::open(&root).unwrap();
    let first = store.create("first").unwrap();
    let second = store.create("second").unwrap();
    for project in [&first, &second] {
        store
            .authorize(&project.project.id, &project.api_key)
            .unwrap()
            .execute("CREATE TABLE t(id INT PRIMARY KEY,value TEXT)", &[])
            .unwrap();
    }
    drop(store);
    let master = issue_key().unwrap();
    let server = support::Server::start(&root, &master);
    let barrier = Arc::new(Barrier::new(4));
    let mut writers = Vec::new();
    for worker in 0..4 {
        let project = if worker % 2 == 0 { &first } else { &second };
        let id = project.project.id.clone();
        let key = project.api_key.clone();
        let address = server.address;
        let barrier = barrier.clone();
        writers.push(std::thread::spawn(move || {
            let path=format!("/v1/projects/{id}/sql");let mut transactions=Vec::new();barrier.wait();
            for index in 1..=8 {
                let row=(worker/2)*8+index;
                let payload=json!({"sql":"INSERT INTO t VALUES ($1,$2)","parameters":[{"type":"integer","value":row},{"type":"text","value":format!("worker-{worker}")} ]});
                let (status,report)=support::call(address,"POST",&path,&key,&payload).unwrap();assert_eq!(status,200);assert_eq!(report["committed"],true);
                transactions.push(report["transaction"].as_u64().unwrap());
            }
            let payload=json!({"sql":format!("INSERT INTO t VALUES ({},'staged'); INSERT INTO t VALUES (1,'duplicate')",100+worker)});
            assert_eq!(support::call(address,"POST",&path,&key,&payload).unwrap().0,400);
            (worker%2,transactions)
        }));
    }
    let mut ids = [BTreeSet::new(), BTreeSet::new()];
    for writer in writers {
        let (project, transactions) = writer.join().unwrap();
        for transaction in transactions {
            assert!(ids[project].insert(transaction));
        }
    }
    for transactions in &ids {
        assert_eq!(
            transactions.iter().copied().collect::<Vec<_>>(),
            (3..=18).collect::<Vec<_>>()
        );
    }
    let log = server.stop();
    for private in [
        master.as_str(),
        first.api_key.as_str(),
        second.api_key.as_str(),
        first.project.id.as_str(),
        second.project.id.as_str(),
        "worker-",
        "INSERT INTO",
    ] {
        assert!(!log.contains(private));
    }
    let store = ProjectStore::open(&root).unwrap();
    for (index, project) in [&first, &second].iter().enumerate() {
        let report = store
            .authorize(&project.project.id, &project.api_key)
            .unwrap()
            .execute("SELECT * FROM t ORDER BY id", &[])
            .unwrap();
        assert_eq!(report.transaction, 18);
        assert_eq!(report.results[0].rows.len(), 16);
        for (row_index, row) in report.results[0].rows.iter().enumerate() {
            assert_eq!(row[0], Value::Integer(row_index as i64 + 1));
            assert_eq!(
                row[1],
                Value::Text(format!(
                    "worker-{}",
                    if row_index < 8 { index } else { index + 2 }
                ))
            );
        }
    }
}
#[test]
fn sigterm_drains_an_accepted_partial_body_before_releasing_directory_ownership() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("projects");
    let mut store = ProjectStore::open(&root).unwrap();
    let project = store.create("drain").unwrap();
    drop(store);
    let server = support::Server::start(&root, &issue_key().unwrap());
    let path = format!("/v1/projects/{}/sql", project.project.id);
    let body =
        json!({"sql":"CREATE TABLE t(id INT PRIMARY KEY); INSERT INTO t VALUES (7)"}).to_string();
    let mut socket = support::socket(server.address).unwrap();
    socket
        .write_all(support::headers("POST", &path, &project.api_key, body.len()).as_bytes())
        .unwrap();
    socket
        .write_all(&body.as_bytes()[..body.len() / 2])
        .unwrap();
    std::thread::sleep(Duration::from_millis(50));
    server.signal();
    std::thread::sleep(Duration::from_millis(50));
    assert!(matches!(ProjectStore::open(&root), Err(Error::Busy)));
    socket
        .write_all(&body.as_bytes()[body.len() / 2..])
        .unwrap();
    let (status, report) = support::read(socket).unwrap();
    assert_eq!(status, 200);
    assert_eq!(report["committed"], true);
    server.drain();
    let store = ProjectStore::open(root).unwrap();
    let status = store
        .authorize(&project.project.id, &project.api_key)
        .unwrap()
        .status()
        .unwrap();
    assert_eq!(status.transaction, 2);
    assert_eq!(status.rows, 1);
}

#[test]
fn missing_or_corrupt_project_wal_fails_closed_while_other_projects_stay_available() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    for missing in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("projects");
        let mut store = ProjectStore::open(&root).unwrap();
        let broken = store.create("broken").unwrap();
        let healthy = store.create("healthy").unwrap();
        for project in [&broken, &healthy] {
            store
                .authorize(&project.project.id, &project.api_key)
                .unwrap()
                .execute(
                    "CREATE TABLE t(id INT PRIMARY KEY);INSERT INTO t VALUES (7)",
                    &[],
                )
                .unwrap();
        }
        let data = root.join(&broken.project.id).join("data");
        drop(store);
        Database::open(&data).unwrap().checkpoint().unwrap();
        let checkpoint = std::fs::read(data.join("checkpoint.emily")).unwrap();
        let wal = data.join("redo.wal");
        let damaged = if missing {
            std::fs::remove_file(&wal).unwrap();
            None
        } else {
            let mut bytes = std::fs::read(&wal).unwrap();
            bytes[0] ^= 0xff;
            std::fs::write(&wal, &bytes).unwrap();
            Some(bytes)
        };
        let server = support::Server::start(&root, &issue_key().unwrap());
        for suffix in ["status", "sql"] {
            let method = if suffix == "status" { "GET" } else { "POST" };
            let path = format!("/v1/projects/{}/{suffix}", broken.project.id);
            let (status, problem) = support::call(
                server.address,
                method,
                &path,
                &broken.api_key,
                &json!({"sql":"SELECT * FROM t"}),
            )
            .unwrap();
            assert_eq!(status, 503);
            assert_eq!(problem, json!({"code":"storage_unavailable"}));
        }
        let path = format!("/v1/projects/{}/sql", healthy.project.id);
        let (status, report) = support::call(
            server.address,
            "POST",
            &path,
            &healthy.api_key,
            &json!({"sql":"SELECT * FROM t"}),
        )
        .unwrap();
        assert_eq!(status, 200);
        assert_eq!(report["results"][0]["rows"][0][0]["value"], 7);
        server.stop();
        assert_eq!(
            std::fs::read(data.join("checkpoint.emily")).unwrap(),
            checkpoint
        );
        if let Some(damaged) = damaged {
            assert_eq!(std::fs::read(wal).unwrap(), damaged);
        } else {
            assert!(!wal.exists());
        }
    }
}

#[test]
fn corrupt_optional_checkpoint_is_ignored_by_http_and_replaced_only_by_explicit_checkpoint() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("projects");
    let mut store = ProjectStore::open(&root).unwrap();
    let project = store.create("checkpoint").unwrap();
    store
        .authorize(&project.project.id, &project.api_key)
        .unwrap()
        .execute(
            "CREATE TABLE t(id INT PRIMARY KEY);INSERT INTO t VALUES (42)",
            &[],
        )
        .unwrap();
    drop(store);
    let data = root.join(&project.project.id).join("data");
    Database::open(&data).unwrap().checkpoint().unwrap();
    let wal = std::fs::read(data.join("redo.wal")).unwrap();
    std::fs::write(data.join("checkpoint.emily"), b"damaged cache").unwrap();
    let server = support::Server::start(&root, &issue_key().unwrap());
    let path = format!("/v1/projects/{}/sql", project.project.id);
    let (status, report) = support::call(
        server.address,
        "POST",
        &path,
        &project.api_key,
        &json!({"sql":"SELECT * FROM t"}),
    )
    .unwrap();
    assert_eq!(status, 200);
    assert_eq!(report["transaction"], 2);
    assert_eq!(report["results"][0]["rows"][0][0]["value"], 42);
    server.stop();
    assert_eq!(std::fs::read(data.join("redo.wal")).unwrap(), wal);
    assert_eq!(
        std::fs::read(data.join("checkpoint.emily")).unwrap(),
        b"damaged cache"
    );
    let mut database = Database::open(&data).unwrap();
    assert_eq!(database.last_transaction(), 2);
    database.checkpoint().unwrap();
    assert_ne!(
        std::fs::read(data.join("checkpoint.emily")).unwrap(),
        b"damaged cache"
    );
}
