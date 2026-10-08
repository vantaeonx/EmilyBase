use super::tests::{database, integer, invoke, point};
use super::*;
use emilybase_catalog::Value;
use serde_json::{Value as Json, json};
fn insert(key: i64, value: &str) -> Json {
    json!({"op":"insert","row":[integer(key),{"type":"text","value":value}]})
}
fn delete(key: i64) -> Json {
    json!({"op":"delete","key":integer(key)})
}
fn update(key: i64, value: &str) -> Json {
    json!({"op":"update","key":integer(key),"row":[integer(key),{"type":"text","value":value}]})
}
fn batch(operations: Vec<Json>) -> Json {
    json!({"table":"t","operations":operations})
}
#[test]
fn batch_commits_once_and_sees_prior_staged_writes_without_partial_failures() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut db = database(dir.path());
    let before = db.last_transaction();
    let reply = invoke(
        &mut db,
        Operation::Batch,
        batch(vec![
            insert(1, "before"),
            update(1, "after"),
            insert(2, "temporary"),
            delete(2),
        ]),
    )
    .unwrap();
    assert_eq!(
        reply,
        json!({"changed":4,"transaction":(before+1).to_string()})
    );
    assert_eq!(db.last_transaction(), before + 1);
    assert_eq!(
        invoke(&mut db, Operation::Get, point(1)).unwrap()["row"],
        json!([integer(1),{"type":"text","value":"after"}])
    );
    assert!(invoke(&mut db, Operation::Get, point(2)).unwrap()["row"].is_null());
    let committed = db.committed_wal().unwrap();
    for operations in [
        vec![insert(3, "not-committed"), insert(1, "duplicate")],
        vec![update(1, "not-committed"), delete(99)],
        vec![delete(1), delete(1)],
        vec![
            insert(3, "not-committed"),
            json!({"op":"update","key":integer(1),"row":[integer(7),{"type":"null"}]}),
        ],
        vec![
            insert(3, "not-committed"),
            json!({"op":"insert","row":[integer(5),{"type":"boolean","value":true}]}),
        ],
    ] {
        assert!(invoke(&mut db, Operation::Batch, batch(operations)).is_err());
        assert_eq!(db.committed_wal().unwrap(), committed);
        assert_eq!(db.last_transaction(), before + 1);
        assert!(invoke(&mut db, Operation::Get, point(3)).unwrap()["row"].is_null());
        assert_eq!(
            invoke(&mut db, Operation::Get, point(1)).unwrap()["row"][1],
            json!({"type":"text","value":"after"})
        );
    }
    drop(db);
    let mut db = Database::open(dir.path().join("data")).unwrap();
    assert_eq!(db.view().unwrap().row_count(), 1);
    assert_eq!(
        invoke(&mut db, Operation::Get, point(1)).unwrap()["row"][1],
        json!({"type":"text","value":"after"})
    );
}
#[test]
fn batch_event_bound_and_strict_documents_preserve_exact_committed_history() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut db = database(dir.path());
    let before = db.committed_wal().unwrap();
    for input in [
        batch(vec![]),
        batch((0..257).map(|n| insert(n, "x")).collect()),
        json!({"table":"t","operations":[{"op":"get","key":integer(1)}]}),
        json!({"table":"t","operations":[{"op":"insert","row":[integer(1),{"type":"null"}],"table":"other"}]}),
        json!({"table":"t","operations":[insert(1,"x")],"project":"synthetic-private"}),
    ] {
        assert!(invoke(&mut db, Operation::Batch, input).is_err());
        assert_eq!(db.committed_wal().unwrap(), before);
    }
    for bytes in [b"{\"table\":\"t\",\"operations\":[],\"operations\":[]}".to_vec(),b"{\"table\":\"t\",\"operations\":[{\"op\":\"delete\",\"op\":\"delete\",\"key\":{\"type\":\"integer\",\"value\":\"0\"}}]}".to_vec(),vec![b' ';crate::http::MAX_BODY+1]]{assert!(validate_row_request(Operation::Batch,&bytes).is_err());}
    let reply = invoke(
        &mut db,
        Operation::Batch,
        batch((0..256).map(|n| insert(n, "x")).collect()),
    )
    .unwrap();
    assert_eq!(reply["changed"], 256);
    assert_eq!(db.view().unwrap().row_count(), 256);
    assert!(
        serde_json::from_value::<wire::Input>(json!({"type":"null","value":null}))
            .unwrap()
            .value()
            .is_ok()
    );
}
proptest::proptest! {
 #![proptest_config(proptest::test_runner::Config::with_cases(16))]
 #[test]
 fn generated_batches_match_independent_all_or_nothing_model(batches in proptest::collection::vec(proptest::collection::vec((0..3_u8,-8..8_i64,0..100_u8),1..10),0..20)){
  let _serial=crate::durability::PROCESS_TESTS.blocking_lock();let dir=tempfile::tempdir().unwrap();let mut db=database(dir.path());let mut model=std::collections::BTreeMap::new();
  for steps in batches{let mut candidate=model.clone();let mut valid=true;let mut operations=Vec::new();
   for (op,key,value) in steps{let text=format!("synthetic-{value}");operations.push(match op{0=>insert(key,&text),1=>update(key,&text),_=>delete(key)});if !valid{continue;}
    match op{0=>{match candidate.entry(key){std::collections::btree_map::Entry::Occupied(_)=>valid=false,std::collections::btree_map::Entry::Vacant(e)=>{e.insert(text);}}},1=>{if let Some(current)=candidate.get_mut(&key){*current=text;}else{valid=false;}},_=>{if candidate.remove(&key).is_none(){valid=false;}}}
   }
   let before=db.committed_wal().unwrap();let last=db.last_transaction();let actual=invoke(&mut db,Operation::Batch,batch(operations.clone()));proptest::prop_assert_eq!(actual.is_ok(),valid);
   if valid{model=candidate;proptest::prop_assert_eq!(actual.unwrap()["changed"].clone(),json!(operations.len()));proptest::prop_assert_eq!(db.last_transaction(),last+1);}else{proptest::prop_assert_eq!(db.committed_wal().unwrap(),before);}
   let actual=invoke(&mut db,Operation::Page,json!({"table":"t","limit":128})).unwrap();let expected=model.iter().map(|(k,v)|json!([integer(*k),{"type":"text","value":v}])).collect::<Vec<_>>();proptest::prop_assert_eq!(actual["rows"].clone(),json!(expected));
  }
 }
}

#[test]
#[ignore = "native child helper; exercised by the precommit kill test"]
fn batch_stage_child() {
    let path = std::path::PathBuf::from(
        std::env::var_os("EMILYBASE_BATCH_TEST_PATH").expect("child database path"),
    );
    let phase = std::env::var("EMILYBASE_BATCH_TEST_PHASE").expect("child phase");
    let mut db = Database::open(&path).unwrap();
    let Prepared::Batch(table, operations) = decode(
        Operation::Batch,
        &serde_json::to_vec(&batch(vec![
            update(1, "synthetic-batch-after"),
            insert(2, "synthetic-batch-second"),
        ]))
        .unwrap(),
    )
    .unwrap() else {
        unreachable!()
    };
    let wait = || {
        std::fs::write(path.parent().unwrap().join("ready"), b"ready").unwrap();
        loop {
            std::thread::park();
        }
    };
    batch_after_stage(&mut db, &table, operations, || {
        if phase == "staged" {
            wait();
        }
    })
    .unwrap();
    wait();
}
#[test]
fn killed_batch_before_commit_never_appears_and_after_ack_recovers_on_both_wal_versions() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for phase in ["staged", "ack"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("data");
            let mut db = database(dir.path());
            let mut tx = db.begin().unwrap();
            tx.insert(
                "t",
                vec![
                    Value::Integer(1),
                    Value::Text("synthetic-batch-before".into()),
                ],
            )
            .unwrap();
            tx.commit().unwrap();
            if compact {
                db.compact().unwrap();
            }
            let before = db.committed_wal().unwrap();
            drop(db);
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "row_api::batch_tests::batch_stage_child",
                    "--test-threads=1",
                ])
                .env("EMILYBASE_BATCH_TEST_PATH", &path)
                .env("EMILYBASE_BATCH_TEST_PHASE", phase)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !dir.path().join("ready").exists() {
                if let Some(status) = child.try_wait().unwrap() {
                    panic!("batch child exited early: {status}");
                }
                if std::time::Instant::now() > deadline {
                    child.kill().unwrap();
                    let output = child.wait_with_output().unwrap();
                    panic!(
                        "batch stage deadline: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            child.kill().unwrap();
            assert!(!child.wait().unwrap().success());
            let mut db = Database::open(path).unwrap();
            let first = invoke(&mut db, Operation::Get, point(1)).unwrap();
            let second = invoke(&mut db, Operation::Get, point(2)).unwrap();
            if phase == "staged" {
                assert_eq!(db.committed_wal().unwrap(), before);
                assert_eq!(
                    first["row"][1],
                    json!({"type":"text","value":"synthetic-batch-before"})
                );
                assert!(second["row"].is_null());
            } else {
                assert_eq!(
                    first["row"][1],
                    json!({"type":"text","value":"synthetic-batch-after"})
                );
                assert_eq!(
                    second["row"][1],
                    json!({"type":"text","value":"synthetic-batch-second"})
                );
            }
        }
    }
}
