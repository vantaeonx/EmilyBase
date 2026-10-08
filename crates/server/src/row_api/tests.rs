use super::*;
use emilybase_catalog::{Column, DataType, Schema, Value};
use serde_json::{Value as Json, json};
fn schema() -> Schema {
    Schema {
        name: "t".into(),
        primary_key: 0,
        columns: vec![
            Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
            Column {
                name: "v".into(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    }
}
fn database(dir: &std::path::Path) -> Database {
    let mut db = Database::create(dir.join("data")).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(schema()).unwrap();
    tx.commit().unwrap();
    db
}
fn integer(value: i64) -> Json {
    json!({"type":"integer","value":value.to_string()})
}
fn invoke(db: &mut Database, op: Operation, input: Json) -> Result<Json> {
    let response = run(db, op, &serde_json::to_vec(&input).unwrap())?;
    let bytes = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(axum::body::to_bytes(
            response.into_body(),
            crate::http::MAX_BODY,
        ))
        .unwrap();
    Ok(serde_json::from_slice(&bytes).unwrap())
}
fn point(key: i64) -> Json {
    json!({"table":"t","key":integer(key)})
}
fn insertion(key: i64, value: &str) -> Json {
    json!({"table":"t","row":[integer(key),{"type":"text","value":value}]})
}
#[test]
fn point_crud_and_exclusive_pages_preserve_primary_order_and_exact_histories() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut db = database(dir.path());
    for id in [i64::MAX, 0, i64::MIN, 42] {
        let reply = invoke(&mut db, Operation::Insert, insertion(id, "synthetic-data")).unwrap();
        assert_eq!(reply["key"], integer(id));
        assert!(
            reply["transaction"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                > 1
        );
    }
    let before = db.committed_wal().unwrap();
    let first = invoke(&mut db, Operation::Page, json!({"table":"t","limit":2})).unwrap();
    assert_eq!(first["rows"][0][0], integer(i64::MIN));
    assert_eq!(first["rows"][1][0], integer(0));
    assert_eq!(first["next"], integer(0));
    let second = invoke(
        &mut db,
        Operation::Page,
        json!({"table":"t","after":first["next"],"limit":2}),
    )
    .unwrap();
    assert_eq!(second["rows"][0][0], integer(42));
    assert_eq!(second["rows"][1][0], integer(i64::MAX));
    assert!(second["next"].is_null());
    assert!(invoke(&mut db, Operation::Get, point(1)).unwrap()["row"].is_null());
    assert_eq!(
        invoke(&mut db, Operation::Get, point(i64::MAX)).unwrap()["row"][0],
        integer(i64::MAX)
    );
    assert_eq!(db.committed_wal().unwrap(), before);
    for (op, input) in [
        (Operation::Insert, insertion(0, "duplicate")),
        (Operation::Delete, point(1)),
        (
            Operation::Update,
            json!({"table":"t","key":integer(0),"row":[integer(2),{"type":"null"}]}),
        ),
    ] {
        assert!(invoke(&mut db, op, input).is_err());
        assert_eq!(db.committed_wal().unwrap(), before);
    }
    invoke(
        &mut db,
        Operation::Update,
        json!({"table":"t","key":integer(0),"row":[integer(0),{"type":"null"}]}),
    )
    .unwrap();
    invoke(&mut db, Operation::Delete, point(42)).unwrap();
    drop(db);
    let mut db = Database::open(dir.path().join("data")).unwrap();
    assert_eq!(
        invoke(&mut db, Operation::Get, point(0)).unwrap()["row"][1],
        json!({"type":"null"})
    );
    assert!(invoke(&mut db, Operation::Get, point(42)).unwrap()["row"].is_null());
    let gap = invoke(
        &mut db,
        Operation::Page,
        json!({"table":"t","after":integer(42),"limit":1}),
    )
    .unwrap();
    assert_eq!(gap["rows"][0][0], integer(i64::MAX));
    assert!(gap["next"].is_null());
}
#[test]
fn strict_numeric_wire_and_bad_request_bounds_refuse_without_wal_or_input_echo() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut db = database(dir.path());
    let before = db.committed_wal().unwrap();
    for value in [
        "+1",
        "01",
        "-0",
        " 1",
        "9223372036854775808",
        "-9223372036854775809",
        "",
    ] {
        assert!(
            invoke(
                &mut db,
                Operation::Get,
                json!({"table":"t","key":{"type":"integer","value":value}})
            )
            .is_err()
        );
    }
    for input in [
        json!({"table":"t","limit":0}),
        json!({"table":"t","limit":129}),
        json!({"table":"t","limit":1,"project":"synthetic-private"}),
        json!({"table":"t","limit":1,"after":{"type":"integer","value":1}}),
    ] {
        assert!(invoke(&mut db, Operation::Page, input).is_err());
    }
    for bytes in [
        b"{\"table\":\"t\",\"table\":\"t\",\"limit\":1}".to_vec(),
        b"synthetic-private".to_vec(),
        vec![b' '; crate::http::MAX_BODY + 1],
    ] {
        let error = run(&mut db, Operation::Page, &bytes).err().unwrap();
        assert!(!error.to_string().contains("synthetic-private"));
    }
    for value in [
        json!({"type":"float_bits","value":"7ff0000000000000"}),
        json!({"type":"float_bits","value":"fff8000000000001"}),
        json!({"type":"float_bits","value":"3FF0000000000000"}),
        json!({"type":"float_bits","value":"0"}),
        json!({"type":"text","value":"x".repeat(3073)}),
        json!({"type":"bytes","value":vec![0;3073]}),
        json!({"type":"integer","value":"0","extra":"synthetic-private"}),
    ] {
        assert!(
            serde_json::from_value::<Input>(value)
                .map_err(|_| TableError::Document)
                .and_then(Input::value)
                .is_err()
        );
    }
    assert!(
        invoke(
            &mut db,
            Operation::Insert,
            json!({"table":"t","row":vec![json!({"type":"null"});65]})
        )
        .is_err()
    );
    assert_eq!(db.committed_wal().unwrap(), before);
}
#[test]
fn byte_capped_page_refuses_whole_result_and_smaller_page_succeeds() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut db = database(dir.path());
    let payload = "\0".repeat(3072);
    let mut tx = db.begin().unwrap();
    for id in 0..5 {
        tx.insert("t", vec![Value::Integer(id), Value::Text(payload.clone())])
            .unwrap();
    }
    tx.commit().unwrap();
    let before = db.committed_wal().unwrap();
    assert!(matches!(
        invoke(&mut db, Operation::Page, json!({"table":"t","limit":5})),
        Err(TableError::Limit)
    ));
    let small = invoke(&mut db, Operation::Page, json!({"table":"t","limit":1})).unwrap();
    assert_eq!(small["rows"].as_array().unwrap().len(), 1);
    assert_eq!(small["next"], integer(0));
    assert_eq!(db.committed_wal().unwrap(), before);
}
#[test]
fn long_text_keys_and_all_value_variants_keep_lossless_wire() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::create(dir.path().join("data")).unwrap();
    let mut s = schema();
    s.primary_key = 1;
    s.columns[0].data_type = DataType::Float;
    s.columns[1].nullable = false;
    let mut tx = db.begin().unwrap();
    tx.create_table(s).unwrap();
    tx.commit().unwrap();
    let key = "界".repeat(1024);
    for bits in [0_u64, 1, 0x8000000000000000, 0x7fefffffffffffff] {
        let row = json!([{"type":"float_bits","value":format!("{bits:016x}")},{"type":"text","value":key}]);
        invoke(&mut db, Operation::Insert, json!({"table":"t","row":row})).unwrap();
        let found = invoke(
            &mut db,
            Operation::Get,
            json!({"table":"t","key":{"type":"text","value":key}}),
        )
        .unwrap();
        assert_eq!(found["row"], row);
        invoke(
            &mut db,
            Operation::Delete,
            json!({"table":"t","key":{"type":"text","value":key}}),
        )
        .unwrap();
    }
    for original in [
        Value::Null,
        Value::Boolean(true),
        Value::Integer(i64::MIN),
        Value::Bytes(vec![0, 255, 128]),
        Value::Text("\0' UNION SELECT;界".into()),
    ] {
        let encoded = serde_json::to_value(wire::Output::from(&original)).unwrap();
        let decoded = serde_json::from_value::<Input>(encoded)
            .unwrap()
            .value()
            .unwrap();
        assert_eq!(decoded, original);
    }
}
proptest::proptest! {
 #![proptest_config(proptest::test_runner::Config::with_cases(16))]
 #[test]
 fn generated_crud_matches_independent_ordered_rows(steps in proptest::collection::vec((0..3_u8,-12..12_i64,0..100_u8),0..40)){
  let _serial=crate::durability::PROCESS_TESTS.blocking_lock();let dir=tempfile::tempdir().unwrap();let mut db=database(dir.path());let mut model=std::collections::BTreeMap::new();
  for(op,key,v)in steps{let before=db.committed_wal().unwrap();let text=format!("synthetic-{v}");let existed=model.contains_key(&key);
   let success=match op{
    0=>{let actual=invoke(&mut db,Operation::Insert,insertion(key,&text));if !existed{model.insert(key,text);}proptest::prop_assert_eq!(actual.is_ok(),!existed);!existed}
    1=>{let actual=invoke(&mut db,Operation::Update,json!({"table":"t","key":integer(key),"row":[integer(key),{"type":"text","value":text}]}));if existed{model.insert(key,text);}proptest::prop_assert_eq!(actual.is_ok(),existed);existed}
    _=>{let actual=invoke(&mut db,Operation::Delete,point(key));model.remove(&key);proptest::prop_assert_eq!(actual.is_ok(),existed);existed}
   };
   if !success{proptest::prop_assert_eq!(db.committed_wal().unwrap(),before);}
   let actual=invoke(&mut db,Operation::Page,json!({"table":"t","limit":128})).unwrap();let expected=model.iter().map(|(k,v)|json!([integer(*k),{"type":"text","value":v}])).collect::<Vec<_>>();proptest::prop_assert_eq!(actual["rows"].clone(),json!(expected));proptest::prop_assert!(actual["next"].is_null());
  }
 }
 #[test]
 fn generated_finite_float_bits_roundtrip_exactly(bits in proptest::num::u64::ANY){
  let number=f64::from_bits(bits);let original=Value::Float(number);let encoded=serde_json::to_value(wire::Output::from(&original)).unwrap();let decoded=serde_json::from_value::<Input>(encoded).unwrap().value();
  if number.is_finite(){let Value::Float(actual)=decoded.unwrap()else{unreachable!()};proptest::prop_assert_eq!(actual.to_bits(),bits);}else{proptest::prop_assert!(decoded.is_err());}
 }
}
