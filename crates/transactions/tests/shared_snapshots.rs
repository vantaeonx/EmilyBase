use emilybase_catalog::{Column, DataType, Key, Row, Schema, Value};
use emilybase_database::Snapshot;
use emilybase_transactions::{Database, recover_image};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn schema(name: &str) -> Schema {
    Schema {
        name: name.into(),
        primary_key: 0,
        columns: vec![
            Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
            Column {
                name: "value".into(),
                data_type: DataType::Text,
                nullable: false,
            },
        ],
    }
}

fn row(key: i64, value: &str) -> Row {
    vec![Value::Integer(key), Value::Text(value.into())]
}

fn create(path: &std::path::Path) -> Database {
    let mut db = Database::create(path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(schema("changed")).unwrap();
    tx.create_table(schema("untouched")).unwrap();
    tx.insert("untouched", row(0, &"u".repeat(768))).unwrap();
    tx.insert("changed", row(0, "initial")).unwrap();
    tx.commit().unwrap();
    db
}

fn shared(a: &Snapshot, b: &Snapshot, name: &str) -> bool {
    match (
        a.get(name, &Key::Integer(0)).unwrap(),
        b.get(name, &Key::Integer(0)).unwrap(),
    ) {
        (Some(a), Some(b)) => std::ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

#[test]
fn begin_noop_rollback_and_abort_preserve_shared_committed_tables_and_exact_wal() {
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut db = create(&dir.path().join("db"));
        if compact {
            db.compact().unwrap();
        }
        let history = db.view().unwrap().clone();
        let before = db.committed_wal().unwrap();
        let transaction = db.last_transaction();
        let old = history
            .row_location("changed", &Key::Integer(0))
            .unwrap()
            .unwrap();
        let tx = db.begin().unwrap();
        assert!(shared(&history, tx.view().unwrap(), "changed"));
        assert!(shared(&history, tx.view().unwrap(), "untouched"));
        assert_eq!(tx.commit().unwrap(), transaction);
        assert_eq!(db.committed_wal().unwrap(), before);
        let mut tx = db.begin().unwrap();
        tx.update("changed", &Key::Integer(0), row(0, "rollback"))
            .unwrap();
        assert!(!shared(&history, tx.view().unwrap(), "changed"));
        assert!(shared(&history, tx.view().unwrap(), "untouched"));
        tx.rollback();
        assert!(shared(&history, db.view().unwrap(), "changed"));
        assert_eq!(db.committed_wal().unwrap(), before);
        let mut tx = db.begin().unwrap();
        tx.update("changed", &Key::Integer(0), row(0, "abort earlier write"))
            .unwrap();
        assert!(tx.delete("changed", &Key::Integer(99)).is_err());
        assert!(tx.commit().is_err());
        assert_eq!(db.committed_wal().unwrap(), before);
        assert_eq!(db.last_transaction(), transaction);
        assert!(shared(&history, db.view().unwrap(), "changed"));
        assert_eq!(
            db.view()
                .unwrap()
                .row_location("changed", &Key::Integer(0))
                .unwrap(),
            Some(old)
        );
    }
}

#[test]
fn committed_detachment_keeps_old_readers_through_reopen_checkpoint_and_compaction() {
    for initial_compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut db = create(&path);
        let id = db.database_id();
        if initial_compact {
            db.compact().unwrap();
        }
        let old = db.view().unwrap().clone();
        let old_digest = old.page_fingerprint();
        let old_location = old
            .row_location("changed", &Key::Integer(0))
            .unwrap()
            .unwrap();
        let mut tx = db.begin().unwrap();
        tx.update("changed", &Key::Integer(0), row(0, "committed"))
            .unwrap();
        let staged_location = tx
            .view()
            .unwrap()
            .row_location("changed", &Key::Integer(0))
            .unwrap()
            .unwrap();
        let acknowledged = tx.commit().unwrap();
        assert!(shared(&old, db.view().unwrap(), "untouched"));
        assert!(!shared(&old, db.view().unwrap(), "changed"));
        assert_eq!(old.page_fingerprint(), old_digest);
        db.checkpoint().unwrap();
        let before = db.committed_wal().unwrap();
        let recovered = recover_image(&before, Some(id)).unwrap();
        assert_eq!(recovered.last_transaction, acknowledged);
        assert_eq!(recovered.wal_version, if initial_compact { 2 } else { 1 });
        assert_eq!(
            recovered
                .snapshot
                .row_location("changed", &Key::Integer(0))
                .unwrap(),
            Some(staged_location)
        );
        db.compact().unwrap();
        assert_eq!(
            db.view().unwrap().page_fingerprint(),
            recovered.snapshot.page_fingerprint()
        );
        drop(db);
        let reopened = Database::open_bound(&path, Some(id)).unwrap();
        assert_eq!(reopened.last_transaction(), acknowledged);
        assert_eq!(
            reopened
                .view()
                .unwrap()
                .get("changed", &Key::Integer(0))
                .unwrap(),
            Some(&row(0, "committed"))
        );
        assert_eq!(
            reopened
                .view()
                .unwrap()
                .row_location("changed", &Key::Integer(0))
                .unwrap(),
            Some(staged_location)
        );
        assert_eq!(
            old.resolve_row_location("changed", &Key::Integer(0), old_location)
                .unwrap(),
            &row(0, "initial")
        );
        assert_eq!(old.page_fingerprint(), old_digest);
    }
}

#[test]
fn empty_table_after_committed_delete_stays_empty_after_rolled_back_reinsertion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = create(&path);
    let old = db.view().unwrap().clone();
    let mut tx = db.begin().unwrap();
    tx.delete("changed", &Key::Integer(0)).unwrap();
    tx.commit().unwrap();
    let bytes = db.committed_wal().unwrap();
    let empty = db.view().unwrap().clone();
    let mut tx = db.begin().unwrap();
    tx.insert("changed", row(0, "rollback")).unwrap();
    tx.rollback();
    assert!(shared(&empty, db.view().unwrap(), "changed"));
    assert_eq!(db.committed_wal().unwrap(), bytes);
    assert_eq!(empty.row_count(), 1);
    assert_eq!(old.row_count(), 2);
    assert_eq!(
        old.get("changed", &Key::Integer(0)).unwrap(),
        Some(&row(0, "initial"))
    );
    drop(db);
    let db = Database::open(&path).unwrap();
    assert!(
        db.view()
            .unwrap()
            .get("changed", &Key::Integer(0))
            .unwrap()
            .is_none()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn shared_transactions_match_independent_committed_rows_after_each_restart(
        operations in prop::collection::vec((0u8..4,0i64..8,any::<bool>()),1..30), compact in any::<bool>()
    ) {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("db");
        let mut db=create(&path);let id=db.database_id();
        if compact {db.compact().unwrap();}
        let mut expected=BTreeMap::from([(0,row(0,"initial"))]);
        for (operation,key,commit) in operations {
            let old=db.view().unwrap().clone();
            let old_rows=old.scan("changed",8).unwrap();
            let before=db.committed_wal().unwrap();
            let mut tx=db.begin().unwrap();
            prop_assert!(shared(&old,tx.view().unwrap(),"changed"));
            let exists=expected.contains_key(&key);
            let value=row(key,&format!("{operation}-{key}"));
            let valid=match operation {0=>!exists,1|2=>exists,_=>true};
            let result=match operation {
                0=>tx.insert("changed",value.clone()).map(|_|()),
                1=>tx.update("changed",&Key::Integer(key),value.clone()),
                2=>tx.delete("changed",&Key::Integer(key)),
                _=>Ok(()),
            };
            prop_assert_eq!(result.is_ok(),valid);
            if commit && valid {
                tx.commit().unwrap();
                if operation==2 {expected.remove(&key);} else if operation!=3 {expected.insert(key,value);}
            } else if valid {tx.rollback();} else {prop_assert!(tx.commit().is_err());}
            prop_assert!(shared(&old,db.view().unwrap(),"untouched"));
            prop_assert_eq!(old.scan("changed",8).unwrap(),old_rows);
            if !commit || !valid || operation==3 {prop_assert_eq!(db.committed_wal().unwrap(),before);}
            let rows=expected.values().cloned().collect::<Vec<_>>();
            prop_assert_eq!(db.view().unwrap().scan("changed",8).unwrap(),rows.clone());
            let bytes=db.committed_wal().unwrap();
            let recovered=recover_image(&bytes,Some(id)).unwrap();
            prop_assert_eq!(recovered.snapshot.scan("changed",8).unwrap(),rows.clone());
            drop(db);db=Database::open_bound(&path,Some(id)).unwrap();
            prop_assert_eq!(db.view().unwrap().scan("changed",8).unwrap(),rows);
        }
    }
}
