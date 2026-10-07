use emilybase_catalog::{Column, DataType, Schema};
use emilybase_commit_model::{ImagePlan, Model};
use emilybase_database::{Event, EventKind};

fn wide(name: &str) -> Schema {
    Schema {
        name: name.into(),
        columns: (0..64)
            .map(|i| Column {
                name: format!("c{i:02}_{}", "x".repeat(50)),
                data_type: DataType::Integer,
                nullable: false,
            })
            .collect(),
        primary_key: 63,
    }
}
fn complete() -> Model {
    let mut model = Model::new([22; 16]).unwrap();
    let mut staged = model.begin().unwrap();
    for id in 0..128 {
        let name = format!("t{id:03}");
        staged
            .apply(Event {
                table_id: staged.view().unwrap().next_table_id(),
                kind: EventKind::Create(wide(&name)),
            })
            .unwrap();
        staged.rebuild_index(&name).unwrap();
    }
    model.publish(staged.prepare().unwrap()).unwrap();
    model
}

#[test]
fn wide_complete_inventory_preserves_roots_fingerprints_and_plan_replay_after_recreation() {
    let mut model = complete();
    let old = model.clone();
    let before = old.fingerprint();
    let mut staged = model.begin().unwrap();
    for id in [1, 64, 128] {
        staged
            .apply(Event {
                table_id: id,
                kind: EventKind::Drop,
            })
            .unwrap();
    }
    for name in ["t000", "t063", "t127"] {
        let table_id = staged.view().unwrap().next_table_id();
        staged
            .apply(Event {
                table_id,
                kind: EventKind::Create(wide(name)),
            })
            .unwrap();
        staged.rebuild_index(name).unwrap();
    }
    let prepared = staged.prepare().unwrap();
    assert_eq!(prepared.retired_tables(), [1, 64, 128]);
    assert_eq!(prepared.view().table_count(), 128);
    assert_eq!(prepared.view().schema_refs().next().unwrap().name, "t001");
    assert_eq!(
        prepared.view().schema_refs().next_back().unwrap().name,
        "t127"
    );
    for id in 2..128 {
        if id != 64 {
            assert_eq!(prepared.selection(id), model.selection(id));
        }
    }
    for id in [129, 130, 131] {
        let selection = prepared.selection(id).unwrap();
        assert_eq!(selection.binding().revision(), 1);
        assert_eq!(selection.binding().covered(), 0);
        assert!(selection.binding().predecessor().is_none());
    }
    let bytes = prepared.image_plan().unwrap().encode().unwrap();
    let replay = ImagePlan::decode(&bytes).unwrap().replay(&old).unwrap();
    model.publish(prepared).unwrap();
    assert_eq!(replay.fingerprint(), model.fingerprint());
    assert_eq!(replay.view().schemas(), model.view().schemas());
    assert_eq!(old.fingerprint(), before);
    assert_eq!(old.view().table_id("t000").unwrap(), 1);
    assert_eq!(model.view().table_id("t000").unwrap(), 129);
    for id in 2..128 {
        if id != 64 {
            let name = format!("t{:03}", id - 1);
            assert!(std::ptr::eq(
                old.view().schema(&name).unwrap(),
                model.view().schema(&name).unwrap()
            ));
        }
    }
}

#[test]
fn last_changed_table_without_candidate_is_still_refused_with_complete_borrowed_inventory() {
    let model = complete();
    let before = model.fingerprint();
    let mut staged = model.begin().unwrap();
    staged
        .apply(Event {
            table_id: 128,
            kind: EventKind::Insert(vec![emilybase_catalog::Value::Integer(1); 64]),
        })
        .unwrap();
    staged.rebuild_index("t000").unwrap();
    assert!(staged.prepare().is_err());
    assert_eq!(model.fingerprint(), before);
    assert_eq!(model.view().row_count(), 0);
    assert_eq!(model.view().table_count(), 128);
}
