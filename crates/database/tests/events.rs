use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{DATABASE_MARKER, Event, EventKind};
use proptest::prelude::*;

#[test]
fn event_kinds_round_trip_and_root_has_golden_bytes() {
    let schema = Schema {
        name: "items".into(),
        columns: vec![Column {
            name: "id".into(),
            data_type: DataType::Integer,
            nullable: false,
        }],
        primary_key: 0,
    };
    let root = Event {
        table_id: 0,
        kind: EventKind::Root,
    };
    assert_eq!(root.encode().unwrap(), DATABASE_MARKER);
    assert_eq!(Event::decode(&DATABASE_MARKER).unwrap(), root);
    for kind in [
        EventKind::Create(schema),
        EventKind::Drop,
        EventKind::Insert(vec![Value::Integer(7)]),
        EventKind::Replace(vec![Value::Integer(7)]),
        EventKind::Delete(Key::Integer(7)),
        EventKind::Delete(Key::Text("text key".into())),
    ] {
        let event = Event { table_id: 1, kind };
        assert_eq!(Event::decode(&event.encode().unwrap()).unwrap(), event);
    }
}

#[test]
fn malformed_event_headers_fail_closed() {
    let valid = Event {
        table_id: 1,
        kind: EventKind::Insert(vec![Value::Integer(7)]),
    }
    .encode()
    .unwrap();
    for end in 0..valid.len() {
        assert!(Event::decode(&valid[..end]).is_err());
    }
    for (offset, replacement) in [(0, 0), (4, 2), (6, 255), (7, 1), (8, 0)] {
        let mut corrupt = valid.clone();
        corrupt[offset] = replacement;
        assert!(Event::decode(&corrupt).is_err());
    }
    let mut root = DATABASE_MARKER.to_vec();
    root.push(1);
    assert!(Event::decode(&root).is_err());
    assert!(
        Event {
            table_id: 1,
            kind: EventKind::Root
        }
        .encode()
        .is_err()
    );
    assert!(
        Event {
            table_id: 0,
            kind: EventKind::Drop
        }
        .encode()
        .is_err()
    );
}

#[test]
fn invalid_delete_key_payloads_are_rejected() {
    for row in [
        vec![],
        vec![Value::Null],
        vec![Value::Float(1.0)],
        vec![Value::Integer(1), Value::Integer(2)],
    ] {
        let mut bytes = Event {
            table_id: 1,
            kind: EventKind::Drop,
        }
        .encode()
        .unwrap();
        bytes[6] = 5;
        bytes.extend_from_slice(&emilybase_catalog::encode_row(&row).unwrap());
        assert!(Event::decode(&bytes).is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_event_bytes_are_rejected_or_round_trip(bytes in prop::collection::vec(any::<u8>(), 0..5000)) {
        if let Ok(event) = Event::decode(&bytes) {
            prop_assert_eq!(Event::decode(&event.encode().unwrap()).unwrap(), event);
        }
    }
}
