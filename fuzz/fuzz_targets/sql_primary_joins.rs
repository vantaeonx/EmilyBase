#![no_main]
#![forbid(unsafe_code)]
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::{explain, query};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;

fn value(number: i64, text: bool) -> Value {
    if !text {
        return Value::Integer(number);
    }
    Value::Text(match number.rem_euclid(5) {
        0 => format!("k{number}\0"),
        1 => format!("{number:04}{}", "λ".repeat(125)),
        2 => format!("{number:04}{}", "λ".repeat(126)),
        3 => format!("{number:04}{}", "λ".repeat(1400)),
        _ => format!("я{number}"),
    })
}
fuzz_target!(|input: &[u8]| {
    if input.len() < 3 || input.len() > 512 {
        return;
    }
    let text = input[0] & 1 != 0;
    let descending = input[0] & 2 != 0;
    let reversed = input[0] & 4 != 0;
    let filter = input[0] & 8 != 0;
    let mut snapshot = Snapshot::empty().unwrap();
    for (id, name) in [(1, "l"), (2, "r")] {
        let kinds = if id == 1 {
            [
                DataType::Integer,
                if text {
                    DataType::Text
                } else {
                    DataType::Integer
                },
            ]
        } else {
            [
                if text {
                    DataType::Text
                } else {
                    DataType::Integer
                },
                DataType::Integer,
            ]
        };
        snapshot
            .apply(Event {
                table_id: id,
                kind: EventKind::Create(Schema {
                    name: name.into(),
                    columns: ["id", "link"]
                        .into_iter()
                        .zip(kinds)
                        .enumerate()
                        .map(|(i, (name, data_type))| Column {
                            name: name.into(),
                            data_type,
                            nullable: id == 1 && i == 1,
                        })
                        .collect(),
                    primary_key: 0,
                }),
            })
            .unwrap();
    }
    let commands: Vec<_> = input[2..].as_chunks::<3>().0.iter().take(48).collect();
    let mut right = BTreeMap::new();
    for command in commands.iter().take(usize::from(input[1] % 24)) {
        right.insert(i64::from(command[0] as i8), i64::from(command[1] as i8));
    }
    for (key, payload) in &right {
        snapshot
            .apply(Event {
                table_id: 2,
                kind: EventKind::Insert(vec![value(*key, text), Value::Integer(*payload)]),
            })
            .unwrap();
    }
    let mut expected = Vec::new();
    for (id, command) in commands.iter().enumerate() {
        let key = i64::from(command[0] as i8);
        let missing = command[2] % 7 == 0;
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![
                    Value::Integer(id as i64),
                    if missing {
                        Value::Null
                    } else {
                        value(key, text)
                    },
                ]),
            })
            .unwrap();
        if !missing && right.get(&key).is_some_and(|v| !filter || *v >= 0) {
            expected.push(vec![Value::Integer(id as i64), value(key, text)]);
        }
    }
    let old = snapshot.clone();
    let digest = old.page_fingerprint();
    let equality = if reversed {
        "b.id=a.link"
    } else {
        "a.link=b.id"
    };
    let suffix = if filter { " AND b.link>=$1" } else { "" };
    let order = if descending { "DESC" } else { "ASC" };
    let limit = usize::from(input[1] % 32);
    let sql = format!(
        "SELECT a.id,b.id FROM l AS a JOIN r AS b ON {equality}{suffix} ORDER BY a.id {order} LIMIT {limit}"
    );
    let reference = format!(
        "SELECT a.id,b.id FROM l AS a JOIN r AS b ON ({equality}{suffix}) OR FALSE ORDER BY a.id {order} LIMIT {limit}"
    );
    let parameters = [Value::Integer(0)];
    if descending {
        expected.reverse();
    }
    expected.truncate(limit);
    assert_eq!(
        explain(&snapshot, &sql, &parameters).unwrap().access,
        "primary_join"
    );
    assert_eq!(
        explain(&snapshot, &reference, &parameters).unwrap().access,
        "bounded_nested_loop"
    );
    let actual = query(&snapshot, &sql, &parameters).unwrap();
    assert_eq!(actual.rows, expected);
    assert_eq!(actual, query(&snapshot, &reference, &parameters).unwrap());
    assert_eq!(snapshot.page_fingerprint(), digest);
    assert_eq!(old.page_fingerprint(), digest);
});
