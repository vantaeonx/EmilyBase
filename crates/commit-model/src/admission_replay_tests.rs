use super::*;
use crate::{DecodedPlanLimits, DecodedPlanPool};
use emilybase_catalog::{Column, DataType};
use emilybase_database::EventKind;

fn fixture() -> (ModelPool, ModelProject, AdmittedPlan) {
    let pool = ModelPool::new(AdmissionLimits::new(1, 2, 1, 1).unwrap());
    let project = pool.create([7; 16]).unwrap();
    let raw = Model::new([7; 16]).unwrap();
    let mut stage = raw.begin().unwrap();
    stage
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "items".into(),
                primary_key: 0,
                columns: vec![Column {
                    name: "id".into(),
                    data_type: DataType::Integer,
                    nullable: false,
                }],
            }),
        })
        .unwrap();
    stage.rebuild_index("items").unwrap();
    let bytes = stage
        .prepare()
        .unwrap()
        .image_plan()
        .unwrap()
        .encode()
        .unwrap();
    let decoded = DecodedPlanPool::new(DecodedPlanLimits::new(1, 64 * 1024).unwrap());
    (pool, project, decoded.decode(&bytes).unwrap())
}

#[test]
fn reconstructed_storage_and_base_lifetimes_match_generation_ownership() {
    let (pool, mut project, input) = fixture();
    let old = project.read().unwrap();
    let base = Arc::downgrade(&project.current.model.state);
    let output = project.replay(&input).unwrap();
    let state = Arc::downgrade(&output.model.state);
    assert!(!Arc::ptr_eq(&output.model.state, &output.base.model.state));
    assert!(Arc::ptr_eq(&output.base, &project.current));
    project.publish_replayed(output).unwrap();
    assert!(state.upgrade().is_some());
    assert!(base.upgrade().is_some());
    assert_eq!(pool.usage().unwrap().generations, 2);
    drop(old);
    assert!(base.upgrade().is_none());
    assert!(state.upgrade().is_some());
    assert_eq!(pool.usage().unwrap().generations, 1);
    drop(project);
    assert!(state.upgrade().is_none());
    assert_eq!(pool.usage().unwrap().generations, 0);
}

#[test]
fn discarded_replay_frees_actual_next_state_and_owner_after_last_descendant() {
    let (pool, project, input) = fixture();
    let output = project.replay(&input).unwrap();
    let next = Arc::downgrade(&output.model.state);
    let base = Arc::downgrade(&output.base.model.state);
    let owner = Arc::downgrade(&output.generation.owner);
    drop(project);
    drop(input);
    assert!(next.upgrade().is_some());
    assert!(base.upgrade().is_some());
    assert!(owner.upgrade().is_some());
    drop(output);
    assert!(next.upgrade().is_none());
    assert!(base.upgrade().is_none());
    assert!(owner.upgrade().is_none());
    let usage = pool.usage().unwrap();
    assert_eq!(
        (usage.projects, usage.generations, usage.writers),
        (0, 0, 0)
    );
}

#[test]
fn unwind_discards_pending_output_without_publishing_or_leaking_leases() {
    let (pool, project, input) = fixture();
    let before = project.fingerprint();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let output = project.replay(&input).unwrap();
        assert_eq!(output.transaction(), 2);
        panic!("synthetic caller unwind");
    }));
    assert!(result.is_err());
    assert_eq!(project.fingerprint(), before);
    assert_eq!(pool.usage().unwrap().writers, 0);
    assert_eq!(pool.usage().unwrap().generations, 1);
    assert!(project.replay(&input).is_ok());
}

#[test]
fn poisoned_admission_refuses_new_replay_and_retained_output_still_cleans_up() {
    let (pool, project, input) = fixture();
    let output = project.replay(&input).unwrap();
    let state = Arc::downgrade(&output.model.state);
    let shared = Arc::clone(&pool.shared);
    assert!(
        std::thread::spawn(move || {
            let _guard = shared.ledger.lock().unwrap();
            panic!("synthetic internal poison");
        })
        .join()
        .is_err()
    );
    assert!(matches!(
        project.replay(&input),
        Err(Error::AdmissionPoisoned)
    ));
    assert_eq!(output.row_count(), 0);
    drop(output);
    drop(project);
    assert!(state.upgrade().is_none());
    let ledger = pool.shared.ledger.lock().err().unwrap().into_inner();
    assert!(ledger.projects.is_empty());
    assert!(ledger.writers.is_empty());
    assert_eq!(ledger.generations, 0);
}
