#![no_main]
#![forbid(unsafe_code)]
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::query;
use libfuzzer_sys::fuzz_target;
use std::cmp::Ordering;
use std::collections::BTreeMap;

fuzz_target!(|input: &[u8]| {
    if input.len() < 23 || input.len() > 512 {
        return;
    }
    let descending = input[0] & 1 != 0;
    let nulls_first = input[0] & 2 != 0;
    let point = input[0] & 4 != 0;
    let secondary = input[0] & 8 != 0;
    let lower = i64::from(input[1] as i8);
    let upper = i64::from(input[2] as i8);
    let limit = usize::from(input[3] % 32);
    let minimum = i64::from(input[4] % 9) - 4;
    let mut snapshot = Snapshot::empty().unwrap();
    for (id, name) in [(1, "l"), (2, "r")] {
        let fields = if id == 1 {
            vec![("id", false), ("link", true)]
        } else {
            vec![("rank", true), ("id", false)]
        };
        snapshot
            .apply(Event {
                table_id: id,
                kind: EventKind::Create(Schema {
                    name: name.into(),
                    columns: fields
                        .into_iter()
                        .map(|(name, nullable)| Column {
                            name: name.into(),
                            data_type: DataType::Integer,
                            nullable,
                        })
                        .collect(),
                    primary_key: if id == 1 { 0 } else { 1 },
                }),
            })
            .unwrap();
    }
    let right = input[5..21]
        .iter()
        .map(|b| {
            if b % 5 == 0 {
                None
            } else {
                Some(i64::from(b % 9) - 4)
            }
        })
        .collect::<Vec<_>>();
    for (id, rank) in right.iter().enumerate() {
        snapshot
            .apply(Event {
                table_id: 2,
                kind: EventKind::Insert(vec![
                    rank.map_or(Value::Null, Value::Integer),
                    Value::Integer(id as i64),
                ]),
            })
            .unwrap();
    }
    let mut model = BTreeMap::new();
    for command in input[21..].as_chunks::<2>().0.iter().take(80) {
        model.insert(
            i64::from(command[0] as i8),
            if command[1] % 7 == 0 {
                None
            } else {
                Some(i64::from(command[1] % 20))
            },
        );
    }
    let mut expected = Vec::new();
    for (id, link) in model {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![
                    Value::Integer(id),
                    link.map_or(Value::Null, Value::Integer),
                ]),
            })
            .unwrap();
        if (point && id != lower) || (!point && (id < lower || id >= upper)) {
            continue;
        }
        if let Some(link) = link.filter(|v| *v < 16) {
            let rank = right[link as usize];
            if rank.is_none_or(|v| v >= minimum) {
                expected.push((id, link, rank));
            }
        }
    }
    expected.sort_by(|a, b| {
        let cmp = match (a.2, b.2) {
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
        };
        if secondary {
            cmp.then_with(|| b.1.cmp(&a.1))
        } else {
            cmp
        }
    });
    expected.truncate(limit);
    let expected = expected
        .into_iter()
        .map(|(id, link, rank)| {
            vec![
                Value::Integer(id),
                Value::Integer(link),
                rank.map_or(Value::Null, Value::Integer),
            ]
        })
        .collect::<Vec<_>>();
    let source = if point {
        "a.id=$1"
    } else {
        "a.id>=$1 AND a.id<$2"
    };
    let sql = format!(
        "SELECT a.id,b.id,b.rank FROM l AS a JOIN r AS b ON a.link=b.id WHERE {source} AND (b.rank IS NULL OR b.rank>=$3) ORDER BY b.rank {} NULLS {}{} LIMIT {limit}",
        if descending { "DESC" } else { "ASC" },
        if nulls_first { "FIRST" } else { "LAST" },
        if secondary { ",a.link DESC" } else { "" }
    );
    let reference = sql.replace("ON a.link=b.id WHERE", "ON a.link=b.id OR FALSE WHERE");
    let parameters = [
        Value::Integer(lower),
        Value::Integer(upper),
        Value::Integer(minimum),
    ];
    let old = snapshot.clone();
    let digest = old.page_fingerprint();
    let actual = query(&snapshot, &sql, &parameters).unwrap();
    assert_eq!(actual.rows, expected);
    assert_eq!(actual, query(&snapshot, &reference, &parameters).unwrap());
    assert_eq!(snapshot.page_fingerprint(), digest);
    assert_eq!(old.page_fingerprint(), digest);
});
