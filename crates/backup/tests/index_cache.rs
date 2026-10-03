use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::Database;
use std::fs;

#[test]
fn verified_backup_omits_disposable_damaged_sidecars_and_restore_can_build_new_ones() {
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
        if compacted {
            source.compact().unwrap();
        }
        let active = path.join("primary-1.table-index");
        fs::write(&active, b"synthetic damaged optional image").unwrap();
        let wal = fs::read(path.join("redo.wal")).unwrap();
        let archive = dir.path().join("synthetic.backup");
        let report = emilybase_backup::create(&mut source, &archive).unwrap();
        assert_eq!(report.wal_version, if compacted { 2 } else { 1 });
        let target = dir.path().join("copy");
        emilybase_backup::restore(&archive, &target).unwrap();
        let mut restored = Database::open(&target).unwrap();
        assert_eq!(restored.load_primary_index_cache("t").unwrap(), None);
        assert!(!target.join("primary-1.table-index").exists());
        assert_eq!(
            restored.view().unwrap().get("t", &Key::Integer(7)).unwrap(),
            Some(&vec![Value::Integer(7)])
        );
        restored.save_primary_index_cache("t").unwrap();
        assert_eq!(
            restored
                .load_primary_index_cache("t")
                .unwrap()
                .unwrap()
                .entries,
            1
        );
        assert_eq!(
            fs::read(active).unwrap(),
            b"synthetic damaged optional image"
        );
        assert_eq!(fs::read(path.join("redo.wal")).unwrap(), wal);
    }
}
