#![no_main]
#![forbid(unsafe_code)]
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::{explain, query};
use libfuzzer_sys::fuzz_target;
use std::cmp::Ordering;
use std::collections::BTreeMap;
fn flag(byte: u8) -> Value {
    match byte % 3 {
        0 => Value::Null,
        1 => Value::Boolean(false),
        _ => Value::Boolean(true),
    }
}
fn truth(value: &Value) -> Option<bool> {
    match value {
        Value::Null => None,
        Value::Boolean(b) => Some(*b),
        _ => unreachable!("generated flag"),
    }
}
fn rank(byte: u8) -> Value {
    if byte.is_multiple_of(7) {
        Value::Null
    } else {
        Value::Integer(i64::from(byte % 13) - 6)
    }
}
fn order(a: &Value, b: &Value, desc: bool, null_first: bool) -> Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => {
            if null_first {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (_, Value::Null) => {
            if null_first {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
        (Value::Integer(a), Value::Integer(b)) => {
            if desc {
                b.cmp(a)
            } else {
                a.cmp(b)
            }
        }
        _ => unreachable!("generated rank"),
    }
}
fuzz_target!(|input: &[u8]| {
    if input.len() < 8 || input.len() > 512 {
        return;
    }
    let desc = input[0] & 1 != 0;
    let null_first = input[0] & 2 != 0;
    let right_order = input[0] & 4 != 0;
    let extra_on = input[0] & 8 != 0;
    let extra_where = input[0] & 16 != 0;
    let limit = usize::from(input[1] % 43);
    let mut left = BTreeMap::new();
    let mut right = BTreeMap::new();
    for cmd in input[2..].as_chunks::<4>().0.iter().take(40) {
        let id = i64::from(cmd[0] % 20);
        let a = vec![
            rank(cmd[1]),
            Value::Integer(id),
            flag(cmd[2]),
            Value::Text("я\0".repeat(usize::from(cmd[3] % 17))),
        ];
        let b = vec![
            rank(cmd[2]),
            Value::Integer(id),
            flag(cmd[3]),
            Value::Bytes(vec![cmd[1]; usize::from(cmd[3] % 33)]),
        ];
        left.insert(id, a);
        if cmd[3] & 1 == 0 {
            right.insert(id, b);
        } else {
            right.remove(&id);
        }
    }
    let mut snapshot = Snapshot::empty().unwrap();
    for (table_id, name, rows) in [(1, "a", &left), (2, "b", &right)] {
        snapshot
            .apply(Event {
                table_id,
                kind: EventKind::Create(Schema {
                    name: name.into(),
                    columns: [
                        ("rank", DataType::Integer),
                        ("id", DataType::Integer),
                        ("flag", DataType::Boolean),
                        (
                            "hidden",
                            if table_id == 1 {
                                DataType::Text
                            } else {
                                DataType::Bytes
                            },
                        ),
                    ]
                    .into_iter()
                    .map(|(name, data_type)| Column {
                        name: name.into(),
                        data_type,
                        nullable: name == "rank" || name == "flag",
                    })
                    .collect(),
                    primary_key: 1,
                }),
            })
            .unwrap();
        for row in rows.values() {
            snapshot
                .apply(Event {
                    table_id,
                    kind: EventKind::Insert(row.clone()),
                })
                .unwrap();
        }
    }
    let mut expected = Vec::new();
    for a in left.values() {
        for b in right.values() {
            // SQL NULL equality is unknown; only true pairs survive ON.
            if a[0] == Value::Null || b[0] == Value::Null || a[0] != b[0] {
                continue;
            }
            if extra_on && truth(&a[2]) != Some(true) && truth(&b[2]) != Some(true) {
                continue;
            }
            if extra_where && !matches!(&b[0],Value::Integer(v) if *v>=0) && b[2] != Value::Null {
                continue;
            }
            expected.push((a, b));
        }
    }
    // Original input is primary ordered; stable sorting keeps source ties.
    expected.sort_by(|(a, b), (c, d)| {
        order(
            if right_order { &b[0] } else { &a[0] },
            if right_order { &d[0] } else { &c[0] },
            desc,
            null_first,
        )
    });
    expected.truncate(limit);
    let expected = expected
        .into_iter()
        .map(|(a, b)| {
            vec![
                a[1].clone(),
                a[0].clone(),
                b[0].clone(),
                a[3].clone(),
                b[3].clone(),
                a[3].clone(),
            ]
        })
        .collect::<Vec<_>>();
    let on = if extra_on {
        "a.rank=b.rank AND (a.flag OR b.flag)"
    } else {
        "a.rank=b.rank"
    };
    let filter = if extra_where {
        " WHERE b.rank>=0 OR b.flag IS NULL"
    } else {
        ""
    };
    let sort = if right_order { "b.rank" } else { "a.rank" };
    let suffix = format!(
        "{filter} ORDER BY {sort} {} NULLS {} LIMIT $1",
        if desc { "DESC" } else { "ASC" },
        if null_first { "FIRST" } else { "LAST" }
    );
    let selected = "a.id,a.rank,b.rank,a.hidden,b.hidden,a.hidden";
    let sql = format!("SELECT {selected} FROM a JOIN b ON {on}{suffix}");
    let fallback = format!("SELECT {selected} FROM a JOIN b ON ({on}) OR FALSE{suffix}");
    let old = snapshot.clone();
    let digest = old.page_fingerprint();
    let parameters = [Value::Integer(limit as i64)];
    assert_eq!(
        explain(&snapshot, &sql, &parameters).unwrap().access,
        "bounded_nested_loop"
    );
    let result = query(&snapshot, &sql, &parameters).unwrap();
    assert_eq!(result.rows, expected);
    assert_eq!(
        query(&snapshot, &fallback, &parameters).unwrap().rows,
        result.rows
    );
    assert_eq!(old.page_fingerprint(), digest);
    assert_eq!(snapshot.page_fingerprint(), digest);
});
