use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};

fn padded(text: &str) -> String {
    let mut s = String::with_capacity(128 * 1024);
    s.push_str(text);
    s
}
fn schema() -> Schema {
    let mut columns = Vec::with_capacity(1024);
    for (name, kind) in [
        ("id", DataType::Integer),
        ("text", DataType::Text),
        ("bytes", DataType::Bytes),
    ] {
        columns.push(Column {
            name: padded(name),
            data_type: kind,
            nullable: false,
        });
    }
    Schema {
        name: padded("items"),
        columns,
        primary_key: 0,
    }
}
#[test]
fn accepted_schema_does_not_retain_caller_spare_capacity() {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(schema()),
        })
        .unwrap();
    let stored = snapshot.schema("items").unwrap();
    assert_eq!(stored.name.capacity(), stored.name.len());
    assert_eq!(stored.columns.capacity(), stored.columns.len());
    for column in &stored.columns {
        assert_eq!(column.name.capacity(), column.name.len());
    }
}
#[test]
fn accepted_insert_and_replace_do_not_retain_caller_spare_capacity() {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(schema()),
        })
        .unwrap();
    for replace in [false, true] {
        let mut row = Vec::with_capacity(1024);
        let mut bytes = Vec::with_capacity(128 * 1024);
        bytes.extend_from_slice(&[0, 1, 2]);
        row.extend([
            Value::Integer(7),
            Value::Text(padded("я\0")),
            Value::Bytes(bytes),
        ]);
        snapshot
            .apply(Event {
                table_id: 1,
                kind: if replace {
                    EventKind::Replace(row)
                } else {
                    EventKind::Insert(row)
                },
            })
            .unwrap();
        let stored = snapshot.get("items", &Key::Integer(7)).unwrap().unwrap();
        assert_eq!(stored.capacity(), stored.len());
        if let Value::Text(text) = &stored[1] {
            assert_eq!(text.capacity(), text.len());
        } else {
            panic!("missing text")
        }
        if let Value::Bytes(bytes) = &stored[2] {
            assert_eq!(bytes.capacity(), bytes.len());
        } else {
            panic!("missing bytes")
        }
    }
}

fn row(key: i64, text: &str, data: &[u8]) -> Vec<Value> {
    let mut r = Vec::with_capacity(1024);
    let mut b = Vec::with_capacity(128 * 1024);
    b.extend_from_slice(data);
    r.extend([
        Value::Integer(key),
        Value::Text(padded(text)),
        Value::Bytes(b),
    ]);
    r
}
fn compact_shape(row: &Vec<Value>) {
    assert_eq!(row.capacity(), row.len());
    for v in row {
        match v {
            Value::Text(s) => assert_eq!(s.capacity(), s.len()),
            Value::Bytes(b) => assert_eq!(b.capacity(), b.len()),
            _ => (),
        }
    }
}
#[test]
fn inflated_inputs_preserve_exact_history_bytes_and_reopen_projection() {
    let mut inflated = Snapshot::empty().unwrap();
    let mut canonical = Snapshot::empty().unwrap();
    let source = schema();
    let decoded = Event::decode(
        &Event {
            table_id: 1,
            kind: EventKind::Create(source),
        }
        .encode()
        .unwrap(),
    )
    .unwrap();
    inflated
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(schema()),
        })
        .unwrap();
    canonical.apply(decoded).unwrap();
    for event in [
        Event {
            table_id: 1,
            kind: EventKind::Insert(row(7, "я\0", &[0, 255])),
        },
        Event {
            table_id: 1,
            kind: EventKind::Replace(row(7, "changed", &[])),
        },
    ] {
        let bytes = event.encode().unwrap();
        inflated.apply(event).unwrap();
        canonical.apply(Event::decode(&bytes).unwrap()).unwrap();
        assert_eq!(inflated.page_fingerprint(), canonical.page_fingerprint());
        assert_eq!(
            inflated.pages().map(|p| p.encode()).collect::<Vec<_>>(),
            canonical.pages().map(|p| p.encode()).collect::<Vec<_>>()
        );
        assert_eq!(
            inflated.row_location("items", &Key::Integer(7)).unwrap(),
            canonical.row_location("items", &Key::Integer(7)).unwrap()
        );
    }
    let reopened = Snapshot::from_pages(inflated.pages().cloned().collect()).unwrap();
    assert_eq!(reopened.page_fingerprint(), inflated.page_fingerprint());
    compact_shape(reopened.get("items", &Key::Integer(7)).unwrap().unwrap());
}
#[test]
fn old_snapshot_keeps_original_row_and_rejected_inputs_preserve_storage() {
    let mut current = Snapshot::empty().unwrap();
    current
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(schema()),
        })
        .unwrap();
    current
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(row(7, "old", &[1])),
        })
        .unwrap();
    let old = current.clone();
    let pointer = old
        .get("items", &Key::Integer(7))
        .unwrap()
        .unwrap()
        .as_ptr();
    current
        .apply(Event {
            table_id: 1,
            kind: EventKind::Replace(row(7, "new", &[2])),
        })
        .unwrap();
    assert_eq!(
        old.get("items", &Key::Integer(7))
            .unwrap()
            .unwrap()
            .as_ptr(),
        pointer
    );
    assert_eq!(
        old.get("items", &Key::Integer(7)).unwrap().unwrap()[1],
        Value::Text("old".into())
    );
    let bytes = current.pages().map(|p| p.encode()).collect::<Vec<_>>();
    for event in [
        Event {
            table_id: 1,
            kind: EventKind::Insert(row(7, "duplicate", &[])),
        },
        Event {
            table_id: 1,
            kind: EventKind::Replace(row(9, "missing", &[])),
        },
        Event {
            table_id: 1,
            kind: EventKind::Replace(row(7, &"x".repeat(3073), &[])),
        },
    ] {
        assert!(current.apply(event).is_err());
        assert_eq!(
            current.pages().map(|p| p.encode()).collect::<Vec<_>>(),
            bytes
        );
        assert_eq!(
            current.get("items", &Key::Integer(7)).unwrap().unwrap()[1],
            Value::Text("new".into())
        );
    }
    compact_shape(old.get("items", &Key::Integer(7)).unwrap().unwrap());
    compact_shape(current.get("items", &Key::Integer(7)).unwrap().unwrap());
}
#[test]
fn direct_file_engine_live_insert_update_and_reopen_have_compact_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("capacity.emily");
    let mut db = emilybase_database::Database::create(&path).unwrap();
    db.create_table(schema()).unwrap();
    db.insert("items", row(7, "old", &[1])).unwrap();
    compact_shape(db.get("items", &Key::Integer(7)).unwrap().unwrap());
    db.update("items", &Key::Integer(7), row(7, "new", &[2]))
        .unwrap();
    compact_shape(db.get("items", &Key::Integer(7)).unwrap().unwrap());
    let bytes = std::fs::read(&path).unwrap();
    drop(db);
    let db = emilybase_database::Database::open(&path).unwrap();
    compact_shape(db.get("items", &Key::Integer(7)).unwrap().unwrap());
    assert_eq!(
        db.get("items", &Key::Integer(7)).unwrap().unwrap()[1],
        Value::Text("new".into())
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
#[test]
fn maximum_columns_and_maximum_text_key_remain_accepted_without_spare_storage() {
    let mut columns = Vec::with_capacity(1024);
    columns.push(Column {
        name: padded("id"),
        data_type: DataType::Text,
        nullable: false,
    });
    for n in 1..64 {
        columns.push(Column {
            name: padded(&format!("value_{n}")),
            data_type: DataType::Text,
            nullable: true,
        });
    }
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: padded("wide"),
                columns,
                primary_key: 0,
            }),
        })
        .unwrap();
    let key = "я".repeat(1536);
    let mut row = Vec::with_capacity(1024);
    row.push(Value::Text(padded(&key)));
    row.extend(std::iter::repeat_n(Value::Null, 63));
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(row),
        })
        .unwrap();
    let stored = snapshot.get("wide", &Key::Text(key)).unwrap().unwrap();
    compact_shape(stored);
    assert_eq!(stored.len(), 64);
    assert_eq!(snapshot.schema("wide").unwrap().columns.capacity(), 64);
    assert_eq!(
        snapshot
            .primary_index_info("wide")
            .unwrap()
            .excluded_long_keys,
        1
    );
}

use proptest::prelude::*;
use std::collections::BTreeMap;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn inflated_mutation_history_matches_independent_rows_and_canonical_bytes(
        actions in prop::collection::vec((0u8..4,-5i64..5,".{0,16}",prop::collection::vec(any::<u8>(),0..16)),1..64)
    ) {
        let mut actual=Snapshot::empty().unwrap();let mut canonical=Snapshot::empty().unwrap();
        actual.apply(Event {table_id:1,kind:EventKind::Create(schema())}).unwrap();
        canonical.apply(Event::decode(&Event {table_id:1,kind:EventKind::Create(schema())}.encode().unwrap()).unwrap()).unwrap();
        let mut expected=BTreeMap::new();let mut old=Vec::new();
        for (op,key,text,bytes) in actions {
            if op==3 {
                if old.len()==4 {old.remove(0);}
                old.push((actual.clone(),expected.clone()));
            }
            let event=Event {table_id:1,kind:match op {
                0=>EventKind::Insert(row(key,&text,&bytes)),
                1=>EventKind::Replace(row(key,&text,&bytes)),
                _=>EventKind::Delete(Key::Integer(key)),
            }};
            let wire=event.encode().unwrap();let before=actual.page_fingerprint();
            let succeeds=if op==0 {!expected.contains_key(&key)} else {expected.contains_key(&key)};
            prop_assert_eq!(actual.apply(event).is_ok(),succeeds);
            prop_assert_eq!(canonical.apply(Event::decode(&wire).unwrap()).is_ok(),succeeds);
            if succeeds {
                if op>=2 {expected.remove(&key);} else {expected.insert(key,vec![Value::Integer(key),Value::Text(text),Value::Bytes(bytes)]);}
            } else {prop_assert_eq!(actual.page_fingerprint(),before);}
            prop_assert_eq!(actual.page_fingerprint(),canonical.page_fingerprint());
            prop_assert_eq!(actual.row_count(),expected.len());
            for key in -5..5 {
                let stored=actual.get("items",&Key::Integer(key)).unwrap();
                prop_assert_eq!(stored,expected.get(&key));
                if let Some(stored)=stored {compact_shape(stored);}
            }
            for (snapshot,rows) in &old {
                prop_assert_eq!(snapshot.row_count(),rows.len());
                for (key,row) in rows {prop_assert_eq!(snapshot.get("items",&Key::Integer(*key)).unwrap(),Some(row));compact_shape(snapshot.get("items",&Key::Integer(*key)).unwrap().unwrap());}
            }
        }
    }
}
