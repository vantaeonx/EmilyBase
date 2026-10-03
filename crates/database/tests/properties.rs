use std::collections::BTreeMap;

use emilybase_catalog::{Column, DataType, Key, Row, Schema, Value};
use emilybase_database::Database;
use proptest::prelude::*;

fn row(key: i64, text: &str) -> Row {
    vec![Value::Integer(key), Value::Text(text.into())]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn persistent_crud_matches_an_independent_model(
        operations in prop::collection::vec((0u8..3, -5i64..5, ".{0,12}"), 0..40),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.emily");
        let mut db = Database::create(&path).unwrap();
        db.create_table(Schema {
            name: "items".into(),
            columns: vec![
                Column { name: "id".into(), data_type: DataType::Integer, nullable: false },
                Column { name: "text".into(), data_type: DataType::Text, nullable: false },
            ], primary_key: 0,
        }).unwrap();
        let mut model: BTreeMap<i64, String> = BTreeMap::new();
        for (operation, key, text) in operations {
            let before = std::fs::read(&path).unwrap();
            let exists = model.contains_key(&key);
            let result = match operation {
                0 => db.insert("items", row(key, &text)).map(|_| ()),
                1 => db.update("items", &Key::Integer(key), row(key, &text)),
                _ => db.delete("items", &Key::Integer(key)),
            };
            let expected_success = if operation == 0 { !exists } else { exists };
            prop_assert_eq!(result.is_ok(), expected_success);
            if expected_success {
                if operation == 2 { model.remove(&key); }
                else { model.insert(key, text); }
            } else {
                prop_assert_eq!(std::fs::read(&path).unwrap(), before);
            }
            drop(db);
            db = Database::open(&path).unwrap();
            let expected: Vec<Row> = model.iter().map(|(key, text)| row(*key, text)).collect();
            prop_assert_eq!(db.scan("items", 100).unwrap(), expected);
            prop_assert_eq!(db.row_count(), model.len());
        }
    }
}
