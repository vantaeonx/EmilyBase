use std::collections::BTreeMap;

use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::Database;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_transaction_batches_match_an_independent_committed_model(
        batches in prop::collection::vec((
            any::<bool>(),
            prop::collection::vec((0u8..3, 0i64..12, "[a-z]{0,20}"), 0..8)
        ), 0..12)
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut db = Database::create(&path).unwrap();
        let mut tx = db.begin().unwrap();
        tx.create_table(Schema {
            name: "items".into(),
            columns: vec![
                Column { name: "id".into(), data_type: DataType::Integer, nullable: false },
                Column { name: "text".into(), data_type: DataType::Text, nullable: false },
            ],
            primary_key: 0,
        }).unwrap();
        tx.commit().unwrap();
        let mut model = BTreeMap::new();
        for (commit, operations) in batches {
            let before = std::fs::read(path.join("redo.wal")).unwrap();
            let mut staged = model.clone();
            let mut tx = db.begin().unwrap();
            let mut failed = false;
            for (kind, id, text) in operations {
                let row = vec![Value::Integer(id), Value::Text(text.clone())];
                let should_succeed = match kind {
                    0 => !staged.contains_key(&id),
                    _ => staged.contains_key(&id),
                };
                let result = match kind {
                    0 => tx.insert("items", row).map(|_| ()),
                    1 => tx.update("items", &Key::Integer(id), row),
                    _ => tx.delete("items", &Key::Integer(id)),
                };
                prop_assert_eq!(result.is_ok(), should_succeed);
                if result.is_err() {
                    failed = true;
                    break;
                }
                if kind == 2 {
                    staged.remove(&id);
                } else {
                    staged.insert(id, text);
                }
            }
            if failed {
                prop_assert!(tx.commit().is_err());
            } else if commit {
                tx.commit().unwrap();
                model = staged;
            } else {
                tx.rollback();
            }
            if failed || !commit {
                prop_assert_eq!(std::fs::read(path.join("redo.wal")).unwrap(), before);
            }
            db.checkpoint().unwrap();
            drop(db);
            db = Database::open(&path).unwrap();
            let expected = model.iter().map(|(&id, text)| {
                vec![Value::Integer(id), Value::Text(text.clone())]
            }).collect::<Vec<_>>();
            prop_assert_eq!(db.view().unwrap().scan("items", 100).unwrap(), expected);
            prop_assert_eq!(db.view().unwrap().row_count(), model.len());
        }
    }
}
