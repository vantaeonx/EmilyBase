use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::Database;
use std::fs;

#[test]
fn both_version_restore_skips_missing_caches_and_can_adopt_an_exact_source_clone() {
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        let mut source = Database::create(&path).unwrap();
        let mut transaction = source.begin().unwrap();
        transaction
            .create_table(Schema {
                name: "t".into(),
                columns: vec![Column {
                    name: "id".into(),
                    data_type: DataType::Integer,
                    nullable: false,
                }],
                primary_key: 0,
            })
            .unwrap();
        transaction.insert("t", vec![Value::Integer(7)]).unwrap();
        transaction.commit().unwrap();
        source.save_primary_index_cache("t").unwrap();
        let image = fs::read(path.join("primary-1.table-index")).unwrap();
        if compacted {
            source.compact().unwrap();
        }
        let wal = source.committed_wal().unwrap();
        let archive = dir.path().join("synthetic.backup");
        let report = emilybase_backup::create(&mut source, &archive).unwrap();
        assert_eq!(report.wal_version, if compacted { 2 } else { 1 });
        let target = dir.path().join("copy");
        emilybase_backup::restore(&archive, &target).unwrap();
        let restored = Database::open(&target).unwrap();
        let startup = restored.primary_cache_startup().unwrap();
        assert_eq!(
            (startup.loaded, startup.missing, startup.rejected),
            (0, 1, 0)
        );
        assert_eq!(
            restored.view().unwrap().get("t", &Key::Integer(7)).unwrap(),
            Some(&vec![Value::Integer(7)])
        );
        drop(restored);
        fs::copy(
            path.join("primary-1.table-index"),
            target.join("primary-1.table-index"),
        )
        .unwrap();
        let mut restored = Database::open(&target).unwrap();
        assert_eq!(restored.primary_cache_startup().unwrap().loaded, 1);
        assert_eq!(restored.primary_index_image("t").unwrap(), image);
        let mut transaction = restored.begin().unwrap();
        transaction.insert("t", vec![Value::Integer(8)]).unwrap();
        transaction.commit().unwrap();
        drop(restored);
        let restored = Database::open(&target).unwrap();
        let startup = restored.primary_cache_startup().unwrap();
        assert_eq!((startup.loaded, startup.rejected), (0, 1));
        assert_eq!(
            restored.view().unwrap().get("t", &Key::Integer(8)).unwrap(),
            Some(&vec![Value::Integer(8)])
        );
        assert_eq!(
            fs::read(target.join("primary-1.table-index")).unwrap(),
            image
        );
        assert_eq!(source.committed_wal().unwrap(), wal);
        drop(source);
        let source = Database::open(&path).unwrap();
        assert_eq!(source.primary_cache_startup().unwrap().loaded, 1);
        assert!(
            source
                .view()
                .unwrap()
                .get("t", &Key::Integer(8))
                .unwrap()
                .is_none()
        );
    }
}
