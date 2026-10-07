use super::*;
use emilybase_catalog::{Column, DataType};

fn padded(text: &str) -> String {
    let mut value = String::with_capacity(4096);
    value.push_str(text);
    value
}
fn row() -> Row {
    let mut bytes = Vec::with_capacity(4096);
    bytes.extend_from_slice(&[0, 255, 1]);
    let mut row = Vec::with_capacity(64);
    row.extend([
        Value::Integer(i64::MIN),
        Value::Text(padded("я\0")),
        Value::Bytes(bytes),
        Value::Null,
        Value::Boolean(true),
        Value::Float(-0.0),
    ]);
    row
}
fn assert_row(row: &Row) {
    assert_eq!(row.capacity(), row.len());
    for value in row {
        match value {
            Value::Text(text) => assert_eq!(text.capacity(), text.len()),
            Value::Bytes(bytes) => assert_eq!(bytes.capacity(), bytes.len()),
            _ => (),
        }
    }
}
#[test]
fn compaction_preserves_insert_replace_wire_bytes_and_every_value_kind() {
    for kind in [EventKind::Insert(row()), EventKind::Replace(row())] {
        let event = Event { table_id: 1, kind };
        let before = event.encode().unwrap();
        let event = event.compact_payload();
        assert_eq!(event.encode().unwrap(), before);
        let row = match &event.kind {
            EventKind::Insert(r) | EventKind::Replace(r) => r,
            _ => unreachable!(),
        };
        assert_row(row);
        if let Value::Float(value) = row[5] {
            assert_eq!(value.to_bits(), (-0.0f64).to_bits());
        } else {
            panic!("missing float")
        }
    }
}
#[test]
fn schema_compaction_preserves_fields_and_original_event_bytes() {
    let mut columns = Vec::with_capacity(64);
    columns.push(Column {
        name: padded("id"),
        data_type: DataType::Text,
        nullable: false,
    });
    columns.push(Column {
        name: padded("payload"),
        data_type: DataType::Bytes,
        nullable: true,
    });
    let event = Event {
        table_id: 1,
        kind: EventKind::Create(Schema {
            name: padded("items"),
            columns,
            primary_key: 0,
        }),
    };
    let bytes = event.encode().unwrap();
    let event = event.compact_payload();
    assert_eq!(event.encode().unwrap(), bytes);
    if let EventKind::Create(schema) = event.kind {
        assert_eq!(schema.name.capacity(), schema.name.len());
        assert_eq!(schema.columns.capacity(), schema.columns.len());
        for column in schema.columns {
            assert_eq!(column.name.capacity(), column.name.len());
        }
    } else {
        panic!("missing schema")
    }
}
#[test]
fn zero_length_payloads_and_empty_internal_vector_release_spare_shape() {
    let mut values = Vec::with_capacity(64);
    values.push(Value::Text(String::with_capacity(4096)));
    values.push(Value::Bytes(Vec::with_capacity(4096)));
    let event = Event {
        table_id: 1,
        kind: EventKind::Insert(values),
    }
    .compact_payload();
    if let EventKind::Insert(row) = event.kind {
        assert_row(&row);
    } else {
        panic!("missing row")
    }
    let mut internal = Vec::<u64>::with_capacity(1000);
    compact_vector(&mut internal);
    assert_eq!((internal.len(), internal.capacity()), (0, 0));
}
#[test]
fn exact_capacity_fast_path_and_repeated_compaction_preserve_actual_buffers() {
    let event = Event {
        table_id: 1,
        kind: EventKind::Insert(row()),
    }
    .compact_payload();
    let pointers = |event: &Event| {
        let EventKind::Insert(row) = &event.kind else {
            panic!("missing row")
        };
        let Value::Text(text) = &row[1] else {
            panic!("missing text")
        };
        let Value::Bytes(bytes) = &row[2] else {
            panic!("missing bytes")
        };
        (row.as_ptr(), text.as_ptr(), bytes.as_ptr())
    };
    let before = pointers(&event);
    let event = event.compact_payload();
    assert_eq!(pointers(&event), before);
    let event = event.compact_payload();
    assert_eq!(pointers(&event), before);
}
#[test]
fn root_drop_and_integer_or_maximum_text_delete_keep_event_meaning() {
    for kind in [
        EventKind::Root,
        EventKind::Drop,
        EventKind::Delete(Key::Integer(i64::MAX)),
        EventKind::Delete(Key::Text(padded(&"я".repeat(1536)))),
    ] {
        let table_id = if matches!(kind, EventKind::Root) {
            0
        } else {
            1
        };
        let event = Event { table_id, kind };
        let bytes = event.encode().unwrap();
        let event = event.compact_payload();
        assert_eq!(event.encode().unwrap(), bytes);
        if let EventKind::Delete(Key::Text(text)) = event.kind {
            assert_eq!(text.capacity(), text.len());
            assert_eq!(text.len(), 3072);
        }
    }
}
