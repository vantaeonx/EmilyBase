#![cfg(feature = "heap-profile")]
//! Input construction is included; the empty fixture precedes profiling.
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[test]
fn live_row_payload_does_not_keep_megabytes_of_unused_input_capacity() {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "items".into(),
                primary_key: 0,
                columns: vec![
                    Column {
                        name: "id".into(),
                        data_type: DataType::Integer,
                        nullable: false,
                    },
                    Column {
                        name: "text".into(),
                        data_type: DataType::Text,
                        nullable: false,
                    },
                    Column {
                        name: "bytes".into(),
                        data_type: DataType::Bytes,
                        nullable: false,
                    },
                ],
            }),
        })
        .unwrap();
    let profiler = dhat::Profiler::builder().testing().build();
    let mut text = String::with_capacity(1024 * 1024);
    text.push_str("я\0");
    let mut bytes = Vec::with_capacity(1024 * 1024);
    bytes.extend_from_slice(&[0, 1, 2]);
    let mut row = Vec::with_capacity(1024);
    row.extend([Value::Integer(7), Value::Text(text), Value::Bytes(bytes)]);
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(row),
        })
        .unwrap();
    assert_eq!(
        snapshot.get("items", &Key::Integer(7)).unwrap().unwrap()[1],
        Value::Text("я\0".into())
    );
    let live = dhat::HeapStats::get();
    assert!(
        live.max_bytes >= 2 * 1024 * 1024,
        "input allocation must be included"
    );
    assert!(
        live.curr_bytes < 64 * 1024,
        "unused input retained {} bytes",
        live.curr_bytes
    );
    drop(snapshot);
    let released = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!(released.curr_bytes, 0);
    // Test-output capture owns its own buffer; report only after profiling ends.
    eprintln!(
        "retained row live={} peak={} released={}",
        live.curr_bytes, live.max_bytes, released.curr_bytes
    );
}
