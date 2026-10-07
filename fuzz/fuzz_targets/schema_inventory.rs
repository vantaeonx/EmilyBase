#![no_main]
#![forbid(unsafe_code)]

use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;

fn schema(number: u8, width: usize) -> Schema {
    Schema {
        name: format!("t{number}"),
        columns: (0..width)
            .map(|i| Column {
                name: format!("c{i:02}_{}", "x".repeat(50)),
                data_type: DataType::Integer,
                nullable: false,
            })
            .collect(),
        primary_key: (width - 1) as u16,
    }
}
fn verify(view: &Snapshot, expected: &BTreeMap<u64, Schema>) {
    assert_eq!(view.table_count(), expected.len());
    let mut schemas = view.schema_refs();
    let mut remaining = expected.values().collect::<Vec<_>>();
    while !remaining.is_empty() {
        assert_eq!(schemas.len(), remaining.len());
        let schema = if remaining.len() % 2 == 0 {
            schemas.next_back().unwrap()
        } else {
            schemas.next().unwrap()
        };
        let expected = if remaining.len() % 2 == 0 {
            remaining.pop().unwrap()
        } else {
            remaining.remove(0)
        };
        assert_eq!(schema, expected);
        assert!(std::ptr::eq(schema, view.schema(&schema.name).unwrap()));
    }
    assert_eq!(schemas.size_hint(), (0, Some(0)));
    assert!(schemas.next().is_none());
    assert!(schemas.next_back().is_none());
    assert!(schemas.next().is_none());
    assert_eq!(
        view.schemas(),
        expected.values().cloned().collect::<Vec<_>>()
    );
}

fuzz_target!(|bytes: &[u8]| {
    if bytes.is_empty() || bytes.len() > 256 {
        return;
    }
    let mut live = Snapshot::empty().unwrap();
    let mut expected = BTreeMap::<u64, Schema>::new();
    for command in bytes.as_chunks::<3>().0.iter().take(32) {
        let old = live.clone();
        let old_expected = expected.clone();
        let number = command[1] % 8;
        let width = usize::from(command[2] % 64) + 1;
        let schema = schema(number, width);
        let existing = expected
            .iter()
            .find(|(_, s)| s.name == schema.name)
            .map(|(id, s)| (*id, s.columns.len()));
        let operation = command[0] % 4;
        let mut candidate = live.clone();
        if operation < 2 {
            let table_id = if operation == 1 {
                existing.map_or(u64::MAX, |(id, _)| id)
            } else {
                candidate.next_table_id()
            };
            let succeeds = if operation == 1 {
                existing.is_some()
            } else {
                existing.is_none()
            };
            let event = Event {
                table_id,
                kind: if operation == 1 {
                    EventKind::Drop
                } else {
                    EventKind::Create(schema.clone())
                },
            };
            assert_eq!(candidate.apply(event).is_ok(), succeeds);
            if succeeds {
                if operation == 1 {
                    expected.remove(&table_id);
                } else {
                    expected.insert(table_id, schema);
                }
                live = candidate;
            }
        } else if let Some((id, width)) = existing {
            let row = vec![Value::Integer(i64::from(command[2])); width];
            let event = Event {
                table_id: id,
                kind: EventKind::Insert(row),
            };
            // A discarded candidate may detach its schema without changing the old view.
            if candidate.apply(event).is_ok() && operation == 2 {
                live = candidate;
            }
        }
        verify(&live, &expected);
        verify(&old, &old_expected);
        assert_eq!(
            old.page_fingerprint(),
            Snapshot::from_pages(old.pages().cloned().collect())
                .unwrap()
                .page_fingerprint()
        );
    }
    let replay = Snapshot::from_pages(live.pages().cloned().collect()).unwrap();
    verify(&replay, &expected);
    assert_eq!(replay.page_fingerprint(), live.page_fingerprint());
    assert_eq!(replay.row_count(), live.row_count());
});
