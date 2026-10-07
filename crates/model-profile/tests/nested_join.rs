#![cfg(feature = "heap-profile")]
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::query;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

fn fixture() -> Snapshot {
    let mut view = Snapshot::empty().unwrap();
    for (table_id, name) in [(1, "a"), (2, "b")] {
        view.apply(Event {
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
        for id in 0..100 {
            view.apply(Event {
                table_id,
                kind: EventKind::Insert(vec![
                    Value::Integer(id),
                    Value::Integer(id),
                    Value::Text("λ".repeat(1500)),
                ]),
            })
            .unwrap();
        }
        view.primary_index_info(name).unwrap();
    }
    view
}

#[test]
fn nested_join_rejects_borrowed_pairs_without_copying_hidden_sources() {
    let snapshot = fixture();
    let before = snapshot.page_fingerprint();
    let mut totals = Vec::new();
    for (label, sql, expected) in [
        (
            "false_on",
            "SELECT a.id FROM a JOIN b ON a.rank=b.rank AND b.rank<0 LIMIT 2",
            vec![],
        ),
        (
            "false_where",
            "SELECT a.id FROM a JOIN b ON a.rank=b.rank WHERE b.rank<0 LIMIT 2",
            vec![],
        ),
        (
            "accepted",
            "SELECT a.id FROM a JOIN b ON a.rank=b.rank ORDER BY b.rank LIMIT 2",
            vec![vec![Value::Integer(0)], vec![Value::Integer(1)]],
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
            "{label} total={} blocks={} peak={} live={} released={}",
            observed.total_bytes,
            observed.total_blocks,
            observed.max_bytes,
            observed.curr_bytes,
            released.curr_bytes
        );
        assert_eq!(released.curr_bytes, 0);
        totals.push(observed.total_bytes);
    }
    assert_eq!(snapshot.page_fingerprint(), before);
    assert!(
        totals.iter().all(|bytes| *bytes < 2 * 1024 * 1024),
        "{totals:?}"
    );
}
