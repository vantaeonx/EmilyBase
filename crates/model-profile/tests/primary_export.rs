#![cfg(feature = "heap-profile")]
//! The complete retained relational fixture/cache is outside the operation sample.
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[test]
fn full_primary_export_keeps_admission_without_complete_intermediate_images() {
    let mut base = Snapshot::empty().unwrap();
    base.apply(Event {
        table_id: 1,
        kind: EventKind::Create(Schema {
            name: "items".into(),
            columns: vec![
                Column {
                    name: "id".into(),
                    data_type: DataType::Text,
                    nullable: false,
                },
                Column {
                    name: "value".into(),
                    data_type: DataType::Text,
                    nullable: false,
                },
            ],
            primary_key: 0,
        }),
    })
    .unwrap();
    for number in 0..10000 {
        base.apply(Event {
            table_id: 1,
            kind: EventKind::Insert(vec![
                Value::Text(format!("{number:08}{}", "x".repeat(248))),
                Value::Text("v".repeat(32)),
            ]),
        })
        .unwrap();
    }
    let expected = base.primary_index_info("items").unwrap();
    assert_eq!(expected.entries, 10000);
    assert_eq!(expected.pages, 768);
    let before = base.page_fingerprint();
    let key = Key::Text(format!("{:08}{}", 5000, "x".repeat(248)));
    let location = base.row_location("items", &key).unwrap();
    let profiler = dhat::Profiler::builder().testing().build();
    {
        let exported = base.export_primary_tree("items").unwrap();
        assert!(exported.has_stable_ids());
        assert_eq!(
            base.verify_primary_tree("items", &exported).unwrap(),
            expected
        );
        assert_eq!(exported.len(), 10000);
    }
    let stats = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!(stats.curr_bytes, 0);
    assert!(
        stats.max_bytes < 128 * 1024,
        "primary export peak {}",
        stats.max_bytes
    );
    assert_eq!(base.page_fingerprint(), before);
    assert_eq!(base.row_location("items", &key).unwrap(), location);
    eprintln!(
        "primary export requested peak={} total={} blocks={}",
        stats.max_bytes, stats.total_bytes, stats.total_blocks
    );
}
