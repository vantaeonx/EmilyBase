#![cfg(feature = "heap-profile")]
use emilybase_catalog::{Column, DataType, Schema};
use emilybase_commit_model::Model;
use emilybase_database::{Event, EventKind};

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

fn wide_model(count: usize) -> Model {
    let mut model = Model::new([21; 16]).unwrap();
    let mut staged = model.begin().unwrap();
    for id in 0..count {
        let name = format!("t{id:03}_{}", "n".repeat(48));
        let schema = Schema {
            name: name.clone(),
            columns: (0..64)
                .map(|column| Column {
                    name: format!("c{column:02}_{}", "x".repeat(50)),
                    data_type: DataType::Integer,
                    nullable: false,
                })
                .collect(),
            primary_key: 0,
        };
        staged
            .apply(Event {
                table_id: staged.view().unwrap().next_table_id(),
                kind: EventKind::Create(schema),
            })
            .unwrap();
        staged.rebuild_index(&name).unwrap();
    }
    model.publish(staged.prepare().unwrap()).unwrap();
    model
}

#[test]
fn complete_wide_model_prepare_does_not_clone_every_schema() {
    let mut peaks = Vec::new();
    for count in [1, 64, 128] {
        let model = wide_model(count);
        let before = model.fingerprint();
        let mut staged = model.begin().unwrap();
        let name = format!("t000_{}", "n".repeat(48));
        staged.rebuild_index(&name).unwrap();
        let profiler = dhat::Profiler::builder().testing().build();
        let prepared = staged.prepare().unwrap();
        assert_eq!(prepared.view().row_count(), 0);
        assert_eq!(prepared.selection(1).unwrap().binding().covered(), 0);
        let observed = dhat::HeapStats::get();
        drop(prepared);
        let released = dhat::HeapStats::get();
        drop(profiler);
        eprintln!(
            "prepare_{count} total={} blocks={} peak={} live={} released={}",
            observed.total_bytes,
            observed.total_blocks,
            observed.max_bytes,
            observed.curr_bytes,
            released.curr_bytes
        );
        assert_eq!(model.fingerprint(), before);
        assert_eq!(released.curr_bytes, 0);
        peaks.push(observed.max_bytes);
    }
    assert!(peaks.iter().all(|peak| *peak < 128 * 1024), "{peaks:?}");
    // Keep all samples in one test: the process allocator also observes parallel
    // test-harness teardown allocations, even if fixture work uses a mutex.
    complete_borrowed_inventory_has_zero_allocations_during_iteration_and_counting();
}

fn complete_borrowed_inventory_has_zero_allocations_during_iteration_and_counting() {
    let model = wide_model(128);
    let view = model.view();
    let profiler = dhat::Profiler::builder().testing().build();
    let mut sum = 0usize;
    for _ in 0..1000 {
        assert_eq!(view.table_count(), 128);
        let mut schemas = view.schema_refs();
        assert_eq!(schemas.len(), 128);
        for schema in schemas.by_ref() {
            sum += std::hint::black_box(schema.columns[63].name.len());
        }
        assert!(schemas.next_back().is_none());
    }
    let observed = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!(sum, 128 * 1000 * 54);
    assert_eq!(observed.total_bytes, 0);
    assert_eq!(observed.total_blocks, 0);
    assert_eq!(observed.max_bytes, 0);
    assert_eq!(observed.curr_bytes, 0);
}
