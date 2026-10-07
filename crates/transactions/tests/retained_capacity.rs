use emilybase_catalog::{Column, DataType, Key, Row, Schema, Value};
use emilybase_database::Snapshot;
use emilybase_transactions::{Database, recover_image};

fn text(value: &str) -> String {
    let mut s = String::with_capacity(64 * 1024);
    s.push_str(value);
    s
}
fn schema() -> Schema {
    let mut columns = Vec::with_capacity(1024);
    columns.push(Column {
        name: text("id"),
        data_type: DataType::Text,
        nullable: false,
    });
    columns.push(Column {
        name: text("value"),
        data_type: DataType::Text,
        nullable: false,
    });
    Schema {
        name: text("items"),
        columns,
        primary_key: 0,
    }
}
fn row(key: &str, value: &str) -> Row {
    let mut r = Vec::with_capacity(1024);
    r.push(Value::Text(text(key)));
    r.push(Value::Text(text(value)));
    r
}
fn check(snapshot: &Snapshot, key: &str, value: &str) {
    let schema = snapshot.schema("items").unwrap();
    assert_eq!(schema.name.capacity(), schema.name.len());
    assert_eq!(schema.columns.capacity(), schema.columns.len());
    for column in &schema.columns {
        assert_eq!(column.name.capacity(), column.name.len());
    }
    let row = snapshot
        .get("items", &Key::Text(key.into()))
        .unwrap()
        .unwrap();
    assert_eq!(row.capacity(), row.len());
    assert_eq!(row[1], Value::Text(value.into()));
    for v in row {
        if let Value::Text(s) = v {
            assert_eq!(s.capacity(), s.len());
        }
    }
}
#[test]
fn both_wal_versions_stage_commit_recover_checkpoint_and_compact_without_spare_payload() {
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut db = Database::create(&path).unwrap();
        let short = "я".repeat(128);
        let long = "ю".repeat(1536);
        let mut tx = db.begin().unwrap();
        tx.create_table(schema()).unwrap();
        tx.insert("items", row(&short, "short")).unwrap();
        tx.insert("items", row(&long, "long")).unwrap();
        check(tx.view().unwrap(), &long, "long");
        tx.commit().unwrap();
        if compact {
            db.compact().unwrap();
        }
        let old = db.view().unwrap().clone();
        let old_digest = old.page_fingerprint();
        let old_location = old.row_location("items", &Key::Text(long.clone())).unwrap();
        let mut tx = db.begin().unwrap();
        tx.update("items", &Key::Text(long.clone()), row(&long, "new"))
            .unwrap();
        check(tx.view().unwrap(), &long, "new");
        check(&old, &long, "long");
        let acknowledged = tx.commit().unwrap();
        let source = db.committed_wal().unwrap();
        let recovered = recover_image(&source, Some(db.database_id())).unwrap();
        assert_eq!(recovered.wal_version, if compact { 2 } else { 1 });
        assert_eq!(recovered.last_transaction, acknowledged);
        check(&recovered.snapshot, &long, "new");
        assert_eq!(
            recovered.snapshot.page_fingerprint(),
            db.view().unwrap().page_fingerprint()
        );
        db.checkpoint().unwrap();
        drop(db);
        let mut db = Database::open(&path).unwrap();
        check(db.view().unwrap(), &long, "new");
        assert_eq!(db.committed_wal().unwrap(), source);
        db.compact().unwrap();
        check(db.view().unwrap(), &long, "new");
        assert_eq!(old.page_fingerprint(), old_digest);
        check(&old, &long, "long");
        assert_eq!(
            old.row_location("items", &Key::Text(long.clone())).unwrap(),
            old_location
        );
        let mut tx = db.begin().unwrap();
        tx.delete("items", &Key::Text(long)).unwrap();
        tx.commit().unwrap();
        check(db.view().unwrap(), &short, "short");
        assert_eq!(db.view().unwrap().row_count(), 1);
    }
}
#[test]
fn rolled_back_and_aborted_inflated_rows_leave_exact_wal_and_original_owner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(schema()).unwrap();
    tx.insert("items", row("key", "old")).unwrap();
    tx.commit().unwrap();
    for compact in [false, true] {
        if compact {
            db.compact().unwrap();
        }
        let source = db.committed_wal().unwrap();
        let old = db.view().unwrap().clone();
        let row_pointer = old
            .get("items", &Key::Text("key".into()))
            .unwrap()
            .unwrap()
            .as_ptr();
        let mut tx = db.begin().unwrap();
        tx.update("items", &Key::Text("key".into()), row("key", "rollback"))
            .unwrap();
        check(tx.view().unwrap(), "key", "rollback");
        tx.rollback();
        assert_eq!(db.committed_wal().unwrap(), source);
        check(db.view().unwrap(), "key", "old");
        assert_eq!(
            db.view()
                .unwrap()
                .get("items", &Key::Text("key".into()))
                .unwrap()
                .unwrap()
                .as_ptr(),
            row_pointer
        );
        let mut tx = db.begin().unwrap();
        tx.update("items", &Key::Text("key".into()), row("key", "aborted"))
            .unwrap();
        assert!(tx.insert("items", row("key", "duplicate")).is_err());
        assert!(tx.commit().is_err());
        assert_eq!(db.committed_wal().unwrap(), source);
        check(&old, "key", "old");
        check(db.view().unwrap(), "key", "old");
    }
}
