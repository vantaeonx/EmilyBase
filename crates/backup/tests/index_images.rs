use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::Database;
use std::fs;

#[test]
fn verified_restore_of_both_wal_versions_accepts_the_source_bound_tree() {
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
        for id in 0..100 {
            transaction.insert("t", vec![Value::Integer(id)]).unwrap();
        }
        transaction.commit().unwrap();
        let image = source.primary_index_image("t").unwrap();
        if compacted {
            source.compact().unwrap();
        }
        let wal = fs::read(path.join("redo.wal")).unwrap();
        let archive = dir.path().join("synthetic.backup");
        let report = emilybase_backup::create(&mut source, &archive).unwrap();
        assert_eq!(report.wal_version, if compacted { 2 } else { 1 });
        let target = dir.path().join("copy");
        assert_eq!(
            emilybase_backup::restore(&archive, &target).unwrap(),
            report
        );
        let mut restored = Database::open(&target).unwrap();
        assert_eq!(restored.database_id(), source.database_id());
        assert_eq!(restored.last_transaction(), source.last_transaction());
        assert_eq!(
            restored.view().unwrap().page_fingerprint(),
            source.view().unwrap().page_fingerprint()
        );
        let before = restored.committed_wal().unwrap();
        assert_eq!(
            restored
                .load_primary_index_image("t", &image)
                .unwrap()
                .entries,
            100
        );
        assert_eq!(restored.primary_index_image("t").unwrap(), image);
        assert_eq!(restored.committed_wal().unwrap(), before);
        let mut transaction = restored.begin().unwrap();
        transaction.delete("t", &Key::Integer(9)).unwrap();
        transaction.commit().unwrap();
        assert!(restored.load_primary_index_image("t", &image).is_err());
        assert!(source.verify_primary_index_image("t", &image).is_ok());
        assert!(
            source
                .view()
                .unwrap()
                .get("t", &Key::Integer(9))
                .unwrap()
                .is_some()
        );
        assert!(
            restored
                .view()
                .unwrap()
                .get("t", &Key::Integer(9))
                .unwrap()
                .is_none()
        );
        let current = restored.primary_index_image("t").unwrap();
        restored.compact().unwrap();
        drop(restored);
        let mut restored = Database::open(target).unwrap();
        restored.load_primary_index_image("t", &current).unwrap();
        assert_eq!(
            restored
                .view()
                .unwrap()
                .scan_integer_range("t", Some(8), Some(11), 100)
                .unwrap(),
            vec![vec![Value::Integer(8)], vec![Value::Integer(10)]]
        );
        assert_eq!(fs::read(path.join("redo.wal")).unwrap(), wal);
    }
}
