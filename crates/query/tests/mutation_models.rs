use emilybase_catalog::{Key, Value};
use emilybase_query::{ExecutionError, execute, query};
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::collections::BTreeMap;

fn key(number: i16, text: bool) -> Key {
    if !text {
        return Key::Integer(i64::from(number));
    }
    Key::Text(match number {
        0 => format!("a{}", "x".repeat(3071)),
        1 => String::new(),
        2 => "\0".into(),
        3 => "界😀".into(),
        _ => format!("{number:05}"),
    })
}

fn rows(model: &BTreeMap<Key, Option<i64>>) -> Vec<Vec<Value>> {
    model
        .iter()
        .map(|(key, n)| vec![key.to_value(), n.map_or(Value::Null, Value::Integer)])
        .collect()
}

type Operation = (u8, i16, i16, i64, u8);

fn model_run(text: bool, compact: bool, count: i16, operations: Vec<Operation>) {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("source");
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
    let mut model = BTreeMap::new();
    for start in (0..count).step_by(150) {
        let mut transaction = database.begin().unwrap();
        for number in start..(start + 150).min(count) {
            let key = key(number, text);
            let n = if number % 7 == 0 {
                None
            } else {
                Some(i64::from(number % 11))
            };
            transaction
                .insert(
                    "t",
                    vec![key.to_value(), n.map_or(Value::Null, Value::Integer)],
                )
                .unwrap();
            model.insert(key, n);
        }
        transaction.commit().unwrap();
    }
    database.save_primary_index_cache("t").unwrap();
    for (action, lo, hi, value, filter) in operations {
        let lower = key(lo, text);
        let upper = key(hi, text);
        let before = database.committed_wal().unwrap();
        let transaction = database.last_transaction();
        let mut staged = model.clone();
        let first = staged
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| key(999, text));
        let prefix_events = if let Some(n) = staged.get_mut(&first) {
            *n = Some(value);
            1
        } else {
            0
        };
        let selected = staged
            .iter()
            .filter(|(key, n)| {
                let in_range = **key >= lower && **key < upper;
                if filter == 2 {
                    in_range || n.is_none()
                } else {
                    in_range && n.is_none_or(|n| n >= value)
                }
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        let predicate = match filter {
            0 => "id>=$1 AND id<$2 AND (n IS NULL OR n>=$3)",
            1 => "NOT (id<$1 OR id>=$2) AND (n IS NULL OR n>=$3)",
            _ => "(id>=$1 AND id<$2) OR n IS NULL",
        };
        let mutation = if action == 1 {
            format!("DELETE FROM t WHERE {predicate}")
        } else {
            format!("UPDATE t SET n=$3 WHERE {predicate}")
        };
        let body =
            format!("UPDATE t SET n=$3 WHERE id=$4; {mutation}; SELECT id,n FROM t ORDER BY id");
        let script = match action {
            2 => format!("BEGIN; {body}; ROLLBACK"),
            3 => format!("{body}; INSERT INTO t VALUES(NULL,0)"),
            _ => body,
        };
        let result = execute(
            &mut database,
            &script,
            &[
                lower.to_value(),
                upper.to_value(),
                Value::Integer(value),
                first.to_value(),
            ],
        );
        let overflow = selected.len() + prefix_events > 256;
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
            assert_eq!(report.results[0].affected, prefix_events);
            assert_eq!(report.results[1].affected, selected.len());
            for key in selected {
                if action == 1 {
                    staged.remove(&key);
                } else {
                    *staged.get_mut(&key).unwrap() = Some(value);
                }
            }
            assert_eq!(report.results[2].rows, rows(&staged));
            if action != 2 {
                model = staged;
            }
        }
        if overflow || action >= 2 {
            assert_eq!(database.committed_wal().unwrap(), before);
            assert_eq!(database.last_transaction(), transaction);
        } else {
            assert_eq!(
                database.last_transaction(),
                transaction + u64::from(prefix_events > 0)
            );
        }
        drop(database);
        database = Database::open(&path).unwrap();
        assert_eq!(
            query(
                database.view().unwrap(),
                "SELECT id,n FROM t ORDER BY id",
                &[]
            )
            .unwrap()
            .rows,
            rows(&model)
        );
    }
    let archive = temporary.path().join("synthetic.backup");
    let report = emilybase_backup::create(&mut database, &archive).unwrap();
    assert_eq!(report.wal_version, if compact { 2 } else { 1 });
    let restored = temporary.path().join("restored");
    emilybase_backup::restore(&archive, &restored).unwrap();
    let copy = Database::open(&restored).unwrap();
    assert_eq!(
        query(copy.view().unwrap(), "SELECT id,n FROM t ORDER BY id", &[])
            .unwrap()
            .rows,
        rows(&model)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_bounded_mutations_match_independent_integer_and_text_states(
        text in any::<bool>(),compact in any::<bool>(),count in 260i16..310,
        operations in prop::collection::vec((0u8..4,-2i16..320,-2i16..320,-3i64..12,0u8..3),0..12)
    ) {
        model_run(text,compact,count,operations);
    }
}
