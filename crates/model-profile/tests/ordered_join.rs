#![cfg(feature = "heap-profile")]
//! Warmed operation observations; complete fixture/cache construction is excluded.
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::query;

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
                    columns: [
                        ("id", DataType::Integer),
                        ("link", DataType::Integer),
                        ("hidden", DataType::Text),
                    ]
                    .into_iter()
                    .map(|(name, data_type)| Column {
                        name: name.into(),
                        data_type,
                        nullable: false,
                    })
                    .collect(),
                    primary_key: 0,
                }),
            })
            .unwrap();
        for n in 0..1500 {
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
fn ordered_point_and_range_primary_joins_do_not_materialize_all_wide_matches() {
    let snapshot = fixture();
    let old = snapshot.clone();
    let digest = old.page_fingerprint();
    for (label, sql, expected) in [
        (
            "ordered",
            "SELECT a.id,b.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY a.id DESC LIMIT 2",
            vec![vec![Value::Integer(1499); 2], vec![Value::Integer(1498); 2]],
        ),
        (
            "range",
            "SELECT a.id,b.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE a.id>=1000 AND a.id<1400 ORDER BY a.id DESC LIMIT 2",
            vec![vec![Value::Integer(1399); 2], vec![Value::Integer(1398); 2]],
        ),
        (
            "point",
            "SELECT a.id,b.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE a.id=1390 ORDER BY a.id DESC LIMIT 2",
            vec![vec![Value::Integer(1390); 2]],
        ),
    ] {
        let profiler = dhat::Profiler::builder().testing().build();
        let result = query(&snapshot, sql, &[]).unwrap();
        let observed = dhat::HeapStats::get();
        assert_eq!(result.rows, expected);
        assert!(
            observed.max_bytes < 128 * 1024,
            "ordered join retained full matches: {}",
            observed.max_bytes
        );
        drop(result);
        let released = dhat::HeapStats::get();
        drop(profiler);
        assert_eq!(released.curr_bytes, 0);
        eprintln!(
            "{label} join live={} peak={} released={}",
            observed.curr_bytes, observed.max_bytes, released.curr_bytes
        );
    }
    assert_eq!(snapshot.page_fingerprint(), digest);
    assert_eq!(old.page_fingerprint(), digest);
}
