use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::Database;
use proptest::prelude::*;

fn column(name: &str, data_type: DataType, nullable: bool) -> Column {
    Column {
        name: name.into(),
        data_type,
        nullable,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn repeated_baselines_are_canonical_and_preserve_all_types_unicode_and_rollback(
        values in prop::collection::vec((".{0,12}",any::<bool>(),any::<i64>(),-2048i64..2048,
            prop::collection::vec(any::<u8>(),0..32),prop::option::of(".{0,12}")),0..12)
    ) {
        let dir=tempfile::tempdir().unwrap();
        let path=dir.path().join("db");
        let mut db=Database::create(&path).unwrap();
        let mut tx=db.begin().unwrap();
        tx.create_table(Schema { name:"typed".into(),columns:vec![
            column("key",DataType::Text,false),column("flag",DataType::Boolean,false),
            column("integer",DataType::Integer,false),column("float",DataType::Float,false),
            column("bytes",DataType::Bytes,false),column("optional",DataType::Text,true)
        ],primary_key:0 }).unwrap();
        let mut expected=Vec::new();
        for (index,(text,flag,integer,float,bytes,optional)) in values.into_iter().enumerate() {
            let row=vec![Value::Text(format!("{index:02}-{text}")),Value::Boolean(flag),Value::Integer(integer),
                Value::Float(float as f64/2.0),Value::Bytes(bytes),optional.map_or(Value::Null,Value::Text)];
            tx.insert("typed",row.clone()).unwrap();
            expected.push(row);
        }
        tx.commit().unwrap();
        let id=db.database_id();
        let boundary=db.last_transaction();
        let events=db.view().unwrap().event_count();
        db.compact().unwrap();
        let canonical=db.committed_wal().unwrap();
        for _ in 0..3 {
            let mut tx=db.begin().unwrap();
            tx.insert("typed",vec![Value::Text("rolled-back".into()),Value::Boolean(false),Value::Integer(0),
                Value::Float(0.0),Value::Bytes(vec![]),Value::Null]).unwrap();
            tx.rollback();
            db.compact().unwrap();
            prop_assert_eq!(&db.committed_wal().unwrap(),&canonical);
            drop(db);
            db=Database::open_bound(&path,Some(id)).unwrap();
            prop_assert_eq!(db.last_transaction(),boundary);
            prop_assert_eq!(db.view().unwrap().event_count(),events);
            prop_assert_eq!(&db.view().unwrap().scan("typed",100).unwrap(),&expected);
            prop_assert!(db.view().unwrap().get("typed",&Key::Text("rolled-back".into())).unwrap().is_none());
        }
        let mut tx=db.begin().unwrap();
        tx.insert("typed",vec![Value::Text("new-key".into()),Value::Boolean(true),Value::Integer(1),
            Value::Float(1.5),Value::Bytes(vec![255]),Value::Text("after compaction".into())]).unwrap();
        prop_assert_eq!(tx.commit().unwrap(),boundary+1);
        db.compact().unwrap();
        drop(db);
        let db=Database::open(path).unwrap();
        prop_assert_eq!(db.view().unwrap().row_count(),expected.len()+1);
        prop_assert_eq!(db.last_transaction(),boundary+1);
    }
}
