#![no_main]
#![forbid(unsafe_code)]
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::query;
use libfuzzer_sys::fuzz_target;
use std::cmp::Ordering;
use std::collections::BTreeMap;
fn key(number: u8, text: bool) -> Value {
    if !text {
        return Value::Integer(i64::from(number as i8));
    }
    Value::Text(match number % 5 {
        0 => format!("{number:03}я\0"),
        1 => format!("{number:03}{}", "λ".repeat(125)),
        2 => format!("{number:03}{}", "λ".repeat(127)),
        3 => format!("{number:03}{}", "λ".repeat(1534)),
        _ => format!("{number:03}"),
    })
}
fn compare(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Integer(a), Value::Integer(b)) => a.cmp(b),
        (Value::Text(a), Value::Text(b)) => a.cmp(b),
        _ => unreachable!("fixed generated key type"),
    }
}
fuzz_target!(|input: &[u8]| {
    if input.len() < 6 || input.len() > 512 {
        return;
    }
    let descending = input[0] & 1 != 0;
    let nulls_first = input[0] & 2 != 0;
    let text = input[0] & 4 != 0;
    let point = input[0] & 8 != 0;
    let under_or = input[0] & 16 != 0;
    let lower = key(input[1], text);
    let upper = key(input[2], text);
    let limit = usize::from(input[3] % 82);
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "t".into(),
                columns: vec![
                    Column {
                        name: "rank".into(),
                        data_type: DataType::Integer,
                        nullable: true,
                    },
                    Column {
                        name: "id".into(),
                        data_type: if text {
                            DataType::Text
                        } else {
                            DataType::Integer
                        },
                        nullable: false,
                    },
                ],
                primary_key: 1,
            }),
        })
        .unwrap();
    let mut model = BTreeMap::new();
    for command in input[4..].as_chunks::<2>().0.iter().take(80) {
        model.insert(
            command[0],
            if command[1] % 7 == 0 {
                None
            } else {
                Some(i64::from(command[1] % 13) - 6)
            },
        );
    }
    let mut expected = Vec::new();
    for (number, rank) in model {
        let id = key(number, text);
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![rank.map_or(Value::Null, Value::Integer), id.clone()]),
            })
            .unwrap();
        let selected = if point {
            compare(&id, &lower).is_eq()
        } else {
            compare(&id, &lower).is_ge() && compare(&id, &upper).is_lt()
        };
        if selected {
            expected.push((id, rank));
        }
    }
    // Restore the actual source primary order before independently stable rank sorting.
    expected.sort_by(|a, b| compare(&a.0, &b.0));
    expected.sort_by(|a, b| match (a.1, b.1) {
        (None, None) => Ordering::Equal,
        (None, _) => {
            if nulls_first {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (_, None) => {
            if nulls_first {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
        (Some(a), Some(b)) => {
            if descending {
                b.cmp(&a)
            } else {
                a.cmp(&b)
            }
        }
    });
    expected.truncate(limit);
    let expected = expected
        .into_iter()
        .map(|(id, rank)| vec![id, rank.map_or(Value::Null, Value::Integer)])
        .collect::<Vec<_>>();
    let predicate = if point {
        "x.id=$1"
    } else {
        "x.id>=$1 AND x.id<$2"
    };
    let predicate = if under_or {
        format!("({predicate}) OR FALSE")
    } else {
        predicate.into()
    };
    let sql = format!(
        "SELECT x.id,x.rank FROM t AS x WHERE {predicate} ORDER BY x.rank {} NULLS {} LIMIT {limit}",
        if descending { "DESC" } else { "ASC" },
        if nulls_first { "FIRST" } else { "LAST" }
    );
    let old = snapshot.clone();
    let digest = old.page_fingerprint();
    assert_eq!(
        query(&snapshot, &sql, &[lower, upper]).unwrap().rows,
        expected
    );
    assert_eq!(snapshot.page_fingerprint(), digest);
    assert_eq!(old.page_fingerprint(), digest);
});
