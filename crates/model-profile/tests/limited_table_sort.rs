#![cfg(feature = "heap-profile")]
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::query;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;
fn fixture() -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "t".into(),
                columns: [
                    ("rank", DataType::Integer),
                    ("id", DataType::Integer),
                    ("hidden", DataType::Text),
                ]
                .into_iter()
                .map(|(name, data_type)| Column {
                    name: name.into(),
                    data_type,
                    nullable: false,
                })
                .collect(),
                primary_key: 1,
            }),
        })
        .unwrap();
    for id in 0..1500 {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![
                    Value::Integer(1499 - id),
                    Value::Integer(id),
                    Value::Text("x".repeat(3072)),
                ]),
            })
            .unwrap();
    }
    snapshot.primary_index_info("t").unwrap();
    snapshot
}
#[test]
fn ordinary_small_sort_limit_does_not_copy_the_whole_wide_source() {
    let snapshot = fixture();
    let digest = snapshot.page_fingerprint();
    for (label, sql, expected) in [
        (
            "table_sort",
            "SELECT id FROM t ORDER BY rank LIMIT 2",
            vec![vec![Value::Integer(1499)], vec![Value::Integer(1498)]],
        ),
        (
            "table_range",
            "SELECT id FROM t WHERE id>=1000 AND id<1400 ORDER BY rank LIMIT 2",
            vec![vec![Value::Integer(1399)], vec![Value::Integer(1398)]],
        ),
        (
            "table_point",
            "SELECT id FROM t WHERE id=1390 ORDER BY rank LIMIT 2",
            vec![vec![Value::Integer(1390)]],
        ),
    ] {
        let profiler = dhat::Profiler::builder().testing().build();
        let result = query(&snapshot, sql, &[]).unwrap();
        let observed = dhat::HeapStats::get();
        assert_eq!(result.rows, expected);
        drop(result);
        let released = dhat::HeapStats::get();
        drop(profiler);
        eprintln!(
            "{label} live={} peak={} released={}",
            observed.curr_bytes, observed.max_bytes, released.curr_bytes
        );
        assert_eq!(released.curr_bytes, 0);
        assert!(
            observed.max_bytes < 128 * 1024,
            "table sort copied full source: {}",
            observed.max_bytes
        );
    }
    assert_eq!(snapshot.page_fingerprint(), digest);
}
