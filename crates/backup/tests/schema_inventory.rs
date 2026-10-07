use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_transactions::{Database, Error};

fn wide(name: &str) -> Schema {
    Schema {
        name: name.into(),
        columns: (0..64)
            .map(|i| Column {
                name: format!("c{i:02}_{}", "x".repeat(50)),
                data_type: DataType::Integer,
                nullable: false,
            })
            .collect(),
        primary_key: 63,
    }
}
fn names(database: &Database) -> Vec<String> {
    database
        .view()
        .unwrap()
        .schema_refs()
        .map(|s| s.name.clone())
        .collect()
}

#[test]
fn both_wal_versions_keep_borrowed_metadata_exact_through_failure_reopen_and_restore() {
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        let mut database = Database::create(&path).unwrap();
        let mut transaction = database.begin().unwrap();
        for name in ["z", "a", "b"] {
            transaction.create_table(wide(name)).unwrap();
        }
        transaction.commit().unwrap();
        if compacted {
            database.compact().unwrap();
        }
        let old = database.view().unwrap().clone();
        let mut transaction = database.begin().unwrap();
        transaction.drop_table("a").unwrap();
        transaction.create_table(wide("a")).unwrap();
        transaction
            .insert("a", vec![Value::Integer(7); 64])
            .unwrap();
        transaction.commit().unwrap();
        assert_eq!(names(&database), ["z", "b", "a"]);
        assert_eq!(database.view().unwrap().table_id("a").unwrap(), 4);
        assert_eq!(
            old.schema_refs()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["z", "a", "b"]
        );
        assert_eq!(old.table_id("a").unwrap(), 2);
        assert_eq!(old.row_count(), 0);
        let wal = database.committed_wal().unwrap();
        let before = database.view().unwrap().page_fingerprint();
        let mut transaction = database.begin().unwrap();
        transaction.drop_table("b").unwrap();
        transaction.create_table(wide("discarded")).unwrap();
        transaction.rollback();
        let mut transaction = database.begin().unwrap();
        transaction.drop_table("z").unwrap();
        assert!(transaction.create_table(wide("a")).is_err());
        assert!(matches!(transaction.commit(), Err(Error::Aborted)));
        assert_eq!(database.committed_wal().unwrap(), wal);
        assert_eq!(database.view().unwrap().page_fingerprint(), before);
        let archive = dir.path().join("synthetic.backup");
        assert_eq!(
            emilybase_backup::create(&mut database, &archive)
                .unwrap()
                .wal_version,
            if compacted { 2 } else { 1 }
        );
        let target = dir.path().join("restored");
        emilybase_backup::restore(&archive, &target).unwrap();
        let restored = Database::open(&target).unwrap();
        assert_eq!(names(&restored), ["z", "b", "a"]);
        assert_eq!(
            restored.view().unwrap().schemas(),
            database.view().unwrap().schemas()
        );
        assert_eq!(restored.view().unwrap().page_fingerprint(), before);
        assert_eq!(restored.view().unwrap().table_count(), 3);
        assert_eq!(restored.view().unwrap().row_count(), 1);
        drop(database);
        let mut reopened = Database::open(&path).unwrap();
        assert_eq!(names(&reopened), names(&restored));
        assert_eq!(reopened.committed_wal().unwrap(), wal);
    }
}
