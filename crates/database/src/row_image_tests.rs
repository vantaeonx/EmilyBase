use super::*;
#[test]
fn matching_insert_replace_and_nonrow_kinds_preserve_full_validation() {
    let row = vec![
        Value::Integer(7),
        Value::Text("я\0".into()),
        Value::Float(-0.0),
    ];
    for kind in [
        EventKind::Insert(row.clone()),
        EventKind::Replace(row.clone()),
    ] {
        let bytes = Event { table_id: 9, kind }.encode().unwrap();
        assert!(Event::row_image_matches(&bytes, 9, &row).unwrap());
        assert!(!Event::row_image_matches(&bytes, 10, &row).unwrap());
        assert!(!Event::row_image_matches(&bytes, 9, &row[..2]).unwrap());
        let mut changed = row.clone();
        changed[0] = Value::Integer(8);
        assert!(!Event::row_image_matches(&bytes, 9, &changed).unwrap());
        changed = row.clone();
        changed[2] = Value::Float(0.0);
        assert!(Event::row_image_matches(&bytes, 9, &changed).unwrap());
    }
    for event in [
        Event {
            table_id: 0,
            kind: EventKind::Root,
        },
        Event {
            table_id: 9,
            kind: EventKind::Drop,
        },
        Event {
            table_id: 9,
            kind: EventKind::Delete(Key::Integer(7)),
        },
    ] {
        assert!(!Event::row_image_matches(&event.encode().unwrap(), 9, &row).unwrap());
    }
}
#[test]
fn mismatched_identity_still_checks_payload_and_all_envelope_errors_match_decode() {
    let row = vec![Value::Integer(7), Value::Boolean(true)];
    let valid = Event {
        table_id: 9,
        kind: EventKind::Insert(row),
    }
    .encode()
    .unwrap();
    for end in 0..valid.len() {
        let bytes = &valid[..end];
        assert_eq!(
            Event::row_image_matches(bytes, 10, &[])
                .unwrap_err()
                .to_string(),
            Event::decode(bytes).unwrap_err().to_string()
        );
    }
    for (offset, byte) in [(0, 0), (4, 2), (6, 255), (7, 1), (24, 255), (34, 2)] {
        let mut bytes = valid.clone();
        bytes[offset] = byte;
        assert_eq!(
            Event::row_image_matches(&bytes, 10, &[])
                .unwrap_err()
                .to_string(),
            Event::decode(&bytes).unwrap_err().to_string()
        );
    }
    let mut bytes = valid;
    bytes[8..16].fill(0);
    assert_eq!(
        Event::row_image_matches(&bytes, 10, &[])
            .unwrap_err()
            .to_string(),
        Event::decode(&bytes).unwrap_err().to_string()
    );
}
#[test]
fn malformed_nonrow_payload_is_not_misclassified_as_a_valid_nonmatch() {
    let mut bytes = Event {
        table_id: 9,
        kind: EventKind::Delete(Key::Integer(7)),
    }
    .encode()
    .unwrap();
    bytes[24] = 255;
    assert_eq!(
        Event::row_image_matches(&bytes, 9, &[])
            .unwrap_err()
            .to_string(),
        Event::decode(&bytes).unwrap_err().to_string()
    );
    let mut bytes = Event {
        table_id: 9,
        kind: EventKind::Drop,
    }
    .encode()
    .unwrap();
    bytes.push(0);
    assert_eq!(
        Event::row_image_matches(&bytes, 9, &[])
            .unwrap_err()
            .to_string(),
        Event::decode(&bytes).unwrap_err().to_string()
    );
}
