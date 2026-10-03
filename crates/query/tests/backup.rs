use emilybase_catalog::Value;
use emilybase_query::{execute, query};
use emilybase_transactions::Database;

#[test]
fn sql_data_in_both_wal_versions_survives_verified_backup_and_new_commits() {
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let restored = dir.path().join("restored");
        let archive = dir.path().join("synthetic.backup");
        let mut db = Database::create(&source).unwrap();
        let parameters = [
            Value::Text("κλειδί-🌌".into()),
            Value::Text("'; DROP TABLE t --".into()),
            Value::Bytes(vec![0, 255, 42]),
        ];
        execute(&mut db, "CREATE TABLE t(id TEXT PRIMARY KEY,text TEXT,b BOOLEAN,f FLOAT,data BYTES); INSERT INTO t VALUES($1,$2,TRUE,-0.0,$3); INSERT INTO t VALUES('nulls',NULL,NULL,NULL,NULL)", &parameters).unwrap();
        execute(&mut db, "BEGIN;DELETE FROM t;ROLLBACK", &[]).unwrap();
        if compact {
            db.compact().unwrap();
        }
        let before = query(db.view().unwrap(), "SELECT * FROM t ORDER BY id", &[]).unwrap();
        let transaction = db.last_transaction();
        let report = emilybase_backup::create(&mut db, &archive).unwrap();
        assert_eq!(report.last_transaction, transaction);
        emilybase_backup::inspect(&archive).unwrap();
        emilybase_backup::restore(&archive, &restored).unwrap();
        let mut copy = Database::open(&restored).unwrap();
        assert_eq!(copy.last_transaction(), transaction);
        assert_eq!(
            query(copy.view().unwrap(), "SELECT * FROM t ORDER BY id", &[]).unwrap(),
            before
        );
        execute(
            &mut copy,
            "UPDATE t SET text='after restore' WHERE id=$1",
            &parameters[..1],
        )
        .unwrap();
        assert_eq!(copy.last_transaction(), transaction + 1);
        assert_eq!(
            query(db.view().unwrap(), "SELECT * FROM t ORDER BY id", &[]).unwrap(),
            before
        );
        drop(copy);
        let copy = Database::open(&restored).unwrap();
        assert_eq!(
            query(
                copy.view().unwrap(),
                "SELECT text FROM t WHERE id=$1",
                &parameters[..1]
            )
            .unwrap()
            .rows,
            [vec![Value::Text("after restore".into())]]
        );
    }
}
