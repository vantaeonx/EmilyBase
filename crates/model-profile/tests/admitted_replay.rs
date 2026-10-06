#![cfg(feature = "heap-profile")]
//! Complete retained models/source/decoded plan/pool precede every sample.
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_commit_model::{
    AdmissionLimit, AdmissionLimits, DecodedPlanLimits, DecodedPlanPool, Error, Model, ModelPool,
};
use emilybase_database::{Event, EventKind};

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[test]
fn destination_slot_and_writer_refusals_allocate_no_replay_projection() {
    let pool = ModelPool::new(AdmissionLimits::new(1, 2, 1, 1).unwrap());
    let mut project = pool.create([7; 16]).unwrap();
    let old = project.read().unwrap();
    let mut base = Model::new([7; 16]).unwrap();
    let mut stage = base.begin().unwrap();
    stage
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
                        name: "value".into(),
                        data_type: DataType::Text,
                        nullable: false,
                    },
                ],
            }),
        })
        .unwrap();
    stage.rebuild_index("items").unwrap();
    let prepared = stage.prepare().unwrap();
    let decoder = DecodedPlanPool::new(DecodedPlanLimits::new(1, 2 * 1024 * 1024).unwrap());
    let input = decoder
        .decode(&prepared.image_plan().unwrap().encode().unwrap())
        .unwrap();
    project
        .publish_replayed(project.replay(&input).unwrap())
        .unwrap();
    base.publish(prepared).unwrap();
    drop(input);
    let mut stage = base.begin().unwrap();
    for key in 0..256 {
        stage
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Integer(key), Value::Text("v".repeat(3072))]),
            })
            .unwrap();
    }
    stage.rebuild_index("items").unwrap();
    let prepared = stage.prepare().unwrap();
    let input = decoder
        .decode(&prepared.image_plan().unwrap().encode().unwrap())
        .unwrap();
    let profiler = dhat::Profiler::builder().testing().build();
    assert!(matches!(
        project.replay(&input),
        Err(Error::Admission(AdmissionLimit::Generations))
    ));
    let generations = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!((generations.curr_bytes, generations.total_bytes), (0, 0));
    assert_eq!(pool.usage().unwrap().writers, 0);
    drop(old);
    let stage = project.begin().unwrap();
    let profiler = dhat::Profiler::builder().testing().build();
    assert!(matches!(
        project.replay(&input),
        Err(Error::Admission(AdmissionLimit::ProjectWriter))
    ));
    let writer = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!((writer.curr_bytes, writer.total_bytes), (0, 0));
    drop(stage);
    let profiler = dhat::Profiler::builder().testing().build();
    let replayed = project.replay(&input).unwrap();
    assert_eq!(replayed.row_count(), 256);
    let live = dhat::HeapStats::get();
    assert!(live.curr_bytes > 256 * 3072);
    assert_eq!(pool.usage().unwrap().generations, 2);
    drop(replayed);
    let released = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!(released.curr_bytes, 0);
    assert_eq!(pool.usage().unwrap().generations, 1);
    assert_eq!(pool.usage().unwrap().writers, 0);
    eprintln!(
        "replay generation_refusal={} writer_refusal={} live={} peak={} released={}",
        generations.total_bytes,
        writer.total_bytes,
        live.curr_bytes,
        live.max_bytes,
        released.curr_bytes
    );
}
