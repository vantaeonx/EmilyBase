use emilybase_catalog::{DataType, Key, Value};
use emilybase_commit_model::{Error, MAX_EVENTS};
use emilybase_database::{Event, EventKind, MAX_ROWS};
use emilybase_index::{BPlusTree, RecordPointer};
mod support;

#[test]
fn full_global_row_capacity_accepts_dense_rebuild_and_refuses_next_row_atomically() {
    let mut live = support::model(&[("items", DataType::Integer), ("other", DataType::Integer)]);
    let empty_other = live.selection(2).unwrap().clone();
    for start in (0..MAX_ROWS).step_by(MAX_EVENTS) {
        let mut staged = live.begin().unwrap();
        let end = (start + MAX_EVENTS).min(MAX_ROWS);
        for key in start..end {
            staged
                .apply(Event {
                    table_id: 1,
                    kind: EventKind::Insert(vec![
                        Value::Integer(key as i64),
                        Value::Text(format!("synthetic-{key}")),
                    ]),
                })
                .unwrap();
        }
        support::indexes(&live, &mut staged, &["items"]);
        live.publish(staged.prepare().unwrap()).unwrap();
        assert_eq!(live.view().row_count(), end);
        assert_eq!(live.selection(2).unwrap(), &empty_other);
    }
    let old = live.clone();
    let before = live.fingerprint();
    let old_images = old.selection(1).unwrap().index().fingerprint().unwrap();
    let mut entries = Vec::with_capacity(MAX_ROWS);
    for key in 0..MAX_ROWS {
        let key = Key::Integer(key as i64);
        let location = live.view().row_location("items", &key).unwrap().unwrap();
        entries.push((
            key,
            RecordPointer {
                page_id: location.page_id,
                slot_id: location.slot_id,
            },
        ));
    }
    let dense = BPlusTree::from_sorted_stable(&entries).unwrap();
    assert_eq!(dense.page_count(), 768);
    assert!(dense.page_count() > MAX_EVENTS);
    let mut rebuild = live.begin().unwrap();
    let (binding, index) = support::candidate(&live, &rebuild, "items", dense);
    assert_eq!(index.encode().unwrap().len(), (768 + 1) * 4096);
    rebuild.index(binding, index).unwrap();
    let prepared = rebuild.prepare().unwrap();
    assert_eq!(prepared.view().row_count(), MAX_ROWS);
    assert_eq!(prepared.selection(1).unwrap().binding().covered(), 10000);
    assert_eq!(live.fingerprint(), before);
    live.publish(prepared).unwrap();
    assert_eq!(live.view().row_count(), MAX_ROWS);
    assert_eq!(live.selection(2).unwrap(), &empty_other);
    assert_eq!(
        old.selection(1).unwrap().index().fingerprint().unwrap(),
        old_images
    );
    assert_eq!(old.fingerprint(), before);
    for (table, key) in [(1, MAX_ROWS as i64), (2, 0)] {
        let before = live.fingerprint();
        let mut refused = live.begin().unwrap();
        assert!(
            refused
                .apply(Event {
                    table_id: table,
                    kind: EventKind::Insert(vec![
                        Value::Integer(key),
                        Value::Text("overflow".into())
                    ]),
                })
                .is_err()
        );
        assert!(matches!(refused.prepare(), Err(Error::Aborted)));
        assert_eq!(live.fingerprint(), before);
        assert_eq!(live.view().row_count(), MAX_ROWS);
    }
    for key in [0, 1, 4095, 8191, 9999] {
        let row = live
            .view()
            .get("items", &Key::Integer(key))
            .unwrap()
            .unwrap();
        assert_eq!(row[1], Value::Text(format!("synthetic-{key}")));
    }
}

fn text_key(number: usize, bytes: usize) -> String {
    let prefix = format!("{number:05}");
    format!("{prefix}{}", "x".repeat(bytes - prefix.len()))
}

fn full_text_model(bytes: usize) -> emilybase_commit_model::Model {
    let mut live = support::model(&[("items", DataType::Text)]);
    for start in (0..MAX_ROWS).step_by(MAX_EVENTS) {
        let mut staged = live.begin().unwrap();
        let end = (start + MAX_EVENTS).min(MAX_ROWS);
        for key in start..end {
            staged
                .apply(Event {
                    table_id: 1,
                    kind: EventKind::Insert(vec![
                        Value::Text(text_key(key, bytes)),
                        Value::Text(format!("synthetic-{key}")),
                    ]),
                })
                .unwrap();
        }
        support::indexes(&live, &mut staged, &["items"]);
        live.publish(staged.prepare().unwrap()).unwrap();
        assert_eq!(live.view().row_count(), end);
    }
    live
}

#[test]
fn full_capacity_at_text_tree_boundary_preserves_every_pointer_in_dense_rebuild() {
    let mut live = full_text_model(256);
    let old = live.clone();
    let before = old.fingerprint();
    let mut entries = Vec::with_capacity(MAX_ROWS);
    for number in 0..MAX_ROWS {
        let key = Key::Text(text_key(number, 256));
        let location = live.view().row_location("items", &key).unwrap().unwrap();
        entries.push((
            key,
            RecordPointer {
                page_id: location.page_id,
                slot_id: location.slot_id,
            },
        ));
    }
    let dense = BPlusTree::from_sorted_stable(&entries).unwrap();
    assert_eq!(dense.page_count(), 768);
    let mut staged = live.begin().unwrap();
    let (binding, index) = support::candidate(&live, &staged, "items", dense);
    assert_eq!(binding.covered(), MAX_ROWS as u64);
    assert_eq!(binding.excluded(), 0);
    assert_eq!(index.encode().unwrap().len(), 3149824);
    staged.index(binding, index).unwrap();
    live.publish(staged.prepare().unwrap()).unwrap();
    for (number, (key, pointer)) in entries.iter().enumerate() {
        assert_eq!(
            live.selection(1).unwrap().index().tree.get(key).unwrap(),
            Some(*pointer)
        );
        let row = live.view().get("items", key).unwrap().unwrap();
        assert_eq!(row[1], Value::Text(format!("synthetic-{number}")));
    }
    assert_eq!(old.fingerprint(), before);
    assert_eq!(old.view().row_count(), MAX_ROWS);
}

#[test]
fn full_capacity_at_row_text_boundary_retains_all_excluded_long_keys() {
    let live = full_text_model(3072);
    let selection = live.selection(1).unwrap();
    assert_eq!(selection.binding().covered(), 0);
    assert_eq!(selection.binding().excluded(), MAX_ROWS as u64);
    assert_eq!(selection.index().tree.page_count(), 1);
    assert_eq!(selection.index().encode().unwrap().len(), 8192);
    assert_eq!(selection.index().tree.len(), 0);
    for number in 0..MAX_ROWS {
        let key = Key::Text(text_key(number, 3072));
        assert!(selection.index().tree.get(&key).is_err());
        let row = live.view().get("items", &key).unwrap().unwrap();
        assert_eq!(row[1], Value::Text(format!("synthetic-{number}")));
        assert!(live.view().row_location("items", &key).unwrap().is_some());
    }
    let before = live.fingerprint();
    let mut refused = live.begin().unwrap();
    assert!(
        refused
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![
                    Value::Text(text_key(MAX_ROWS, 3072)),
                    Value::Text("overflow".into()),
                ]),
            })
            .is_err()
    );
    assert!(matches!(refused.prepare(), Err(Error::Aborted)));
    assert_eq!(live.fingerprint(), before);
    assert_eq!(live.view().row_count(), MAX_ROWS);
}
