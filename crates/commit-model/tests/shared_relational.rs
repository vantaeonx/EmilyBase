use emilybase_catalog::{DataType, Key, Value};
use emilybase_commit_model::Error;
use emilybase_database::{Event, EventKind};
mod support;

fn populated() -> emilybase_commit_model::Model {
    let mut model = support::model(&[
        ("changed", DataType::Integer),
        ("untouched", DataType::Text),
    ]);
    let mut staged = model.begin().unwrap();
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(vec![Value::Integer(0), Value::Text("base".into())]),
        })
        .unwrap();
    staged
        .apply(Event {
            table_id: 2,
            kind: EventKind::Insert(vec![
                Value::Text("я".repeat(1536)),
                Value::Text("u".repeat(768)),
            ]),
        })
        .unwrap();
    support::indexes(&model, &mut staged, &["changed", "untouched"]);
    model.publish(staged.prepare().unwrap()).unwrap();
    model
}

#[test]
fn index_only_stages_publish_new_roots_with_the_same_relational_rows_and_locations() {
    let mut model = populated();
    let old = model.clone();
    let key = Key::Integer(0);
    let location = old.view().row_location("changed", &key).unwrap();
    let page_digest = old.view().page_fingerprint();
    let components = old.encoded_components().unwrap();
    let mut staged = model.begin().unwrap();
    assert!(std::ptr::eq(
        old.view().get("changed", &key).unwrap().unwrap(),
        staged
            .view()
            .unwrap()
            .get("changed", &key)
            .unwrap()
            .unwrap()
    ));
    support::indexes(&model, &mut staged, &["changed"]);
    let prepared = staged.prepare().unwrap();
    assert!(std::ptr::eq(
        old.view().get("changed", &key).unwrap().unwrap(),
        prepared.view().get("changed", &key).unwrap().unwrap()
    ));
    assert_eq!(prepared.view().page_fingerprint(), page_digest);
    assert_eq!(prepared.encoded_components().unwrap(), components);
    assert_eq!(
        prepared.view().row_location("changed", &key).unwrap(),
        location
    );
    model.publish(prepared).unwrap();
    assert_ne!(model.fingerprint(), old.fingerprint());
    assert_eq!(
        model.selection(1).unwrap().binding().revision(),
        old.selection(1).unwrap().binding().revision() + 1
    );
    assert_eq!(model.selection(2), old.selection(2));
    assert!(std::ptr::eq(
        old.view().get("changed", &key).unwrap().unwrap(),
        model.view().get("changed", &key).unwrap().unwrap()
    ));
}

#[test]
fn mutation_detaches_only_its_table_through_prepare_and_memory_publication() {
    let mut model = populated();
    let old = model.clone();
    let integer = Key::Integer(0);
    let long = Key::Text("я".repeat(1536));
    let untouched = old.view().get("untouched", &long).unwrap().unwrap();
    let original = old.view().get("changed", &integer).unwrap().unwrap();
    let mut staged = model.begin().unwrap();
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Replace(vec![Value::Integer(0), Value::Text("new".into())]),
        })
        .unwrap();
    assert!(!std::ptr::eq(
        original,
        staged
            .view()
            .unwrap()
            .get("changed", &integer)
            .unwrap()
            .unwrap()
    ));
    assert!(std::ptr::eq(
        untouched,
        staged
            .view()
            .unwrap()
            .get("untouched", &long)
            .unwrap()
            .unwrap()
    ));
    support::indexes(&model, &mut staged, &["changed"]);
    let prepared = staged.prepare().unwrap();
    assert!(std::ptr::eq(
        untouched,
        prepared.view().get("untouched", &long).unwrap().unwrap()
    ));
    model.publish(prepared).unwrap();
    assert!(std::ptr::eq(
        untouched,
        model.view().get("untouched", &long).unwrap().unwrap()
    ));
    assert_eq!(
        old.view().get("changed", &integer).unwrap().unwrap()[1],
        Value::Text("base".into())
    );
    assert_eq!(
        model.view().get("changed", &integer).unwrap().unwrap()[1],
        Value::Text("new".into())
    );
}

#[test]
fn stale_shared_index_only_candidate_cannot_publish_over_a_new_table_write() {
    let mut model = populated();
    let old = model.clone();
    let mut index_only = model.begin().unwrap();
    support::indexes(&model, &mut index_only, &["changed"]);
    let stale = index_only.prepare().unwrap();
    let mut written = model.begin().unwrap();
    written
        .apply(Event {
            table_id: 1,
            kind: EventKind::Replace(vec![Value::Integer(0), Value::Text("new".into())]),
        })
        .unwrap();
    support::indexes(&model, &mut written, &["changed"]);
    model.publish(written.prepare().unwrap()).unwrap();
    let new = model.fingerprint();
    assert!(matches!(model.publish(stale), Err(Error::Conflict)));
    assert_eq!(model.fingerprint(), new);
    assert_eq!(
        model
            .view()
            .get("changed", &Key::Integer(0))
            .unwrap()
            .unwrap()[1],
        Value::Text("new".into())
    );
    assert_eq!(
        old.view()
            .get("changed", &Key::Integer(0))
            .unwrap()
            .unwrap()[1],
        Value::Text("base".into())
    );
}
