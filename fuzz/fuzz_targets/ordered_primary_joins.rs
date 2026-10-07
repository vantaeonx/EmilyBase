#![no_main]
#![forbid(unsafe_code)]
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::{explain, query};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;

fn key(number: u8, text: bool) -> Value {
    if !text {
        return Value::Integer(i64::from(number as i8));
    }
    Value::Text(match number % 5 {
        0 => format!("{number:03}я\0"),
        1 => format!("{number:03}{}", "λ".repeat(125)),
        2 => format!("{number:03}{}", "λ".repeat(127)),
        3 => format!("{number:03}{}", "λ".repeat(1400)),
        _ => format!("{number:03}"),
    })
}
fuzz_target!(|input: &[u8]| {
    if input.len() < 5 || input.len() > 512 {
        return;
    }
    let text = input[0] & 1 != 0;
    let descending = input[0] & 2 != 0;
    let point = input[0] & 4 != 0;
    let mut snapshot = Snapshot::empty().unwrap();
    for (id, name) in [(1, "l"), (2, "r")] {
        let fields = if id == 1 {
            vec![
                (
                    "id",
                    if text {
                        DataType::Text
                    } else {
                        DataType::Integer
                    },
                    false,
                ),
                ("link", DataType::Integer, true),
            ]
        } else {
            vec![("id", DataType::Integer, false)]
        };
        snapshot
            .apply(Event {
                table_id: id,
                kind: EventKind::Create(Schema {
                    name: name.into(),
                    columns: fields
                        .into_iter()
                        .map(|(name, data_type, nullable)| Column {
                            name: name.into(),
                            data_type,
                            nullable,
                        })
                        .collect(),
                    primary_key: 0,
                }),
            })
            .unwrap();
    }
    for id in 0..8 {
        snapshot
            .apply(Event {
                table_id: 2,
                kind: EventKind::Insert(vec![Value::Integer(id)]),
            })
            .unwrap();
    }
    let mut model = BTreeMap::new();
    for command in input[4..].as_chunks::<2>().0.iter().take(48) {
        model.insert(
            command[0],
            if command[1] % 5 == 0 {
                None
            } else {
                Some(i64::from(command[1] % 12))
            },
        );
    }
    let mut expected = Vec::new();
    let lower = key(input[1], text);
    let upper = key(input[2], text);
    for (number, link) in model {
        let value = key(number, text);
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![
                    value.clone(),
                    link.map_or(Value::Null, Value::Integer),
                ]),
            })
            .unwrap();
        let compare = |a: &Value, b: &Value| match (a, b) {
            (Value::Integer(a), Value::Integer(b)) => a.cmp(b),
            (Value::Text(a), Value::Text(b)) => a.cmp(b),
            _ => unreachable!("same generated primary type"),
        };
        let selected = if point {
            compare(&value, &lower).is_eq()
        } else {
            compare(&value, &lower).is_ge() && compare(&value, &upper).is_lt()
        };
        if selected && link.is_some_and(|v| (2..8).contains(&v)) {
            expected.push(vec![value, Value::Integer(link.unwrap())]);
        }
    }
    expected.sort_by(|a, b| match (&a[0], &b[0]) {
        (Value::Integer(a), Value::Integer(b)) => a.cmp(b),
        (Value::Text(a), Value::Text(b)) => a.cmp(b),
        _ => unreachable!("same generated primary type"),
    });
    if descending {
        expected.reverse();
    }
    let limit = usize::from(input[3] % 20);
    expected.truncate(limit);
    let predicate = if point {
        "a.id=$1"
    } else {
        "a.id>=$1 AND a.id<$2"
    };
    let order = if descending { "DESC" } else { "ASC" };
    let sql = format!(
        "SELECT a.id,b.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE {predicate} AND b.id>=2 ORDER BY a.id {order},b.id DESC LIMIT {limit}"
    );
    let reference = sql.replace("ON a.link=b.id WHERE", "ON a.link=b.id OR FALSE WHERE");
    let parameters = [lower, upper];
    let old = snapshot.clone();
    let digest = old.page_fingerprint();
    let result = query(&snapshot, &sql, &parameters).unwrap();
    assert_eq!(result.rows, expected);
    assert_eq!(result, query(&snapshot, &reference, &parameters).unwrap());
    assert_eq!(
        explain(&snapshot, &sql, &parameters).unwrap().access,
        "primary_join"
    );
    assert_eq!(snapshot.page_fingerprint(), digest);
    assert_eq!(old.page_fingerprint(), digest);
});
