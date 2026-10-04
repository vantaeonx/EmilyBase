use crate::location::Change;
use crate::primary::IndexChange;
use crate::{Event, EventKind, Snapshot};
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_index::{BPlusTree, RecordPointer};

fn initialized() -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "t".into(),
                columns: vec![Column {
                    name: "id".into(),
                    data_type: DataType::Text,
                    nullable: false,
                }],
                primary_key: 0,
            }),
        })
        .unwrap();
    for key in [
        "a".into(),
        format!("a{}", "x".repeat(3071)),
        "b".into(),
        "c".into(),
    ] {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Text(key)]),
            })
            .unwrap();
    }
    snapshot.primary_index_info("t").unwrap();
    snapshot
}
fn fused_error(snapshot: &Snapshot, lower: Option<&Key>, backwards: bool) {
    let mut rows = snapshot.primary_rows("t", lower, None).unwrap();
    let mut error = false;
    for _ in 0..8 {
        match if backwards {
            rows.next_back()
        } else {
            rows.next()
        } {
            Some(Err(_)) => {
                error = true;
                break;
            }
            Some(Ok(_)) => {}
            None => break,
        }
    }
    assert!(error);
    assert!(rows.next().is_none());
    assert!(rows.next_back().is_none());
    assert_eq!(rows.size_hint(), (0, Some(0)));
}

#[test]
fn missing_extra_and_wrong_eligible_entries_fail_when_consumed_and_fuse_both_ends() {
    let snapshot = initialized();
    let before = snapshot.page_fingerprint();
    let valid = snapshot.export_primary_tree("t").unwrap();
    let mut missing = valid.clone();
    missing.remove(&Key::Text("b".into())).unwrap();
    let mut extra = valid.clone();
    extra
        .insert(
            Key::Text("d".into()),
            RecordPointer {
                page_id: 1,
                slot_id: 0,
            },
        )
        .unwrap();
    let mut wrong = valid;
    wrong
        .replace(
            &Key::Text("b".into()),
            RecordPointer {
                page_id: u64::MAX,
                slot_id: u16::MAX,
            },
        )
        .unwrap();
    for tree in [missing, extra, wrong, BPlusTree::new_stable()] {
        let mut branch = snapshot.clone();
        branch.primary_indexes.apply(IndexChange::Ready(1, tree));
        for backwards in [false, true] {
            fused_error(&branch, None, backwards);
        }
        assert_eq!(branch.page_fingerprint(), before);
    }
}

#[test]
fn malformed_long_locations_and_eligible_points_fail_without_yielding_a_stale_row() {
    let snapshot = initialized();
    let long = Key::Text(format!("a{}", "x".repeat(3071)));
    let mut stale = snapshot.clone();
    let mut location = stale.row_location("t", &long).unwrap().unwrap();
    location.fingerprint = [0; 32];
    stale
        .locations
        .apply(Change::Put(1, long.clone(), location));
    for backwards in [false, true] {
        fused_error(&stale, None, backwards);
    }
    let mut missing = snapshot.clone();
    missing.locations.apply(Change::Delete(1, long.clone()));
    fused_error(&missing, None, false);
    let mut wrong = snapshot.clone();
    let mut tree = wrong.export_primary_tree("t").unwrap();
    tree.replace(
        &Key::Text("b".into()),
        RecordPointer {
            page_id: 1,
            slot_id: 0,
        },
    )
    .unwrap();
    wrong.primary_indexes.apply(IndexChange::Ready(1, tree));
    for backwards in [false, true] {
        fused_error(&wrong, Some(&long), backwards);
    }
}

#[test]
fn partial_reads_check_consumed_rows_and_do_not_claim_to_validate_unread_physical_images() {
    let mut snapshot = initialized();
    let key = Key::Text("c".into());
    let mut location = snapshot.row_location("t", &key).unwrap().unwrap();
    location.fingerprint = [0; 32];
    snapshot.locations.apply(Change::Put(1, key, location));
    let mut cursor = snapshot.primary_rows("t", None, None).unwrap();
    assert_eq!(
        cursor.next().unwrap().unwrap(),
        &vec![Value::Text("a".into())]
    );
    assert!(cursor.next_back().unwrap().is_err());
    assert!(cursor.next().is_none());
}
