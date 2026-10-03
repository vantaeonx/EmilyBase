use emilybase_backup::{encode, inspect_bytes};
use emilybase_catalog::{Column, DataType, Schema, Value};
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
}
