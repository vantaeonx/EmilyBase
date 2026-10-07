#![cfg(feature = "heap-profile")]
use emilybase_catalog::{Column, DataType, Key, Schema};
#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;
#[test]
fn bounded_schema_and_key_validation_use_no_temporary_heap() {
    let mut totals = Vec::new();
    for count in [1, 2, 64] {
        let schema = Schema {
            name: "t".into(),
            columns: (0..count)
                .map(|i| Column {
                    name: format!("c{i:02}_{}", "x".repeat(50)),
                    data_type: DataType::Integer,
                    nullable: false,
                })
                .collect(),
            primary_key: 0,
        };
        let key = Key::Integer(7);
        let profiler = dhat::Profiler::builder().testing().build();
        for _ in 0..1000 {
            schema.validate().unwrap();
            schema.validate_key(&key).unwrap();
        }
        let observed = dhat::HeapStats::get();
        drop(profiler);
        eprintln!(
            "schema_{count} total={} blocks={} peak={} live={}",
            observed.total_bytes, observed.total_blocks, observed.max_bytes, observed.curr_bytes
        );
        totals.push(observed.total_bytes);
        assert_eq!(observed.curr_bytes, 0);
    }
    assert_eq!(totals, [0, 0, 0]);
}
