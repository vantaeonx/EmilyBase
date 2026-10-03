use std::collections::BTreeMap;

use emilybase_backup::{create, encode, inspect_bytes, restore};
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::Database;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn arbitrary_archive_input_never_panics(bytes in prop::collection::vec(any::<u8>(),0..20000)) {
        let _=inspect_bytes(&bytes);
    }

    #[test]
    fn random_payloads_and_commit_boundaries_round_trip(
        values in prop::collection::vec(".{0,32}",0..12)
    ) {
        let dir=tempfile::tempdir().unwrap();
        let mut db=Database::create(dir.path().join("db")).unwrap();
        let mut tx=db.begin().unwrap();
        tx.create_table(Schema {
            name:"items".into(),
            columns:vec![
                Column {name:"id".into(),data_type:DataType::Integer,nullable:false},
                Column {name:"text".into(),data_type:DataType::Text,nullable:false}
            ],primary_key:0
        }).unwrap();
        tx.commit().unwrap();
        for (id,text) in values.iter().enumerate() {
            let mut tx=db.begin().unwrap();
            tx.insert("items",vec![Value::Integer(id as i64),Value::Text(text.clone())]).unwrap();
            tx.commit().unwrap();
        }
        let bytes=encode(&db.committed_wal().unwrap()).unwrap();
        let report=inspect_bytes(&bytes).unwrap();
        prop_assert_eq!(report.rows,values.len());
        prop_assert_eq!(report.last_transaction,values.len() as u64+2);
        prop_assert_eq!(report.database_id,db.database_id());
    }

    #[test]
    fn restored_crud_and_rollback_history_matches_an_independent_map(
        operations in prop::collection::vec((0u8..3,0i64..8,"[a-z]{0,20}",any::<bool>()),0..20)
    ) {
        let dir=tempfile::tempdir().unwrap();
        let mut db=Database::create(dir.path().join("source")).unwrap();
        let mut tx=db.begin().unwrap();
        tx.create_table(Schema {
            name:"items".into(),columns:vec![
                Column {name:"id".into(),data_type:DataType::Integer,nullable:false},
                Column {name:"text".into(),data_type:DataType::Text,nullable:false}
            ],primary_key:0
        }).unwrap();
        tx.commit().unwrap();
        let mut model=BTreeMap::<i64,String>::new();
        for (kind,id,text,commit) in operations {
            let exists=model.contains_key(&id);
            let should_succeed=if kind==0 { !exists } else { exists };
            let mut tx=db.begin().unwrap();
            let row=vec![Value::Integer(id),Value::Text(text.clone())];
            let result=match kind {
                0=>tx.insert("items",row).map(|_|()),
                1=>tx.update("items",&Key::Integer(id),row),
                _=>tx.delete("items",&Key::Integer(id))
            };
            prop_assert_eq!(result.is_ok(),should_succeed);
            if !should_succeed {
                prop_assert!(tx.commit().is_err());
            } else if commit {
                tx.commit().unwrap();
                if kind==2 { model.remove(&id); } else { model.insert(id,text); }
            } else {
                tx.rollback();
            }
        }
        let archive=dir.path().join("snapshot.backup");
        let target=dir.path().join("restored");
        let report=create(&mut db,&archive).unwrap();
        let before=std::fs::read(&archive).unwrap();
        prop_assert_eq!(restore(&archive,&target).unwrap(),report);
        let mut restored=Database::open(&target).unwrap();
        let expected=model.into_iter().map(|(id,text)| {
            vec![Value::Integer(id),Value::Text(text)]
        }).collect::<Vec<_>>();
        prop_assert_eq!(restored.view().unwrap().scan("items",100).unwrap(),expected);
        let original_boundary=restored.last_transaction();
        let mut tx=restored.begin().unwrap();
        tx.insert("items",vec![Value::Integer(99),Value::Text("after restore".into())]).unwrap();
        prop_assert_eq!(tx.commit().unwrap(),original_boundary+1);
        drop(restored);
        let restored=Database::open(target).unwrap();
        prop_assert!(restored.view().unwrap().get("items",&Key::Integer(99)).unwrap().is_some());
        prop_assert_eq!(std::fs::read(archive).unwrap(),before);
        prop_assert!(db.view().unwrap().get("items",&Key::Integer(99)).unwrap().is_none());
    }
}
