#![no_main]
#![forbid(unsafe_code)]
use emilybase_catalog::{Key, Value};
use emilybase_query::{ExecutionError, execute};
use emilybase_transactions::Database;
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;

fn key(number: i64, text: bool) -> Key {
    if !text {
        return Key::Integer(number);
    }
    Key::Text(match number {
        0 => format!("a{}", "x".repeat(3071)),
        1 => String::new(),
        2 => "\0".into(),
        3 => "界".into(),
        _ => format!("{number:05}"),
    })
}

fuzz_target!(|input: &[u8]| {
    let [mode, action, a, b, c, d, count, ..] = input else {
        return;
    };
    if input.len() > 4096 {
        return;
    }
    let text = mode & 1 != 0;
    let compact = mode & 2 != 0;
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("synthetic");
    let mut database = Database::create(&path).unwrap();
    if compact {
        database.compact().unwrap();
    }
    execute(
        &mut database,
        &format!(
            "CREATE TABLE t(id {} PRIMARY KEY,n INT)",
            if text { "TEXT" } else { "INT" }
        ),
        &[],
    )
    .unwrap();
    let count = 258 + i64::from(*count % 48);
    let mut model = BTreeMap::new();
    for start in (0..count).step_by(150) {
        let mut tx = database.begin().unwrap();
        for number in start..(start + 150).min(count) {
            let key = key(number, text);
            let n = if number % 7 == 0 {
                None
            } else {
                Some(number % 11)
            };
            tx.insert(
                "t",
                vec![key.to_value(), n.map_or(Value::Null, Value::Integer)],
            )
            .unwrap();
            model.insert(key, n);
        }
        tx.commit().unwrap();
    }
    let before = database.committed_wal().unwrap();
    let transaction = database.last_transaction();
    let first = model.keys().next().unwrap().clone();
    let lower = key(i64::from(u16::from_le_bytes([*a, *b]) % 330) - 20, text);
    let upper = key(i64::from(u16::from_le_bytes([*c, *d]) % 330) - 20, text);
    let value = i64::from(*b % 14) - 3;
    let mut staged = model.clone();
    *staged.get_mut(&first).unwrap() = Some(value);
    let filter = (mode >> 2) % 4;
    let selected = staged
        .iter()
        .filter(|(key, n)| {
            let interval = **key >= lower && **key < upper;
            match filter {
                0 | 1 => interval && n.is_none_or(|n| n >= value),
                2 => interval || n.is_none(),
                _ => true,
            }
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    let predicate = match filter {
        0 => "id>=$1 AND id<$2 AND (n IS NULL OR n>=$3)",
        1 => "NOT(id<$1 OR id>=$2) AND (n IS NULL OR n>=$3)",
        2 => "(id>=$1 AND id<$2) OR n IS NULL",
        _ => "TRUE",
    };
    let action = action % 4;
    let mutation = if action == 1 {
        format!("DELETE FROM t WHERE {predicate}")
    } else {
        format!("UPDATE t SET n=$3 WHERE {predicate}")
    };
    let body = format!("UPDATE t SET n=$3 WHERE id=$4; {mutation}; SELECT id,n FROM t ORDER BY id");
    let sql = match action {
        2 => format!("BEGIN;{body};ROLLBACK"),
        3 => format!("{body};INSERT INTO t VALUES(NULL,0)"),
        _ => body,
    };
    let parameters = [
        lower.to_value(),
        upper.to_value(),
        Value::Integer(value),
        first.to_value(),
    ];
    let result = execute(&mut database, &sql, &parameters);
    let overflow = selected.len() > 255;
    if overflow {
        assert!(matches!(
            result,
            Err(ExecutionError::Transaction(
                emilybase_transactions::Error::Limit
            ))
        ));
    } else if action == 3 {
        assert!(result.is_err());
    } else {
        let report = result.unwrap();
        assert_eq!(report.results[1].affected, selected.len());
        for key in selected {
            if action == 1 {
                staged.remove(&key);
            } else {
                *staged.get_mut(&key).unwrap() = Some(value);
            }
        }
        let rows = staged
            .iter()
            .map(|(key, n)| vec![key.to_value(), n.map_or(Value::Null, Value::Integer)])
            .collect::<Vec<_>>();
        assert_eq!(report.results[2].rows, rows);
        if action != 2 {
            model = staged;
        }
    }
    if overflow || action >= 2 {
        assert_eq!(database.committed_wal().unwrap(), before);
        assert_eq!(database.last_transaction(), transaction);
    } else {
        assert_eq!(database.last_transaction(), transaction + 1);
    }
    let expected = model
        .iter()
        .map(|(key, n)| vec![key.to_value(), n.map_or(Value::Null, Value::Integer)])
        .collect::<Vec<_>>();
    assert_eq!(database.view().unwrap().scan("t", 1000).unwrap(), expected);
    // Arbitrary bounded SQL additionally checks whole-script error atomicity.
    if let Ok(sql) = std::str::from_utf8(input) {
        let before = database.committed_wal().unwrap();
        let digest = database.view().unwrap().page_fingerprint();
        let transaction = database.last_transaction();
        if execute(&mut database, sql, &parameters).is_err() {
            assert_eq!(database.committed_wal().unwrap(), before);
            assert_eq!(database.view().unwrap().page_fingerprint(), digest);
            assert_eq!(database.last_transaction(), transaction);
        }
    }
    let wal = database.committed_wal().unwrap();
    let digest = database.view().unwrap().page_fingerprint();
    drop(database);
    let mut database = Database::open(&path).unwrap();
    assert_eq!(database.committed_wal().unwrap(), wal);
    assert_eq!(database.view().unwrap().page_fingerprint(), digest);
});
