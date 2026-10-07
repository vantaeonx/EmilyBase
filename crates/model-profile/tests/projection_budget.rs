#![cfg(feature = "heap-profile")]
//! Warmed operation observations; complete fixture/cache construction is excluded.
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::query;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

fn fixture(left_count: i64, right_count: i64) -> Snapshot {
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
        for n in 0..if id == 1 { left_count } else { right_count } {
            snapshot
                .apply(Event {
                    table_id: id,
                    kind: EventKind::Insert(vec![
                        Value::Integer(n),
                        Value::Integer(n % right_count),
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
fn sorted_projection_refuses_shared_budget_before_cloning_all_result_payloads() {
    let snapshot = fixture(1500, 1500);
    let fallback = fixture(1000, 1);
    let digest = snapshot.page_fingerprint();
    let fallback_digest = fallback.page_fingerprint();
    let joined = std::iter::repeat_n("b.hidden", 32)
        .collect::<Vec<_>>()
        .join(",");
    let single = std::iter::repeat_n("hidden", 32)
        .collect::<Vec<_>>()
        .join(",");
    for (label, source, sql) in [
        (
            "sorted_primary_join",
            &snapshot,
            format!(
                "SELECT {joined} FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY b.id LIMIT 1000"
            ),
        ),
        (
            "single_table_sort",
            &snapshot,
            format!("SELECT {single} FROM l ORDER BY link LIMIT 1000"),
        ),
        (
            "streamed_primary_order",
            &snapshot,
            format!("SELECT {single} FROM l ORDER BY id LIMIT 1000"),
        ),
        (
            "fallback_join",
            &fallback,
            format!(
                "SELECT {joined} FROM l AS a JOIN r AS b ON a.link=b.id OR FALSE ORDER BY b.id LIMIT 1000"
            ),
        ),
    ] {
        let profiler = dhat::Profiler::builder().testing().build();
        let error = query(source, &sql, &[]).unwrap_err();
        let observed = dhat::HeapStats::get();
        assert!(matches!(
            error,
            emilybase_query::ExecutionError::Limit("output bytes")
        ));
        drop(error);
        let released = dhat::HeapStats::get();
        drop(profiler);
        eprintln!(
            "{label} refusal live={} peak={} released={}",
            observed.curr_bytes, observed.max_bytes, released.curr_bytes
        );
        assert_eq!(released.curr_bytes, 0);
        assert!(
            observed.max_bytes < 32 * 1024 * 1024,
            "projection cloned output before shared-budget refusal: {}",
            observed.max_bytes
        );
    }
    assert_eq!(snapshot.page_fingerprint(), digest);
    assert_eq!(fallback.page_fingerprint(), fallback_digest);
}
