use super::tests::{database, integer, invoke};
use super::*;
use emilybase_catalog::{Column, DataType, Schema, Value};
use serde_json::{Value as Json, json};
fn page(db: &mut Database, after: Option<Json>, limit: usize) -> Result<Json> {
    invoke(
        db,
        Operation::Page,
        json!({"table":"t","after":after,"limit":limit}),
    )
}
fn text(s: &str) -> Json {
    json!({"type":"text","value":s})
}
#[test]
fn maximum_page_uses_exclusive_current_state_after_deleted_cursor_and_insertions() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut db = database(dir.path());
    let mut tx = db.begin().unwrap();
    for n in 0..130 {
        tx.insert("t", vec![Value::Integer(n), Value::Null])
            .unwrap();
    }
    tx.commit().unwrap();
    let before = db.committed_wal().unwrap();
    let first = page(&mut db, None, 128).unwrap();
    assert_eq!(first["rows"].as_array().unwrap().len(), 128);
    assert_eq!(first["rows"][0][0], integer(0));
    assert_eq!(first["rows"][127][0], integer(127));
    assert_eq!(first["next"], integer(127));
    assert_eq!(db.committed_wal().unwrap(), before);
    let mut tx = db.begin().unwrap();
    tx.delete("t", &Key::Integer(127)).unwrap();
    tx.insert(
        "t",
        vec![
            Value::Integer(-1),
            Value::Text("synthetic-before-cursor".into()),
        ],
    )
    .unwrap();
    tx.insert(
        "t",
        vec![
            Value::Integer(130),
            Value::Text("synthetic-after-cursor".into()),
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    let before = db.committed_wal().unwrap();
    let second = page(&mut db, Some(first["next"].clone()), 128).unwrap();
    assert_eq!(second["rows"].as_array().unwrap().len(), 3);
    assert_eq!(second["rows"][0][0], integer(128));
    assert_eq!(second["rows"][2][0], integer(130));
    assert!(second["next"].is_null());
    assert_eq!(
        page(&mut db, Some(integer(i64::MAX)), 1).unwrap(),
        json!({"rows":[],"next":null})
    );
    assert_eq!(db.committed_wal().unwrap(), before);
}
#[test]
fn long_unicode_and_nul_text_keys_continue_after_deleted_bound_without_wal_writes() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::create(dir.path().join("data")).unwrap();
    let schema = Schema {
        name: "t".into(),
        primary_key: 0,
        columns: vec![Column {
            name: "key".into(),
            data_type: DataType::Text,
            nullable: false,
        }],
    };
    let keys = [
        "".to_string(),
        "\0".repeat(3072),
        "a".repeat(3072),
        "界".repeat(1024),
    ];
    let mut tx = db.begin().unwrap();
    tx.create_table(schema).unwrap();
    for key in &keys {
        tx.insert("t", vec![Value::Text(key.clone())]).unwrap();
    }
    tx.commit().unwrap();
    let before = db.committed_wal().unwrap();
    let first = page(&mut db, None, 2).unwrap();
    assert_eq!(first["rows"], json!([[text(&keys[0])], [text(&keys[1])]]));
    assert_eq!(first["next"], text(&keys[1]));
    assert_eq!(db.committed_wal().unwrap(), before);
    let mut tx = db.begin().unwrap();
    tx.delete("t", &Key::Text(keys[1].clone())).unwrap();
    tx.commit().unwrap();
    let before = db.committed_wal().unwrap();
    let rest = page(&mut db, Some(first["next"].clone()), 2).unwrap();
    assert_eq!(rest["rows"], json!([[text(&keys[2])], [text(&keys[3])]]));
    assert!(rest["next"].is_null());
    assert_eq!(db.committed_wal().unwrap(), before);
    assert!(page(&mut db, Some(integer(0)), 1).is_err());
    assert!(page(&mut db, Some(text(&"x".repeat(3073))), 1).is_err());
    assert_eq!(db.committed_wal().unwrap(), before);
}
proptest::proptest! {
 #![proptest_config(proptest::test_runner::Config::with_cases(16))]
 #[test]
 fn generated_changing_pages_match_independent_exclusive_model(steps in proptest::collection::vec((proptest::bool::ANY,-20..20_i64,-25..25_i64,1..9_usize),0..40)){
  let _serial=crate::durability::PROCESS_TESTS.blocking_lock();let dir=tempfile::tempdir().unwrap();let mut db=database(dir.path());let mut model=std::collections::BTreeSet::new();
  for (insert,key,after,limit)in steps{
   let mut tx=db.begin().unwrap();let changed=if insert{tx.insert("t",vec![Value::Integer(key),Value::Null]).is_ok()}else{tx.delete("t",&Key::Integer(key)).is_ok()};
   if changed{tx.commit().unwrap();if insert{model.insert(key);}else{model.remove(&key);}}else{drop(tx);}
   let before=db.committed_wal().unwrap();let actual=page(&mut db,Some(integer(after)),limit).unwrap();let expected=model.range((std::ops::Bound::Excluded(after),std::ops::Bound::Unbounded)).take(limit).copied().collect::<Vec<_>>();
   let more=model.range((std::ops::Bound::Excluded(after),std::ops::Bound::Unbounded)).count()>limit;
   proptest::prop_assert_eq!(actual["rows"].clone(),json!(expected.iter().map(|k|json!([integer(*k),{"type":"null"}])).collect::<Vec<_>>()));
   proptest::prop_assert_eq!(actual["next"].clone(),if more{integer(*expected.last().unwrap())}else{Json::Null});proptest::prop_assert_eq!(db.committed_wal().unwrap(),before);
  }
 }
}
