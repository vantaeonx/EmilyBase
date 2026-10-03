#![no_main]
#![forbid(unsafe_code)]
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::{MAX_SQL_BYTES, explain, query};
use libfuzzer_sys::fuzz_target;
use std::sync::OnceLock;

fn snapshot() -> &'static Snapshot {
    static SNAPSHOT: OnceLock<Snapshot> = OnceLock::new();
    SNAPSHOT.get_or_init(|| {
        let mut snapshot = Snapshot::empty().unwrap();
        for (id, name) in [(1, "items"), (2, "labels")] {
            snapshot
                .apply(Event {
                    table_id: id,
                    kind: EventKind::Create(Schema {
                        name: name.into(),
                        columns: vec![
                            Column {
                                name: "id".into(),
                                data_type: DataType::Integer,
                                nullable: false,
                            },
                            Column {
                                name: "title".into(),
                                data_type: DataType::Text,
                                nullable: true,
                            },
                            Column {
                                name: "active".into(),
                                data_type: DataType::Boolean,
                                nullable: true,
                            },
                        ],
                        primary_key: 0,
                    }),
                })
                .unwrap();
            for i in 0..24 {
                snapshot
                    .apply(Event {
                        table_id: id,
                        kind: EventKind::Insert(vec![
                            Value::Integer(i),
                            if i % 3 == 0 {
                                Value::Null
                            } else {
                                Value::Text(format!("clé-{i}"))
                            },
                            match i % 3 {
                                0 => Value::Null,
                                1 => Value::Boolean(false),
                                _ => Value::Boolean(true),
                            },
                        ]),
                    })
                    .unwrap();
            }
        }
        snapshot
            .apply(Event {
                table_id: 3,
                kind: EventKind::Create(Schema {
                    name: "words".into(),
                    columns: vec![Column {
                        name: "id".into(),
                        data_type: DataType::Text,
                        nullable: false,
                    }],
                    primary_key: 0,
                }),
            })
            .unwrap();
        for key in [
            String::new(),
            "\0".into(),
            "a".into(),
            "a\0".into(),
            "b".into(),
            "界".into(),
            "😀".into(),
            "z".repeat(255),
            "z".repeat(256),
            "z".repeat(257),
            format!("a{}", "z".repeat(3071)),
        ] {
            snapshot
                .apply(Event {
                    table_id: 3,
                    kind: EventKind::Insert(vec![Value::Text(key)]),
                })
                .unwrap();
        }
        snapshot
    })
}

fuzz_target!(|bytes: &[u8]| {
    if bytes.len() > MAX_SQL_BYTES {
        return;
    }
    let snapshot = snapshot();
    if let Ok(sql) = std::str::from_utf8(bytes) {
        let parameters = [
            Value::Integer(1),
            Value::Text("'; DROP TABLE items --".into()),
        ];
        if let Ok(result) = query(snapshot, sql, &parameters) {
            let plan = explain(snapshot, sql, &parameters).unwrap();
            assert!(result.rows.len() <= plan.limit);
            for row in result.rows {
                assert_eq!(row.len(), result.columns.len());
                for value in row {
                    value.validate().unwrap();
                }
            }
        }
    }
    if let [a, b, c, d, limit, ..] = bytes {
        let lower = i16::from_le_bytes([*a, *b]) as i64;
        let upper = i16::from_le_bytes([*c, *d]) as i64;
        let limit = i64::from(*limit % 25);
        let result = query(
            snapshot,
            "SELECT id FROM items WHERE id >= $1 AND id < $2 ORDER BY id DESC LIMIT $3",
            &[
                Value::Integer(lower),
                Value::Integer(upper),
                Value::Integer(limit),
            ],
        )
        .unwrap();
        let expected = (0..24)
            .rev()
            .filter(|i| *i >= lower && *i < upper)
            .take(limit as usize)
            .map(|i| vec![Value::Integer(i)])
            .collect::<Vec<_>>();
        assert_eq!(result.rows, expected);
        let length = bytes.len().min(64);
        let lower = String::from_utf8_lossy(&bytes[..length]).into_owned();
        let upper = String::from_utf8_lossy(&bytes[bytes.len() - length..]).into_owned();
        let result = query(
            snapshot,
            "SELECT id FROM words WHERE id > $1 AND id <= $2 ORDER BY id DESC LIMIT $3",
            &[
                Value::Text(lower.clone()),
                Value::Text(upper.clone()),
                Value::Integer(limit),
            ],
        )
        .unwrap();
        let expected = snapshot
            .scan("words", 100)
            .unwrap()
            .into_iter()
            .rev()
            .filter(|row| matches!(&row[0],Value::Text(key) if *key>lower && *key<=upper))
            .take(limit as usize)
            .collect::<Vec<_>>();
        assert_eq!(result.rows, expected);
    }
});
