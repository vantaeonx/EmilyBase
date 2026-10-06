use emilybase_catalog::{DataType, Key, Value};
use emilybase_commit_model::{Error, ImagePlan, Model};
use emilybase_database::{Event, EventKind};
use emilybase_index::RecordPointer;
mod support;

fn populated() -> Model {
    let mut model = support::model(&[("items", DataType::Integer)]);
    let mut staged = model.begin().unwrap();
    for number in 0..120 {
        staged
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Integer(number), Value::Text("v".repeat(256))]),
            })
            .unwrap();
    }
    support::indexes(&model, &mut staged, &["items"]);
    model.publish(staged.prepare().unwrap()).unwrap();
    model
}

fn borrowed_key(model: &Model, number: i64) -> &Key {
    let selection = model.selection(1).unwrap();
    let key = Key::Integer(number);
    let (borrowed, _) = selection
        .index()
        .tree
        .cursor(Some(&key), None)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(borrowed, &key);
    borrowed
}

fn shared_rebuild(base: &Model) -> emilybase_commit_model::Prepared {
    let mut staged = base.begin().unwrap();
    let candidate = base.selection(1).unwrap().index().tree.clone();
    let (binding, index) = support::candidate(base, &staged, "items", candidate);
    staged.index(binding, index).unwrap();
    staged.prepare().unwrap()
}

#[test]
fn root_only_rebuild_and_encoded_replay_share_complete_index_pages() {
    let mut model = populated();
    let old = model.clone();
    let old_bytes = old.selection(1).unwrap().index().encode().unwrap();
    let prepared = shared_rebuild(&model);
    for number in [0, 50, 119] {
        let bound = Key::Integer(number);
        let prepared_key = prepared
            .selection(1)
            .unwrap()
            .index()
            .tree
            .cursor(Some(&bound), None)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .0;
        assert!(std::ptr::eq(prepared_key, borrowed_key(&old, number)));
    }
    let plan = prepared.image_plan().unwrap();
    let counts = plan.counts().unwrap();
    assert_eq!(counts.changed_roots(), 1);
    assert_eq!(counts.primary_pages(), 0);
    assert_eq!(counts.history_pages(), 0);
    let encoded = plan.encode().unwrap();
    assert_eq!(encoded.len(), 424);
    let decoded = ImagePlan::decode(&encoded).unwrap();
    let replayed = decoded.replay(&old).unwrap();
    assert_eq!(replayed.fingerprint(), plan.next_fingerprint());
    for number in [0, 50, 119] {
        assert!(std::ptr::eq(
            borrowed_key(&old, number),
            borrowed_key(&replayed, number)
        ));
    }
    model.publish(prepared).unwrap();
    assert_eq!(model.fingerprint(), replayed.fingerprint());
    assert_eq!(model.view().row_count(), 120);
    assert_eq!(
        old.selection(1).unwrap().index().encode().unwrap(),
        old_bytes
    );
    drop(old);
    drop(decoded);
    drop(plan);
    drop(replayed);
    assert_eq!(
        model
            .view()
            .get("items", &Key::Integer(50))
            .unwrap()
            .unwrap()[1],
        Value::Text("v".repeat(256))
    );
    assert_eq!(
        model.selection(1).unwrap().index().tree.validate().unwrap(),
        120
    );
}

#[test]
fn row_pointer_update_shares_untouched_leaf_through_prepare_and_replay() {
    let mut model = populated();
    let old = model.clone();
    let old_pointer = old
        .selection(1)
        .unwrap()
        .index()
        .tree
        .get(&Key::Integer(0))
        .unwrap();
    let original = old.selection(1).unwrap().index().encode().unwrap();
    let mut staged = model.begin().unwrap();
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Replace(vec![Value::Integer(0), Value::Text("changed".into())]),
        })
        .unwrap();
    let location = staged
        .view()
        .unwrap()
        .row_location("items", &Key::Integer(0))
        .unwrap()
        .unwrap();
    let mut candidate = old.selection(1).unwrap().index().tree.clone();
    candidate
        .replace(
            &Key::Integer(0),
            RecordPointer {
                page_id: location.page_id,
                slot_id: location.slot_id,
            },
        )
        .unwrap();
    let (binding, index) = support::candidate(&old, &staged, "items", candidate);
    staged.index(binding, index).unwrap();
    let prepared = staged.prepare().unwrap();
    let plan = prepared.image_plan().unwrap();
    assert_eq!(plan.counts().unwrap().primary_pages(), 1);
    let replayed = ImagePlan::decode(&plan.encode().unwrap())
        .unwrap()
        .replay(&old)
        .unwrap();
    assert!(!std::ptr::eq(
        borrowed_key(&old, 0),
        borrowed_key(&replayed, 0)
    ));
    assert!(std::ptr::eq(
        borrowed_key(&old, 119),
        borrowed_key(&replayed, 119)
    ));
    model.publish(prepared).unwrap();
    assert_eq!(model.fingerprint(), replayed.fingerprint());
    assert!(std::ptr::eq(
        borrowed_key(&old, 119),
        borrowed_key(&model, 119)
    ));
    assert_eq!(
        old.selection(1)
            .unwrap()
            .index()
            .tree
            .get(&Key::Integer(0))
            .unwrap(),
        old_pointer
    );
    assert_eq!(
        old.selection(1).unwrap().index().encode().unwrap(),
        original
    );
    assert_eq!(
        old.view().get("items", &Key::Integer(0)).unwrap().unwrap()[1],
        Value::Text("v".repeat(256))
    );
    assert_eq!(
        model
            .view()
            .get("items", &Key::Integer(0))
            .unwrap()
            .unwrap()[1],
        Value::Text("changed".into())
    );
}

#[test]
fn stale_shared_candidate_is_refused_without_replacing_the_selected_arena() {
    let mut model = populated();
    let old = model.clone();
    let stale = shared_rebuild(&old);
    let stale_plan = stale.image_plan().unwrap();
    model.publish(shared_rebuild(&old)).unwrap();
    let current = model.clone();
    let fingerprint = model.fingerprint();
    assert!(matches!(model.publish(stale), Err(Error::Conflict)));
    assert!(stale_plan.replay(&model).is_err());
    assert_eq!(model.fingerprint(), fingerprint);
    for number in [0, 50, 119] {
        assert!(std::ptr::eq(
            borrowed_key(&model, number),
            borrowed_key(&current, number)
        ));
    }
    assert_eq!(old.view().row_count(), 120);
    assert_eq!(
        old.selection(1).unwrap().index().tree.validate().unwrap(),
        120
    );
}

#[test]
fn parallel_independent_replay_retains_shared_index_after_source_and_plan_release() {
    let model = populated();
    let prepared = shared_rebuild(&model);
    let plan = prepared.image_plan().unwrap();
    let expected = plan.next_fingerprint();
    let gate = std::sync::Barrier::new(5);
    let outputs = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let gate = &gate;
                let base = &model;
                let plan = &plan;
                scope.spawn(move || {
                    gate.wait();
                    let replayed = plan.replay(base).unwrap();
                    assert_eq!(replayed.fingerprint(), expected);
                    assert!(std::ptr::eq(
                        borrowed_key(base, 119),
                        borrowed_key(&replayed, 119)
                    ));
                    replayed
                })
            })
            .collect();
        gate.wait();
        workers
            .into_iter()
            .map(|w| w.join().unwrap())
            .collect::<Vec<_>>()
    });
    drop(prepared);
    drop(plan);
    drop(model);
    for replayed in outputs {
        assert_eq!(replayed.fingerprint(), expected);
        assert_eq!(
            replayed
                .selection(1)
                .unwrap()
                .index()
                .tree
                .validate()
                .unwrap(),
            120
        );
        let location = replayed
            .view()
            .row_location("items", &Key::Integer(119))
            .unwrap()
            .unwrap();
        assert_eq!(
            replayed
                .view()
                .resolve_row_location("items", &Key::Integer(119), location)
                .unwrap()[1],
            Value::Text("v".repeat(256))
        );
    }
}
