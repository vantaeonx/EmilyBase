#![no_main]
#![forbid(unsafe_code)]
use emilybase_catalog::{Column, DataType, Key, Row, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;

fn padded(text: &str, capacity: usize) -> String {
    let mut s = String::with_capacity(capacity);
    s.push_str(text);
    s
}
fn schema() -> Schema {
    let mut columns = Vec::with_capacity(1024);
    for (name, data_type) in [
        ("id", DataType::Integer),
        ("text", DataType::Text),
        ("bytes", DataType::Bytes),
    ] {
        columns.push(Column {
            name: padded(name, 4096),
            data_type,
            nullable: false,
        });
    }
    Schema {
        name: padded("items", 4096),
        columns,
        primary_key: 0,
    }
}
fn row(key: i64, number: u8, capacity: usize) -> Row {
    let text = format!("я\0{number}");
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(&[0, number, 255]);
    let mut row = Vec::with_capacity(1024);
    row.extend([
        Value::Integer(key),
        Value::Text(padded(&text, capacity)),
        Value::Bytes(bytes),
    ]);
    row
}
fn check(snapshot: &Snapshot, expected: &BTreeMap<i64, Row>) {
    let schema = snapshot.schema("items").unwrap();
    assert_eq!(schema.name.capacity(), schema.name.len());
    assert_eq!(schema.columns.capacity(), schema.columns.len());
    for column in &schema.columns {
        assert_eq!(column.name.capacity(), column.name.len());
    }
    assert_eq!(snapshot.row_count(), expected.len());
    for key in 0..8 {
        let stored = snapshot.get("items", &Key::Integer(key)).unwrap();
        assert_eq!(stored, expected.get(&key));
        if let Some(row) = stored {
            assert_eq!(row.capacity(), row.len());
            for value in row {
                match value {
                    Value::Text(s) => assert_eq!(s.capacity(), s.len()),
                    Value::Bytes(b) => assert_eq!(b.capacity(), b.len()),
                    _ => (),
                }
            }
            let location = snapshot
                .row_location("items", &Key::Integer(key))
                .unwrap()
                .unwrap();
            assert_eq!(
                snapshot
                    .resolve_row_location("items", &Key::Integer(key), location)
                    .unwrap(),
                row
            );
        }
    }
}
fuzz_target!(|data: &[u8]| {
    if data.len() > 512 {
        return;
    }
    let mut actual = Snapshot::empty().unwrap();
    let mut canonical = Snapshot::empty().unwrap();
    let create = Event {
        table_id: 1,
        kind: EventKind::Create(schema()),
    };
    let bytes = create.encode().unwrap();
    actual.apply(create).unwrap();
    canonical.apply(Event::decode(&bytes).unwrap()).unwrap();
    let mut expected = BTreeMap::<i64, Row>::new();
    let mut old = Vec::new();
    for command in data.as_chunks::<2>().0.iter().take(64) {
        let action = command[0] % 6;
        let key = i64::from(command[1] % 8);
        if action == 3 {
            if old.len() == 4 {
                old.remove(0);
            }
            old.push((actual.clone(), expected.clone()));
        } else {
            let value = command[0];
            let capacity = 1usize << (usize::from(command[1] % 10) + 3);
            let input = row(key, value, capacity);
            let payload = vec![
                Value::Integer(key),
                Value::Text(format!("я\0{value}")),
                Value::Bytes(vec![0, value, 255]),
            ];
            let event = Event {
                table_id: 1,
                kind: match action {
                    0 => EventKind::Insert(input),
                    1 => EventKind::Replace(input),
                    2 => EventKind::Delete(Key::Integer(key)),
                    4 => EventKind::Insert(vec![Value::Integer(key)]),
                    _ => EventKind::Insert(vec![
                        Value::Integer(key),
                        Value::Text("x".repeat(3073)),
                        Value::Bytes(Vec::new()),
                    ]),
                },
            };
            let before = actual.page_fingerprint();
            let wire = event.encode();
            let success = if action == 0 {
                !expected.contains_key(&key)
            } else if action <= 2 {
                expected.contains_key(&key)
            } else {
                false
            };
            assert_eq!(actual.apply(event).is_ok(), success);
            if let Ok(wire) = wire {
                assert_eq!(
                    canonical.apply(Event::decode(&wire).unwrap()).is_ok(),
                    success
                );
            } else {
                assert!(!success);
            }
            if success {
                if action == 2 {
                    expected.remove(&key);
                } else {
                    expected.insert(key, payload);
                }
            } else {
                assert_eq!(actual.page_fingerprint(), before);
            }
        }
        assert_eq!(actual.page_fingerprint(), canonical.page_fingerprint());
        check(&actual, &expected);
        for (snapshot, rows) in &old {
            check(snapshot, rows);
        }
    }
});
