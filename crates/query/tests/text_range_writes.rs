use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_query::{execute, query};
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::collections::BTreeMap;

fn initialized(path: &std::path::Path) -> Database {
    let mut database = Database::create(path).unwrap();
    let mut transaction = database.begin().unwrap();
    transaction
        .create_table(Schema {
            name: "t".into(),
            columns: vec![
                Column {
                    name: "id".into(),
                    data_type: DataType::Text,
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
fn sixty_four_narrow_text_range_writes_preserve_the_real_query_work_budget() {
    let dir = tempfile::tempdir().unwrap();
    let mut database = initialized(&dir.path().join("db"));
    for start in (0..6000).step_by(200) {
        let mut transaction = database.begin().unwrap();
        for key in start..start + 200 {
            transaction
                .insert(
                    "t",
                    vec![Value::Text(format!("{key:05}")), Value::Integer(0)],
                )
                .unwrap();
        }
        transaction.commit().unwrap();
    }
    let script = "UPDATE t SET n=1 WHERE id>='00000' AND id<='00000';".repeat(63)
        + "DELETE FROM t WHERE id>='05999' AND id<'06000'";
    let report = execute(&mut database, &script, &[]).unwrap();
    assert_eq!(report.results.len(), 64);
    assert!(report.results.iter().all(|result| result.affected == 1));
    assert_eq!(database.view().unwrap().row_count(), 5999);
    assert_eq!(
        query(
            database.view().unwrap(),
            "SELECT n FROM t WHERE id>='00000' AND id<'00002' ORDER BY id",
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
    fn generated_text_range_mutations_rollback_and_error_match_an_independent_map(
        operations in proptest::collection::vec((0u8..4,0usize..8,0usize..8,any::<i64>()),0..32)
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut database = initialized(&path);
        let keys = [String::new(), "\0".into(), "a".into(), "a\0".into(), "b".into(), "界".into(), "😀".into(), format!("a{}", "x".repeat(3071))];
        let mut model = keys.iter().enumerate().map(|(i,key)|(key.clone(),if i % 3 == 0 {None} else {Some(i as i64)})).collect::<BTreeMap<_,_>>();
        let mut transaction = database.begin().unwrap();
        for (key,value) in &model { transaction.insert("t",vec![Value::Text(key.clone()),value.map_or(Value::Null,Value::Integer)]).unwrap(); }
        transaction.commit().unwrap();
        database.save_primary_index_cache("t").unwrap();
        for (action,lo,hi,value) in operations {
            let lower = &keys[lo]; let upper = &keys[hi];
            let matching = model.keys().filter(|key| *key >= lower && *key < upper).cloned().collect::<Vec<_>>();
            let parameters = [Value::Text(lower.clone()),Value::Text(upper.clone()),Value::Integer(value)];
            let update = "UPDATE t SET n=$3 WHERE id>=$1 AND id<$2";
            let wal = database.committed_wal().unwrap();
            match action {
                0 => {
                    let report = execute(&mut database,update,&parameters).unwrap();
                    prop_assert_eq!(report.results[0].affected,matching.len());
                    for key in matching { model.insert(key,Some(value)); }
                }
                1 => {
                    let report = execute(&mut database,"DELETE FROM t WHERE id>=$1 AND id<$2",&parameters).unwrap();
                    prop_assert_eq!(report.results[0].affected,matching.len());
                    for key in matching { model.remove(&key); }
                }
                2 => { execute(&mut database,&format!("BEGIN; {update}; ROLLBACK"),&parameters).unwrap();prop_assert_eq!(database.committed_wal().unwrap(),wal); }
                _ => { let script = format!("{update}; INSERT INTO t VALUES (NULL,0)"); prop_assert!(execute(&mut database,&script,&parameters).is_err());prop_assert_eq!(database.committed_wal().unwrap(),wal); }
            }
            drop(database); database = Database::open(&path).unwrap();
            let expected = model.iter().map(|(key,value)|vec![Value::Text(key.clone()),value.map_or(Value::Null,Value::Integer)]).collect::<Vec<_>>();
            prop_assert_eq!(query(database.view().unwrap(),"SELECT id,n FROM t ORDER BY id",&[]).unwrap().rows,expected);
        }
    }
}
