use emilybase_query::execute;
use emilybase_transactions::Database;

#[test]
fn repeated_primary_key_writes_do_not_scan_the_entire_large_table() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::create(dir.path().join("db")).unwrap();
    execute(&mut db, "CREATE TABLE t(id INT PRIMARY KEY,value INT)", &[]).unwrap();
    for start in (0..6000).step_by(200) {
        let values = (start..start + 200)
            .map(|i| format!("({i},0)"))
            .collect::<Vec<_>>()
            .join(",");
        execute(&mut db, &format!("INSERT INTO t VALUES {values}"), &[]).unwrap();
    }
    let script = "UPDATE t SET value=1 WHERE id=0;".repeat(63) + "DELETE FROM t WHERE id=5999";
    let result = execute(&mut db, &script, &[]);
    assert!(
        result.is_ok(),
        "a bounded key lookup must not spend work on unrelated rows: {result:?}"
    );
    assert_eq!(db.view().unwrap().row_count(), 5999);
}
