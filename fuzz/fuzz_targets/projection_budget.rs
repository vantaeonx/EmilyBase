#![no_main]
#![forbid(unsafe_code)]
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::{ExecutionError, MAX_OUTPUT_BYTES, query};
use libfuzzer_sys::fuzz_target;
use std::cmp::Ordering;

fuzz_target!(|input: &[u8]| {
    if input.len() < 8 || input.len() > 512 {
        return;
    }
    let rows = usize::from(input[3] % 81);
    let repeats = usize::from(input[2] % 64) + 1;
    let limit = usize::from(input[4] % 82);
    let descending = input[0] & 1 != 0;
    let nulls_first = input[0] & 2 != 0;
    let primary_order = input[0] & 4 != 0;
    let text = format!("λ\0{}", "я".repeat(usize::from(input[1] % 7) * 250));
    let bytes = input[7..].to_vec();
    let mut snapshot = Snapshot::empty().unwrap();
    for (id, name) in [(1, "t"), (2, "r")] {
        let fields = if id == 1 {
            vec![
                ("id", DataType::Integer, false),
                ("link", DataType::Integer, false),
                ("payload", DataType::Text, true),
                ("data", DataType::Bytes, true),
                ("rank", DataType::Integer, true),
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
    snapshot
        .apply(Event {
            table_id: 2,
            kind: EventKind::Insert(vec![Value::Integer(0)]),
        })
        .unwrap();
    let mut model = Vec::new();
    for id in 0..rows {
        let null = input[7 + id % (input.len() - 7)].is_multiple_of(5);
        let rank = if id % 5 == 0 {
            None
        } else {
            Some(((id + usize::from(input[5])) % 6) as i64 - 3)
        };
        let row = vec![
            Value::Integer(id as i64),
            Value::Integer(0),
            if null {
                Value::Null
            } else {
                Value::Text(text.clone())
            },
            if null {
                Value::Null
            } else {
                Value::Bytes(bytes.clone())
            },
            rank.map_or(Value::Null, Value::Integer),
        ];
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(row.clone()),
            })
            .unwrap();
        model.push((id, rank, row));
    }
    if primary_order {
        if descending {
            model.reverse();
        }
    } else {
        model.sort_by(|a, b| match (a.1, b.1) {
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
    }
    model.truncate(limit);
    let selected = (0..repeats)
        .map(|n| if n % 2 == 0 { 2 } else { 3 })
        .collect::<Vec<_>>();
    let mut charge = 0;
    for (_, _, row) in &model {
        charge += 24;
        for index in &selected {
            charge += 32
                + match &row[*index] {
                    Value::Text(v) => v.len(),
                    Value::Bytes(v) => v.len(),
                    _ => 0,
                };
        }
    }
    // Refused expectations must not themselves clone over-budget output.
    let expected = if charge <= MAX_OUTPUT_BYTES {
        Some(
            model
                .iter()
                .map(|(_, _, row)| selected.iter().map(|n| row[*n].clone()).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
        )
    } else {
        None
    };
    let columns = selected
        .iter()
        .map(|n| if *n == 2 { "a.payload" } else { "a.data" })
        .collect::<Vec<_>>()
        .join(",");
    let order = if primary_order {
        format!("a.id {}", if descending { "DESC" } else { "ASC" })
    } else {
        format!(
            "a.rank {} NULLS {}",
            if descending { "DESC" } else { "ASC" },
            if nulls_first { "FIRST" } else { "LAST" }
        )
    };
    let old = snapshot.clone();
    let digest = old.page_fingerprint();
    for from in [
        "t AS a",
        "t AS a JOIN r AS b ON a.link=b.id",
        "t AS a JOIN r AS b ON a.link=b.id OR FALSE",
    ] {
        let sql = format!("SELECT {columns} FROM {from} ORDER BY {order} LIMIT {limit}");
        match (query(&snapshot, &sql, &[]), &expected) {
            (Ok(actual), Some(expected)) => {
                assert_eq!(&actual.rows, expected);
                assert_eq!(actual.columns.len(), repeats);
            }
            (Err(ExecutionError::Limit("output bytes")), None) => {}
            (actual, expected) => panic!(
                "projection/model disagreement: success={} expected={} charge={charge}",
                actual.is_ok(),
                expected.is_some()
            ),
        }
    }
    assert_eq!(snapshot.page_fingerprint(), digest);
    assert_eq!(old.page_fingerprint(), digest);
});
