#![cfg(feature = "heap-profile")]
//! Isolated warmed read samples; complete synthetic fixture/cache construction
//! precedes profiling and remains outside these operation-local observations.
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::{explain, query};

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

fn fixture() -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    for (id, name) in [(1, "l"), (2, "r")] {
        snapshot
            .apply(Event {
                table_id: id,
                kind: EventKind::Create(Schema {
                    name: name.into(),
                    columns: vec![
                        Column {
                            name: "id".into(),
                            data_type: DataType::Integer,
                            nullable: false,
                        },
                        Column {
                            name: "link".into(),
                            data_type: DataType::Integer,
                            nullable: false,
                        },
                        Column {
                            name: "hidden".into(),
                            data_type: DataType::Text,
                            nullable: false,
                        },
                    ],
                    primary_key: 0,
                }),
            })
            .unwrap();
        for n in 0..4000 {
            snapshot
                .apply(Event {
                    table_id: id,
                    kind: EventKind::Insert(vec![
                        Value::Integer(n),
                        Value::Integer(n),
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
fn unique_primary_join_avoids_cloning_complete_wide_source_tables() {
    let snapshot = fixture();
    let old = snapshot.clone();
    let digest = old.page_fingerprint();
    let fast = "SELECT a.id,b.id FROM l AS a JOIN r AS b ON a.link=b.id LIMIT 2";
    let slow = "SELECT a.id,b.id FROM l AS a JOIN r AS b ON a.link=b.id OR FALSE LIMIT 2";
    assert_eq!(
        explain(&snapshot, fast, &[]).unwrap().access,
        "primary_join"
    );
    assert_eq!(
        explain(&snapshot, slow, &[]).unwrap().access,
        "bounded_nested_loop"
    );
    let expected = vec![vec![Value::Integer(0); 2], vec![Value::Integer(1); 2]];

    let profiler = dhat::Profiler::builder().testing().build();
    let result = query(&snapshot, slow, &[]).unwrap();
    let baseline = dhat::HeapStats::get();
    assert_eq!(result.rows, expected);
    assert!(baseline.max_bytes > 16 * 1024 * 1024);
    drop(result);
    let baseline_released = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!(baseline_released.curr_bytes, 0);

    let profiler = dhat::Profiler::builder().testing().build();
    let result = query(&snapshot, fast, &[]).unwrap();
    let streamed = dhat::HeapStats::get();
    assert_eq!(result.rows, expected);
    assert!(
        streamed.max_bytes < 128 * 1024,
        "join cloned source payload: {}",
        streamed.max_bytes
    );
    drop(result);
    let streamed_released = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!(streamed_released.curr_bytes, 0);
    assert_eq!(old.page_fingerprint(), digest);
    assert_eq!(snapshot.page_fingerprint(), digest);
    eprintln!(
        "nested join live={} peak={} released={}; primary join live={} peak={} released={}",
        baseline.curr_bytes,
        baseline.max_bytes,
        baseline_released.curr_bytes,
        streamed.curr_bytes,
        streamed.max_bytes,
        streamed_released.curr_bytes
    );
}
