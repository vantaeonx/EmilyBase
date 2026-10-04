use emilybase_catalog::{Column, DataType, Key, Row, Schema, Value};
use emilybase_transactions::Database;

fn schema() -> Schema {
    Schema {
        name: "t".into(),
        columns: vec![
            Column {
                name: "n".into(),
                data_type: DataType::Integer,
                nullable: true,
            },
            Column {
                name: "id".into(),
                data_type: DataType::Text,
                nullable: false,
            },
        ],
        primary_key: 1,
    }
}
fn row(key: &str, n: i64) -> Row {
    vec![Value::Integer(n), Value::Text(key.into())]
}
fn collect(database: &Database) -> Vec<Row> {
    database
        .view()
        .unwrap()
        .primary_rows("t", None, None)
        .unwrap()
        .rev()
        .map(|r| r.unwrap().clone())
        .collect()
}

#[test]
fn borrowed_reads_preserve_commits_rollback_both_wal_versions_and_verified_restore() {
    for version in [1, 2] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db");
        let mut database = Database::create(&path).unwrap();
        if version == 2 {
            database.compact().unwrap();
        }
        let long = format!("a{}", "x".repeat(3071));
        let mut transaction = database.begin().unwrap();
        transaction.create_table(schema()).unwrap();
        for key in ["", "a", &long, "b", "界"] {
            transaction.insert("t", row(key, 1)).unwrap();
        }
        transaction.commit().unwrap();
        let old = database.view().unwrap().clone();
        let old_rows = collect(&database);
        let before = database.committed_wal().unwrap();
        assert_eq!(collect(&database), old_rows);
        assert_eq!(database.committed_wal().unwrap(), before);
        let mut transaction = database.begin().unwrap();
        transaction
            .update("t", &Key::Text(long.clone()), row(&long, 7))
            .unwrap();
        transaction.delete("t", &Key::Text("b".into())).unwrap();
        let staged = transaction
            .view()
            .unwrap()
            .primary_rows("t", None, None)
            .unwrap()
            .map(|r| r.unwrap().clone())
            .collect::<Vec<_>>();
        assert!(staged.contains(&row(&long, 7)));
        assert!(!staged.contains(&row("b", 1)));
        transaction.rollback();
        assert_eq!(collect(&database), old_rows);
        assert_eq!(database.committed_wal().unwrap(), before);
        let mut transaction = database.begin().unwrap();
        transaction
            .update("t", &Key::Text("a".into()), row("a", 8))
            .unwrap();
        assert!(transaction.insert("t", row("a", 9)).is_err());
        assert!(transaction.view().is_err());
        assert!(transaction.commit().is_err());
        assert_eq!(collect(&database), old_rows);
        assert_eq!(database.committed_wal().unwrap(), before);
        let mut transaction = database.begin().unwrap();
        transaction
            .update("t", &Key::Text(long.clone()), row(&long, 9))
            .unwrap();
        transaction.delete("t", &Key::Text("b".into())).unwrap();
        transaction.commit().unwrap();
        let current = collect(&database);
        assert_ne!(current, old_rows);
        let old_again = old
            .primary_rows("t", None, None)
            .unwrap()
            .rev()
            .map(|r| r.unwrap().clone())
            .collect::<Vec<_>>();
        assert_eq!(old_again, old_rows);
        let confirmed = database.last_transaction();
        let wal = database.committed_wal().unwrap();
        database.save_primary_index_cache("t").unwrap();
        database.checkpoint().unwrap();
        let backup = temp.path().join("synthetic.emilybak");
        let report = emilybase_backup::create(&mut database, &backup).unwrap();
        assert_eq!(report.wal_version, version);
        assert_eq!(report.last_transaction, confirmed);
        assert_eq!(database.committed_wal().unwrap(), wal);
        drop(database);
        database = Database::open(&path).unwrap();
        assert_eq!(database.primary_cache_startup().unwrap().loaded, 1);
        assert_eq!(collect(&database), current);
        let restored_path = temp.path().join("restored");
        emilybase_backup::restore(&backup, &restored_path).unwrap();
        let mut restored = Database::open(&restored_path).unwrap();
        assert_eq!(collect(&restored), current);
        assert_eq!(restored.last_transaction(), confirmed);
        assert_eq!(restored.primary_cache_startup().unwrap().loaded, 0);
        restored.compact().unwrap();
        assert_eq!(collect(&restored), current);
        let mut transaction = restored.begin().unwrap();
        transaction.insert("t", row("new", 11)).unwrap();
        transaction.commit().unwrap();
        assert!(collect(&restored).contains(&row("new", 11)));
        assert_eq!(collect(&database), current);
    }
}
