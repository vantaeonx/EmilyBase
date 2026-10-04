use emilybase_catalog::{Key, Value};
use emilybase_query::execute;
use emilybase_transactions::Database;

#[test]
fn borrowed_sql_selection_matches_direct_transaction_wal_bytes_in_both_versions() {
    for text in [false, true] {
        for version in [1, 2] {
            let temporary = tempfile::tempdir().unwrap();
            let mut sql = Database::create(temporary.path().join("source")).unwrap();
            if version == 2 {
                sql.compact().unwrap();
            }
            execute(
                &mut sql,
                &format!(
                    "CREATE TABLE t(id {} PRIMARY KEY,n INT)",
                    if text { "TEXT" } else { "INT" }
                ),
                &[],
            )
            .unwrap();
            let keys = if text {
                [
                    String::new(),
                    "\0".into(),
                    "a".into(),
                    format!("a{}", "x".repeat(3071)),
                    "b".into(),
                    "界".into(),
                    "😀".into(),
                ]
                .into_iter()
                .map(Key::Text)
                .collect::<Vec<_>>()
            } else {
                (-4..3).map(Key::Integer).collect::<Vec<_>>()
            };
            let mut transaction = sql.begin().unwrap();
            for key in keys.iter().rev() {
                transaction
                    .insert("t", vec![key.to_value(), Value::Integer(0)])
                    .unwrap();
            }
            transaction.commit().unwrap();
            sql.save_primary_index_cache("t").unwrap();
            let archive = temporary.path().join("synthetic.backup");
            emilybase_backup::create(&mut sql, &archive).unwrap();
            let restored = temporary.path().join("direct");
            emilybase_backup::restore(&archive, &restored).unwrap();
            let mut direct = Database::open(&restored).unwrap();
            assert_eq!(
                sql.committed_wal().unwrap(),
                direct.committed_wal().unwrap()
            );
            let lower = &keys[2];
            let upper = &keys[5];
            let report=execute(&mut sql,"UPDATE t SET n=9 WHERE id=$1; UPDATE t SET n=7 WHERE id>=$1 AND id<$2; DELETE FROM t WHERE id=$2",&[lower.to_value(),upper.to_value()]).unwrap();
            let mut transaction = direct.begin().unwrap();
            transaction
                .update("t", lower, vec![lower.to_value(), Value::Integer(9)])
                .unwrap();
            for key in keys.iter().filter(|key| *key >= lower && *key < upper) {
                transaction
                    .update("t", key, vec![key.to_value(), Value::Integer(7)])
                    .unwrap();
            }
            transaction.delete("t", upper).unwrap();
            assert_eq!(transaction.commit().unwrap(), report.transaction);
            assert_eq!(
                sql.committed_wal().unwrap(),
                direct.committed_wal().unwrap()
            );
            assert_eq!(
                sql.view().unwrap().page_fingerprint(),
                direct.view().unwrap().page_fingerprint()
            );
        }
    }
}
