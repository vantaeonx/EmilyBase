use super::*;
use emilybase_catalog::{Column, DataType};
use serde_json::json;
fn schema(name: &str) -> Schema {
    Schema {
        name: name.into(),
        primary_key: 1,
        columns: vec![
            Column {
                name: "flag".into(),
                data_type: DataType::Boolean,
                nullable: true,
            },
            Column {
                name: "key".into(),
                data_type: DataType::Text,
                nullable: false,
            },
        ],
    }
}
#[test]
fn typed_schema_create_describe_list_drop_and_recreate_preserve_live_ids() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::create(dir.path().join("data")).unwrap();
    let original = schema("t");
    let first = create(&mut db, original.clone()).unwrap();
    assert_eq!(first.table.id, "1");
    assert_eq!(first.transaction, "2");
    assert_eq!(describe(&mut db, "t").unwrap(), original);
    let before = db.committed_wal().unwrap();
    let list = list(&mut db).unwrap();
    assert_eq!(
        serde_json::to_value(&list).unwrap(),
        json!({"tables":[{"id":"1","name":"t","columns":2,"primary_key":"key"}]})
    );
    assert_eq!(db.committed_wal().unwrap(), before);
    assert!(create(&mut db, original.clone()).is_err());
    assert_eq!(db.committed_wal().unwrap(), before);
    assert_eq!(drop_table(&mut db, "t").unwrap().transaction, "3");
    assert_eq!(create(&mut db, original).unwrap().table.id, "2");
    assert_eq!(db.view().unwrap().row_count(), 0);
    assert!(drop_table(&mut db, "missing").is_err());
}
#[test]
fn maximum_live_table_inventory_keeps_metadata_under_transport_cap() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::create(dir.path().join("data")).unwrap();
    let mut tx = db.begin().unwrap();
    for i in 0..emilybase_database::MAX_TABLES {
        let mut schema = schema(&format!("{}_{i:03}", "t".repeat(59)));
        schema.columns[1].name = "k".repeat(63);
        for n in 2..64 {
            schema.columns.push(Column {
                name: format!("c{n}"),
                data_type: DataType::Bytes,
                nullable: true,
            });
        }
        tx.create_table(schema).unwrap();
    }
    tx.commit().unwrap();
    let before = db.committed_wal().unwrap();
    let tables = list(&mut db).unwrap();
    assert_eq!(tables.tables.len(), 128);
    assert_eq!(tables.tables.first().unwrap().id, "1");
    assert_eq!(tables.tables.last().unwrap().id, "128");
    let encoded = serde_json::to_vec(&tables).unwrap();
    assert!(encoded.len() < crate::http::MAX_BODY);
    assert_eq!(db.committed_wal().unwrap(), before);
}
#[test]
fn schema_input_is_strict_bounded_and_does_not_echo_untrusted_fields() {
    let original = serde_json::to_value(schema("t")).unwrap();
    let mut inputs = vec![b"{}".to_vec(), b"{\"name\":\"t\",\"name\":\"t\"}".to_vec()];
    for (key, value) in [
        ("name", json!("../synthetic-private")),
        ("columns", json!([])),
        ("primary_key", json!(65535)),
        ("extra", json!("synthetic-private-content")),
    ] {
        let mut invalid = original.clone();
        invalid[key] = value;
        inputs.push(serde_json::to_vec(&invalid).unwrap());
    }
    let mut too_many = original.clone();
    too_many["columns"] = json!(vec![original["columns"][0].clone(); 65]);
    inputs.push(serde_json::to_vec(&too_many).unwrap());
    for input in inputs {
        let error = schema_request(&input).unwrap_err();
        assert!(!error.to_string().contains("synthetic-private"));
    }
    assert_eq!(
        schema_request(&serde_json::to_vec(&original).unwrap()).unwrap(),
        schema("t")
    );
    for input in [
        b"{}".as_slice(),
        b"{\"table\":1}",
        b"{\"table\":\"t\",\"table\":\"t\"}",
        b"{\"table\":\"t\",\"path\":\"synthetic-private\"}",
    ] {
        assert!(name_request(input).is_err());
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(16))]
    #[test]
    fn generated_live_catalog_and_recreated_ids_match_independent_model(steps in proptest::collection::vec((proptest::bool::ANY,0..8_u8),0..40)){
        let _serial=crate::durability::PROCESS_TESTS.blocking_lock();let dir=tempfile::tempdir().unwrap();let mut db=Database::create(dir.path().join("data")).unwrap();
        let mut expected=std::collections::BTreeMap::<String,u64>::new();let mut next=1_u64;
        for (insert,slot) in steps {
            let name=format!("t{slot}");let before=db.committed_wal().unwrap();
            if insert{
                let result=create(&mut db,schema(&name));
                match expected.entry(name.clone()) {
                    std::collections::btree_map::Entry::Occupied(_)=>{proptest::prop_assert!(result.is_err());proptest::prop_assert_eq!(db.committed_wal().unwrap(),before);}
                    std::collections::btree_map::Entry::Vacant(entry)=>{proptest::prop_assert_eq!(result.unwrap().table.id,next.to_string());entry.insert(next);next+=1;}
                }
            }else{
                let result=drop_table(&mut db,&name);
                if expected.remove(&name).is_some(){proptest::prop_assert!(result.is_ok());}else{proptest::prop_assert!(result.is_err());proptest::prop_assert_eq!(db.committed_wal().unwrap(),before);}
            }
            let actual=list(&mut db).unwrap().tables.into_iter().map(|s|(s.id,s.name)).collect::<Vec<_>>();
            let mut model=expected.iter().map(|(name,id)|(*id,name.clone())).collect::<Vec<_>>();model.sort_by_key(|(id,_)|*id);
            proptest::prop_assert_eq!(actual,model.into_iter().map(|(id,name)|(id.to_string(),name)).collect::<Vec<_>>());
        }
    }
}
