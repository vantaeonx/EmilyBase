#![cfg(feature = "heap-profile")]
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;
#[test]
fn repeated_physical_validation_does_not_materialize_payload_or_long_primary_keys() {
    let mut snapshot = Snapshot::empty().unwrap();
    let mut fixtures = Vec::new();
    for (table_id, name, long) in [(1, "integer", false), (2, "text", true)] {
        snapshot
            .apply(Event {
                table_id,
                kind: EventKind::Create(Schema {
                    name: name.into(),
                    columns: vec![
                        Column {
                            name: "id".into(),
                            data_type: if long {
                                DataType::Text
                            } else {
                                DataType::Integer
                            },
                            nullable: false,
                        },
                        Column {
                            name: "payload".into(),
                            data_type: if long {
                                DataType::Integer
                            } else {
                                DataType::Text
                            },
                            nullable: false,
                        },
                    ],
                    primary_key: 0,
                }),
            })
            .unwrap();
        let mut entries = Vec::new();
        for id in 0..1000 {
            let key = if long {
                Key::Text(format!("{id:04}{}", "я".repeat(1534)))
            } else {
                Key::Integer(id)
            };
            let payload = if long {
                Value::Integer(id)
            } else {
                Value::Text("x".repeat(3072))
            };
            snapshot
                .apply(Event {
                    table_id,
                    kind: EventKind::Insert(vec![key.to_value(), payload]),
                })
                .unwrap();
            entries.push((
                key.clone(),
                snapshot.row_location(name, &key).unwrap().unwrap(),
            ));
        }
        fixtures.push((name, long, entries));
    }
    let digest = snapshot.page_fingerprint();
    let mut totals = Vec::new();
    for (name, long, entries) in fixtures {
        let profiler = dhat::Profiler::builder().testing().build();
        for (key, location) in &entries {
            let row = snapshot.resolve_row_location(name, key, *location).unwrap();
            assert_eq!(row.len(), 2);
            if long {
                assert!(matches!(row[1], Value::Integer(_)));
            } else {
                assert!(matches!(row[1], Value::Text(_)));
            }
        }
        let observed = dhat::HeapStats::get();
        drop(profiler);
        assert_eq!(observed.curr_bytes, 0);
        eprintln!(
            "{name} total={} blocks={} peak={}",
            observed.total_bytes, observed.total_blocks, observed.max_bytes
        );
        totals.push((name, observed.total_bytes));
    }
    assert_eq!(snapshot.page_fingerprint(), digest);
    for (name, total) in totals {
        assert!(
            total < 128 * 1024,
            "physical {name} validation copied payload: {total}"
        );
    }
}
