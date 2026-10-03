use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::fs;

fn schema() -> Schema {
    Schema {
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
                nullable: false,
            },
        ],
        primary_key: 0,
    }
}
fn initialized(path: &std::path::Path) -> Database {
    let mut database = Database::create(path).unwrap();
    let mut transaction = database.begin().unwrap();
    transaction.create_table(schema()).unwrap();
    transaction.commit().unwrap();
    database
}

#[test]
fn abandoned_staging_is_never_adopted_and_recreated_tables_use_new_cache_names() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut database = initialized(&path);
    let orphan = path.join(".emilybase-table-index-synthetic-orphan");
    fs::write(&orphan, database.primary_index_image("t").unwrap()).unwrap();
    assert_eq!(database.load_primary_index_cache("t").unwrap(), None);
    database.save_primary_index_cache("t").unwrap();
    let original = fs::read(path.join("primary-1.table-index")).unwrap();
    let mut transaction = database.begin().unwrap();
    transaction.drop_table("t").unwrap();
    transaction.create_table(schema()).unwrap();
    transaction
        .insert("t", vec![Value::Integer(9), Value::Integer(99)])
        .unwrap();
    transaction.commit().unwrap();
    assert_eq!(database.view().unwrap().table_id("t").unwrap(), 2);
    assert_eq!(database.load_primary_index_cache("t").unwrap(), None);
    assert!(database.save_primary_index_cache("../t").is_err());
    assert!(database.load_primary_index_cache("../t").is_err());
    database.save_primary_index_cache("t").unwrap();
    assert_eq!(
        fs::read(path.join("primary-1.table-index")).unwrap(),
        original
    );
    assert!(path.join("primary-2.table-index").exists());
    assert!(orphan.exists());
    drop(database);
    let mut database = Database::open(&path).unwrap();
    assert_eq!(
        database
            .load_primary_index_cache("t")
            .unwrap()
            .unwrap()
            .entries,
        1
    );
    assert_eq!(
        database.view().unwrap().get("t", &Key::Integer(9)).unwrap(),
        Some(&vec![Value::Integer(9), Value::Integer(99)])
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn private_cache_save_load_reopen_matches_independent_committed_model(
        operations in proptest::collection::vec((0u8..5, -8i64..8, any::<i64>()), 0..32)
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut database = initialized(&path);
        let mut model = BTreeMap::new();
        database.save_primary_index_cache("t").unwrap();
        for (action, key, value) in operations {
            let old = fs::read(path.join("primary-1.table-index")).unwrap();
            let row = vec![Value::Integer(key), Value::Integer(value)];
            let mut transaction = database.begin().unwrap();
            match action {
                0 => {
                    if model.contains_key(&key) { transaction.update("t", &Key::Integer(key), row).unwrap(); }
                    else { transaction.insert("t", row).unwrap(); }
                    transaction.commit().unwrap();
                    model.insert(key, value);
                }
                1 if model.contains_key(&key) => {
                    transaction.delete("t", &Key::Integer(key)).unwrap();
                    transaction.commit().unwrap();
                    model.remove(&key);
                }
                2 => {
                    if model.contains_key(&key) { transaction.update("t", &Key::Integer(key), row).unwrap(); }
                    else { transaction.insert("t", row).unwrap(); }
                    transaction.rollback();
                }
                3 => {
                    let result = if model.contains_key(&key) { transaction.insert("t", row).map(|_| ()) }
                        else { transaction.update("t", &Key::Integer(key), row) };
                    prop_assert!(result.is_err());
                    prop_assert!(transaction.commit().is_err());
                }
                _ => { transaction.commit().unwrap(); }
            }
            let current = database.primary_index_image("t").unwrap();
            if current == old { prop_assert!(database.load_primary_index_cache("t").is_ok()); }
            else { prop_assert!(database.load_primary_index_cache("t").is_err()); }
            let wal = database.committed_wal().unwrap();
            database.save_primary_index_cache("t").unwrap();
            prop_assert_eq!(database.load_primary_index_cache("t").unwrap().unwrap().entries, model.len());
            prop_assert_eq!(database.committed_wal().unwrap(), wal);
            drop(database);
            database = Database::open(&path).unwrap();
            database.load_primary_index_cache("t").unwrap().unwrap();
            let expected = model.iter().map(|(key, value)| vec![Value::Integer(*key), Value::Integer(*value)]).collect::<Vec<_>>();
            prop_assert_eq!(database.view().unwrap().scan_integer_range("t", None, None, 100).unwrap(), expected);
        }
    }
}
