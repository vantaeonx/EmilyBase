#![cfg(feature = "heap-profile")]
//! Retained fixtures/source/pools are outside each isolated operation sample.
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_commit_model::{
    DecodedPlanLimits, DecodedPlanPool, DecodedPlanUsage, MAX_DECODED_PLAN_BYTES, Model,
};
use emilybase_database::{Event, EventKind};

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[test]
fn denied_owned_decode_has_bounded_scratch_and_shared_handles_keep_one_payload() {
    let mut base = Model::new([7; 16]).unwrap();
    let mut staged = base.begin().unwrap();
    staged
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
    staged.rebuild_index("items").unwrap();
    base.publish(staged.prepare().unwrap()).unwrap();
    let mut staged = base.begin().unwrap();
    for number in 0..256 {
        staged
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![
                    Value::Integer(number),
                    Value::Text("v".repeat(3072)),
                ]),
            })
            .unwrap();
    }
    staged.rebuild_index("items").unwrap();
    let prepared = staged.prepare().unwrap();
    let plan = prepared.image_plan().unwrap();
    assert_eq!(plan.counts().unwrap().history_pages(), 256);
    let payload = plan.counts().unwrap().decoded_vector_bytes().unwrap();
    assert!(payload > 1024 * 1024);
    let source = plan.encode().unwrap();
    let denied = DecodedPlanPool::new(DecodedPlanLimits::new(0, MAX_DECODED_PLAN_BYTES).unwrap());
    let accepted = DecodedPlanPool::new(DecodedPlanLimits::new(1, payload).unwrap());
    let profiler = dhat::Profiler::builder().testing().build();
    assert!(denied.decode(&source).is_err());
    let rejected = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!(rejected.curr_bytes, 0);
    assert!(
        rejected.max_bytes < 128 * 1024,
        "refused decode peak {}",
        rejected.max_bytes
    );
    assert_eq!(denied.usage().unwrap(), DecodedPlanUsage::default());
    let profiler = dhat::Profiler::builder().testing().build();
    let owner = accepted.decode(&source).unwrap();
    let live = dhat::HeapStats::get();
    assert_eq!(
        accepted.usage().unwrap(),
        DecodedPlanUsage {
            plans: 1,
            bytes: payload
        }
    );
    let requested = usize::try_from(payload).unwrap();
    assert!(live.curr_bytes >= requested);
    assert!(live.curr_bytes < requested + 16 * 1024);
    assert!(live.max_bytes < requested + 128 * 1024);
    let clones: [_; 4] = std::array::from_fn(|_| owner.clone());
    let shared = dhat::HeapStats::get();
    assert_eq!(shared.curr_bytes, live.curr_bytes);
    assert_eq!(shared.total_bytes, live.total_bytes);
    drop(owner);
    assert_eq!(accepted.usage().unwrap().plans, 1);
    drop(clones);
    let released = dhat::HeapStats::get();
    drop(profiler);
    assert_eq!(released.curr_bytes, 0);
    assert_eq!(accepted.usage().unwrap(), DecodedPlanUsage::default());
    eprintln!(
        "decoded payload={} refused_peak={} live={} accepted_peak={} released={}",
        payload, rejected.max_bytes, live.curr_bytes, live.max_bytes, released.curr_bytes
    );
}
