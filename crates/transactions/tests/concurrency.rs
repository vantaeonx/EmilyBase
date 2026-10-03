use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::{Database, Error};

#[test]
#[ignore = "subprocess helper, invoked by its parent test"]
fn counter_worker() {
    let Ok(path) = std::env::var("EMILYBASE_COUNTER_TEST_PATH") else {
        return;
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut db = loop {
        match Database::open(&path) {
            Ok(db) => break db,
            Err(Error::Wal(emilybase_wal::Error::Busy)) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("worker failed: {error}"),
        }
    };
    for index in 0..20 {
        let mut tx = db.begin().unwrap();
        let stored = tx
            .view()
            .unwrap()
            .get("counter", &Key::Integer(0))
            .unwrap()
            .unwrap();
        let Value::Integer(value) = stored[1] else {
            panic!("wrong counter type");
        };
        tx.update(
            "counter",
            &Key::Integer(0),
            vec![Value::Integer(0), Value::Integer(value + 1)],
        )
        .unwrap();
        tx.commit().unwrap();
        if index % 5 == 4 {
            db.compact().unwrap();
        }
    }
}

struct Workers(Vec<Child>);
impl Drop for Workers {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn four_competing_processes_serialize_eighty_updates_without_lost_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(Schema {
        name: "counter".into(),
        columns: vec![
            Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
            Column {
                name: "value".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
        ],
        primary_key: 0,
    })
    .unwrap();
    tx.insert("counter", vec![Value::Integer(0), Value::Integer(0)])
        .unwrap();
    tx.commit().unwrap();
    drop(db);
    let mut workers = Workers(Vec::new());
    for _ in 0..4 {
        workers.0.push(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "counter_worker", "--nocapture", "--ignored"])
                .env("EMILYBASE_COUNTER_TEST_PATH", &path)
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
    }
    for child in &mut workers.0 {
        assert!(child.wait().unwrap().success());
    }
    let db = Database::open(path).unwrap();
    assert_eq!(db.last_transaction(), 82);
    assert_eq!(
        db.view().unwrap().get("counter", &Key::Integer(0)).unwrap(),
        Some(&vec![Value::Integer(0), Value::Integer(80)])
    );
}
