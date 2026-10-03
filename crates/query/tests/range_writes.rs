use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_query::{execute, query};
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::collections::BTreeMap;

fn database(path: &std::path::Path) -> Database {
    let mut database = Database::create(path).unwrap();
    let mut transaction = database.begin().unwrap();
    transaction
        .create_table(Schema {
            name: "t".into(),
            columns: vec![
                Column {
                    name: "id".into(),
                    data_type: DataType::Integer,
                    nullable: false,
                },
                Column {
                    name: "n".into(),
                    data_type: DataType::Integer,
                    nullable: true,
                },
            ],
            primary_key: 0,
        })
        .unwrap();
    transaction.commit().unwrap();
    database
}

#[test]
fn repeated_narrow_range_writes_do_not_spend_query_work_on_unrelated_rows() {
    let dir = tempfile::tempdir().unwrap();
    let mut database = database(&dir.path().join("db"));
    for start in (0..6000).step_by(200) {
        let mut transaction = database.begin().unwrap();
        for key in start..start + 200 {
            transaction
                .insert("t", vec![Value::Integer(key), Value::Integer(0)])
                .unwrap();
        }
        transaction.commit().unwrap();
    }
    let script = "UPDATE t SET n=1 WHERE id>=0 AND id<1;".repeat(63)
        + "DELETE FROM t WHERE id>=5999 AND id<6000";
    let result = execute(&mut database, &script, &[]).unwrap();
    assert!(result.committed);
    assert_eq!(result.results.len(), 64);
    assert!(result.results.iter().all(|result| result.affected == 1));
    assert_eq!(database.view().unwrap().row_count(), 5999);
    assert_eq!(
        query(
            database.view().unwrap(),
            "SELECT n FROM t WHERE id>=0 AND id<2 ORDER BY id",
            &[]
        )
        .unwrap()
        .rows,
        vec![vec![Value::Integer(1)], vec![Value::Integer(0)]]
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_ranged_updates_deletes_rollback_and_failures_match_independent_rows(
        operations in proptest::collection::vec((0u8..4,-8i64..32,-8i64..32,any::<i64>()),0..24)
    ) {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("db");let mut database=database(&path);
        let mut transaction=database.begin().unwrap();let mut model=BTreeMap::new();
        for key in 0..24 {
            let value=if key%3==0 {None}else{Some(key)};
            transaction.insert("t",vec![Value::Integer(key),value.map_or(Value::Null,Value::Integer)]).unwrap();model.insert(key,value);
        }
        transaction.commit().unwrap();
        for (action,lower,upper,value) in operations {
            let before=database.committed_wal().unwrap();
            let parameters=[Value::Integer(lower),Value::Integer(upper),Value::Integer(value)];
            let selected=model.keys().filter(|key|**key>=lower && **key<upper).copied().collect::<Vec<_>>();
            match action {
                0 => {
                    let report=execute(&mut database,"UPDATE t SET n=$3 WHERE id>=$1 AND id<$2",&parameters).unwrap();
                    prop_assert_eq!(report.results[0].affected,selected.len());
                    for key in selected {model.insert(key,Some(value));}
                }
                1 => {
                    let report=execute(&mut database,"DELETE FROM t WHERE id>=$1 AND id<$2",&parameters).unwrap();
                    prop_assert_eq!(report.results[0].affected,selected.len());
                    for key in selected {model.remove(&key);}
                }
                2 => {
                    let report=execute(&mut database,"BEGIN; UPDATE t SET n=$3 WHERE id>=$1 AND id<$2; ROLLBACK",&parameters).unwrap();
                    prop_assert!(!report.committed);prop_assert_eq!(database.committed_wal().unwrap(),before);
                }
                _ => {
                    prop_assert!(execute(&mut database,"UPDATE t SET n=$3 WHERE id>=$1 AND id<$2; UPDATE t SET missing=0",&parameters).is_err());
                    prop_assert_eq!(database.committed_wal().unwrap(),before);
                }
            }
            let expected=model.iter().map(|(key,value)|vec![Value::Integer(*key),value.map_or(Value::Null,Value::Integer)]).collect::<Vec<_>>();
            prop_assert_eq!(query(database.view().unwrap(),"SELECT id,n FROM t ORDER BY id",&[]).unwrap().rows,expected.clone());
            drop(database);database=Database::open(&path).unwrap();
            prop_assert_eq!(query(database.view().unwrap(),"SELECT id,n FROM t ORDER BY id",&[]).unwrap().rows,expected);
        }
    }
}
