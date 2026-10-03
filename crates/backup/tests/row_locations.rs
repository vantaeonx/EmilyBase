use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::Database;
use std::fs;

#[test]
fn verified_restore_preserves_locations_and_mutation_retires_only_the_copy_image() {
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        let mut database = Database::create(&path).unwrap();
        let mut transaction = database.begin().unwrap();
        transaction
            .create_table(Schema {
                name: "items".into(),
                columns: vec![
                    Column {
                        name: "id".into(),
                        data_type: DataType::Text,
                        nullable: false,
                    },
                    Column {
                        name: "n".into(),
                        data_type: DataType::Integer,
                        nullable: false,
                    },
                ],
                primary_key: 0,
            })
            .unwrap();
        let text = "界".repeat(1024);
        let key = Key::Text(text.clone());
        let row = vec![Value::Text(text.clone()), Value::Integer(8)];
        transaction.insert("items", row.clone()).unwrap();
        transaction.commit().unwrap();
        if compacted {
            database.compact().unwrap();
        }
        let original = database.row_location("items", &key).unwrap().unwrap();
        let wal = fs::read(path.join("redo.wal")).unwrap();
        let archive = dir.path().join("synthetic.backup");
        let report = emilybase_backup::create(&mut database, &archive).unwrap();
        assert_eq!(report.wal_version, if compacted { 2 } else { 1 });
        let target = dir.path().join("copy");
        assert_eq!(
            emilybase_backup::restore(&archive, &target).unwrap(),
            report
        );
        let mut restored = Database::open(&target).unwrap();
        assert_eq!(
            restored.row_location("items", &key).unwrap(),
            Some(original)
        );
        assert_eq!(
            restored
                .resolve_row_location("items", &key, original)
                .unwrap(),
            &row
        );
        let mut transaction = restored.begin().unwrap();
        let replacement = vec![Value::Text(text), Value::Integer(9)];
        transaction
            .update("items", &key, replacement.clone())
            .unwrap();
        let newer = transaction.row_location("items", &key).unwrap().unwrap();
        transaction.commit().unwrap();
        assert_ne!(newer, original);
        assert!(
            restored
                .resolve_row_location("items", &key, original)
                .is_err()
        );
        restored.compact().unwrap();
        drop(restored);
        let restored = Database::open(&target).unwrap();
        assert_eq!(restored.row_location("items", &key).unwrap(), Some(newer));
        assert_eq!(
            restored.resolve_row_location("items", &key, newer).unwrap(),
            &replacement
        );
        assert_eq!(
            database
                .resolve_row_location("items", &key, original)
                .unwrap(),
            &row
        );
        assert!(database.resolve_row_location("items", &key, newer).is_err());
        assert_eq!(fs::read(path.join("redo.wal")).unwrap(), wal);
    }
}
