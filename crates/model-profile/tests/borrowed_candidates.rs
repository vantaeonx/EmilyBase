#![cfg(feature = "heap-profile")]
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::query;
#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;
fn fixture() -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    for (table_id, name) in [(1, "a"), (2, "b")] {
        snapshot
            .apply(Event {
                table_id,
                kind: EventKind::Create(Schema {
                    name: name.into(),
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
                    table_id,
                    kind: EventKind::Insert(vec![
                        Value::Integer(id),
                        Value::Integer(id),
                        Value::Text("x".repeat(3072)),
                    ]),
                })
                .unwrap();
        }
        snapshot.primary_index_info(name).unwrap();
    }
    snapshot
}
#[test]
fn discarded_sorted_and_filtered_join_candidates_do_not_clone_hidden_payloads() {
    let snapshot = fixture();
    let digest = snapshot.page_fingerprint();
    let mut totals = Vec::new();
    for (label, sql, expected) in [
        (
            "discarded_table",
            "SELECT id FROM a ORDER BY rank LIMIT 2",
            vec![vec![Value::Integer(0)], vec![Value::Integer(1)]],
        ),
        (
            "discarded_join",
            "SELECT a.id FROM a JOIN b ON a.id=b.id ORDER BY b.rank LIMIT 2",
            vec![vec![Value::Integer(0)], vec![Value::Integer(1)]],
        ),
        (
            "filtered_join",
            "SELECT a.id FROM a JOIN b ON a.id=b.id AND b.rank<0 LIMIT 2",
            vec![],
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
            "{label} live={} peak={} total={} blocks={} released={}",
            observed.curr_bytes,
            observed.max_bytes,
            observed.total_bytes,
            observed.total_blocks,
            released.curr_bytes
        );
        assert_eq!(released.curr_bytes, 0);
        totals.push((label, observed.total_bytes));
    }
    assert_eq!(snapshot.page_fingerprint(), digest);
    for (label, total) in totals {
        let bound = if label == "discarded_table" { 6 } else { 12 } * 1024 * 1024;
        assert!(total < bound, "hidden {label} candidates copied: {total}");
    }
}
