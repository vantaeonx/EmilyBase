use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_storage::Page;

fn table() -> Event {
    Event {
        table_id: 1,
        kind: EventKind::Create(Schema {
            name: "items".into(),
            columns: vec![Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            }],
            primary_key: 0,
        }),
    }
}

#[test]
fn cloned_snapshots_isolate_staged_state_and_page_images() {
    let base = Snapshot::empty().unwrap();
    let original_page = base.pages().next().unwrap().encode();
    let mut staged = base.clone();
    staged.apply(table()).unwrap();
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(vec![Value::Integer(7)]),
        })
        .unwrap();
    assert!(base.schemas().is_empty());
    assert_eq!(base.pages().next().unwrap().encode(), original_page);
    let replayed = Snapshot::from_pages(staged.pages().cloned().collect()).unwrap();
    assert_eq!(
        replayed.get("items", &Key::Integer(7)).unwrap(),
        Some(&vec![Value::Integer(7)])
    );
    assert_eq!(replayed.event_count(), 3);
}

#[test]
fn failed_staging_leaves_pages_and_counts_unchanged() {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot.apply(table()).unwrap();
    let before = snapshot.pages().next().unwrap().encode();
    assert!(snapshot.apply(table()).is_err());
    assert_eq!(snapshot.pages().next().unwrap().encode(), before);
    assert_eq!(snapshot.event_count(), 2);
    assert!(
        snapshot
            .get("items", &Key::Text("wrong type".into()))
            .is_err()
    );
    assert!(snapshot.scan("items", usize::MAX).is_err());
}

#[test]
fn snapshot_replay_rejects_missing_gapped_and_deleted_pages() {
    assert!(Snapshot::from_pages(vec![]).is_err());
    let snapshot = Snapshot::empty().unwrap();
    let root = snapshot.pages().next().unwrap().clone();
    let mut deleted = root.clone();
    deleted.delete(0).unwrap();
    assert!(Snapshot::from_pages(vec![deleted]).is_err());
    assert!(Snapshot::from_pages(vec![root, Page::new(3).unwrap()]).is_err());
}
