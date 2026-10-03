use emilybase_catalog::{Column, DataType, Key, Row, Schema, Value};
use emilybase_database::{Error, Event, EventKind, RowLocation, Snapshot};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn create(snapshot: &mut Snapshot, name: &str) -> u64 {
    let id = snapshot.next_table_id();
    snapshot
        .apply(Event {
            table_id: id,
            kind: EventKind::Create(Schema {
                name: name.into(),
                columns: vec![
                    Column {
                        name: "id".into(),
                        data_type: DataType::Integer,
                        nullable: false,
                    },
                    Column {
                        name: "v".into(),
                        data_type: DataType::Text,
                        nullable: false,
                    },
                ],
                primary_key: 0,
            }),
        })
        .unwrap();
    id
}
fn row(key: i64, text: &str) -> Row {
    vec![Value::Integer(key), Value::Text(text.into())]
}
fn insert(snapshot: &mut Snapshot, table: u64, key: i64, text: &str) {
    snapshot
        .apply(Event {
            table_id: table,
            kind: EventKind::Insert(row(key, text)),
        })
        .unwrap();
}
fn pages(snapshot: &Snapshot) -> Vec<[u8; 4096]> {
    snapshot.pages().map(|page| page.encode()).collect()
}

#[test]
fn locations_identify_actual_slotted_records_across_page_boundaries_and_replay() {
    let mut snapshot = Snapshot::empty().unwrap();
    let table = create(&mut snapshot, "t");
    for key in 0..4 {
        insert(&mut snapshot, table, key, &"x".repeat(3000));
    }
    assert_eq!(snapshot.page_count(), 4);
    let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
    for id in 0..4 {
        let key = Key::Integer(id);
        let location = snapshot.row_location("t", &key).unwrap().unwrap();
        assert_eq!(location.table_id, table);
        assert_eq!(location.page_id, id as u64 + 1);
        assert_eq!(location.slot_id, if location.page_id == 1 { 2 } else { 0 });
        let record = snapshot
            .pages()
            .nth(location.page_id as usize - 1)
            .unwrap()
            .get(location.slot_id)
            .unwrap();
        let event = Event::decode(record).unwrap();
        assert_eq!(event.table_id, table);
        assert!(matches!(event.kind, EventKind::Insert(_)));
        assert_eq!(replay.row_location("t", &key).unwrap(), Some(location));
        assert_eq!(
            snapshot.resolve_row_location("t", &key, location).unwrap(),
            replay.resolve_row_location("t", &key, location).unwrap()
        );
    }
    assert_eq!(pages(&snapshot), pages(&replay));
}

#[test]
fn update_delete_and_reinsert_retire_old_locations_but_keep_old_snapshot_readable() {
    let mut snapshot = Snapshot::empty().unwrap();
    let table = create(&mut snapshot, "t");
    insert(&mut snapshot, table, 7, "old");
    let key = Key::Integer(7);
    let old = snapshot.row_location("t", &key).unwrap().unwrap();
    let historical = snapshot.clone();
    snapshot
        .apply(Event {
            table_id: table,
            kind: EventKind::Replace(row(7, "new")),
        })
        .unwrap();
    let new = snapshot.row_location("t", &key).unwrap().unwrap();
    assert_ne!(old, new);
    assert!(matches!(
        snapshot.resolve_row_location("t", &key, old),
        Err(Error::StaleLocation)
    ));
    assert_eq!(
        historical.resolve_row_location("t", &key, old).unwrap(),
        &row(7, "old")
    );
    assert!(historical.resolve_row_location("t", &key, new).is_err());
    snapshot
        .apply(Event {
            table_id: table,
            kind: EventKind::Delete(key.clone()),
        })
        .unwrap();
    assert_eq!(snapshot.row_location("t", &key).unwrap(), None);
    assert!(snapshot.resolve_row_location("t", &key, new).is_err());
    insert(&mut snapshot, table, 7, "old");
    let reinserted = snapshot.row_location("t", &key).unwrap().unwrap();
    assert_ne!(old, reinserted);
    assert!(snapshot.resolve_row_location("t", &key, old).is_err());
    assert_eq!(
        snapshot
            .resolve_row_location("t", &key, reinserted)
            .unwrap(),
        &row(7, "old")
    );
}

#[test]
fn divergent_staged_images_at_the_same_page_slot_require_matching_fingerprints() {
    let mut base = Snapshot::empty().unwrap();
    let table = create(&mut base, "t");
    let mut a = base.clone();
    let mut b = base.clone();
    insert(&mut a, table, 1, "branch A");
    insert(&mut b, table, 1, "branch B");
    let key = Key::Integer(1);
    let left = a.row_location("t", &key).unwrap().unwrap();
    let right = b.row_location("t", &key).unwrap().unwrap();
    assert_eq!(
        (left.table_id, left.page_id, left.slot_id),
        (right.table_id, right.page_id, right.slot_id)
    );
    assert_ne!(left.fingerprint, right.fingerprint);
    assert!(a.resolve_row_location("t", &key, right).is_err());
    assert!(b.resolve_row_location("t", &key, left).is_err());
    assert_eq!(base.row_location("t", &key).unwrap(), None);
    assert!(base.resolve_row_location("t", &key, left).is_err());
    assert_eq!(
        a.resolve_row_location("t", &key, left).unwrap(),
        &row(1, "branch A")
    );
}

#[test]
fn table_key_and_recreated_table_identity_cannot_be_substituted() {
    let mut snapshot = Snapshot::empty().unwrap();
    let table = create(&mut snapshot, "t");
    let sibling = create(&mut snapshot, "s");
    insert(&mut snapshot, table, 1, "a");
    insert(&mut snapshot, sibling, 1, "b");
    let key = Key::Integer(1);
    let location = snapshot.row_location("t", &key).unwrap().unwrap();
    assert!(snapshot.resolve_row_location("s", &key, location).is_err());
    assert!(
        snapshot
            .resolve_row_location("t", &Key::Integer(2), location)
            .is_err()
    );
    assert!(
        snapshot
            .resolve_row_location("t", &Key::Text("invalid".into()), location)
            .is_err()
    );
    snapshot
        .apply(Event {
            table_id: table,
            kind: EventKind::Drop,
        })
        .unwrap();
    assert!(matches!(
        snapshot.resolve_row_location("t", &key, location),
        Err(Error::NoTable)
    ));
    let recreated = create(&mut snapshot, "t");
    insert(&mut snapshot, recreated, 1, "a");
    assert_ne!(recreated, table);
    assert!(snapshot.resolve_row_location("t", &key, location).is_err());
    assert!(
        snapshot
            .resolve_row_location(
                "s",
                &key,
                snapshot.row_location("s", &key).unwrap().unwrap()
            )
            .is_ok()
    );
}

#[test]
fn forged_extreme_locations_and_failed_writes_preserve_all_pages_and_positions() {
    let mut snapshot = Snapshot::empty().unwrap();
    let table = create(&mut snapshot, "t");
    insert(&mut snapshot, table, 1, "synthetic");
    let key = Key::Integer(1);
    let location = snapshot.row_location("t", &key).unwrap().unwrap();
    let before = pages(&snapshot);
    let mut invalid = Vec::new();
    for page_id in [0, u64::MAX] {
        invalid.push(RowLocation {
            page_id,
            ..location
        });
    }
    invalid.push(RowLocation {
        slot_id: u16::MAX,
        ..location
    });
    invalid.push(RowLocation {
        table_id: 0,
        ..location
    });
    invalid.push(RowLocation {
        table_id: u64::MAX,
        ..location
    });
    invalid.push(RowLocation {
        fingerprint: [0; 32],
        ..location
    });
    for value in invalid {
        assert!(matches!(
            snapshot.resolve_row_location("t", &key, value),
            Err(Error::StaleLocation)
        ));
        assert_eq!(pages(&snapshot), before);
    }
    for kind in [
        EventKind::Insert(row(1, "duplicate")),
        EventKind::Replace(row(2, "absent")),
        EventKind::Delete(Key::Integer(2)),
        EventKind::Insert(row(2, &"x".repeat(3073))),
    ] {
        assert!(
            snapshot
                .apply(Event {
                    table_id: table,
                    kind
                })
                .is_err()
        );
        assert_eq!(snapshot.row_location("t", &key).unwrap(), Some(location));
        assert_eq!(pages(&snapshot), before);
    }
}

#[test]
fn all_column_types_and_maximum_utf8_primary_keys_resolve_exactly() {
    let mut snapshot = Snapshot::empty().unwrap();
    let schema = Schema {
        name: "typed".into(),
        columns: vec![
            Column {
                name: "id".into(),
                data_type: DataType::Text,
                nullable: false,
            },
            Column {
                name: "flag".into(),
                data_type: DataType::Boolean,
                nullable: false,
            },
            Column {
                name: "number".into(),
                data_type: DataType::Float,
                nullable: false,
            },
            Column {
                name: "blob".into(),
                data_type: DataType::Bytes,
                nullable: false,
            },
            Column {
                name: "optional".into(),
                data_type: DataType::Integer,
                nullable: true,
            },
        ],
        primary_key: 0,
    };
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(schema),
        })
        .unwrap();
    let text = "界".repeat(1024);
    let key = Key::Text(text.clone());
    let row = vec![
        Value::Text(text),
        Value::Boolean(true),
        Value::Float(-0.0),
        Value::Bytes(vec![0, 255, 1]),
        Value::Null,
    ];
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(row.clone()),
        })
        .unwrap();
    let location = snapshot.row_location("typed", &key).unwrap().unwrap();
    assert_eq!(
        snapshot
            .resolve_row_location("typed", &key, location)
            .unwrap(),
        &row
    );
    let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
    assert_eq!(replay.row_location("typed", &key).unwrap(), Some(location));
    assert_eq!(
        replay
            .resolve_row_location("typed", &key, location)
            .unwrap(),
        &row
    );
    let mut wrong = location;
    wrong.fingerprint[31] ^= 1;
    assert!(replay.resolve_row_location("typed", &key, wrong).is_err());
}

#[test]
fn location_json_is_strict_and_debug_omits_row_fingerprint() {
    let mut snapshot = Snapshot::empty().unwrap();
    let table = create(&mut snapshot, "t");
    insert(&mut snapshot, table, 1, "synthetic private payload");
    let location = snapshot
        .row_location("t", &Key::Integer(1))
        .unwrap()
        .unwrap();
    let value = serde_json::to_value(location).unwrap();
    assert_eq!(
        serde_json::from_value::<RowLocation>(value.clone()).unwrap(),
        location
    );
    let debug = format!("{location:?}");
    assert!(debug.contains("[redacted]"));
    assert!(!debug.contains("synthetic private payload"));
    assert!(!debug.contains(&format!("{:?}", location.fingerprint)));
    let mut extra = value.clone();
    extra["private_extra"] = serde_json::json!("hidden");
    assert!(serde_json::from_value::<RowLocation>(extra).is_err());
    for length in [0, 31, 33, 256] {
        let mut damaged = value.clone();
        damaged["fingerprint"] = serde_json::json!(vec![0; length]);
        assert!(serde_json::from_value::<RowLocation>(damaged).is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn generated_locations_follow_independent_live_rows_and_rebuild_identically(
        operations in proptest::collection::vec((0u8..4,0i64..16,"[a-z]{0,32}"),0..80)
    ) {
        let mut snapshot = Snapshot::empty().unwrap();
        let table = create(&mut snapshot,"t");
        let mut model = BTreeMap::new();
        let mut retired = Vec::new();
        for (action,id,text) in operations {
            let key = Key::Integer(id);
            if let Some(previous) = snapshot.row_location("t",&key).unwrap()
                && action<3 {
                retired.push((key.clone(),previous));
            }
            match action {
                0|1 => {
                    let kind = if model.contains_key(&id) {EventKind::Replace(row(id,&text))}
                        else {EventKind::Insert(row(id,&text))};
                    snapshot.apply(Event{table_id:table,kind}).unwrap(); model.insert(id,text);
                }
                2 if model.contains_key(&id) => {
                    snapshot.apply(Event{table_id:table,kind:EventKind::Delete(key)}).unwrap();model.remove(&id);
                }
                _ => {
                    let before=pages(&snapshot);
                    let mut branch=snapshot.clone();
                    if model.contains_key(&id) {branch.apply(Event{table_id:table,kind:EventKind::Replace(row(id,"discarded"))}).unwrap();}
                    else {insert(&mut branch,table,id,"discarded");}
                    let speculative=branch.row_location("t",&Key::Integer(id)).unwrap().unwrap();
                    prop_assert!(snapshot.resolve_row_location("t",&Key::Integer(id),speculative).is_err());
                    prop_assert_eq!(pages(&snapshot),before);
                }
            }
            let replay=Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
            for id in 0..16 {
                let key=Key::Integer(id);
                let location=snapshot.row_location("t",&key).unwrap();
                prop_assert_eq!(location,replay.row_location("t",&key).unwrap());
                if let Some(text)=model.get(&id) {
                    let expected=row(id,text);
                    prop_assert_eq!(snapshot.resolve_row_location("t",&key,location.unwrap()).unwrap(),&expected);
                    prop_assert_eq!(replay.resolve_row_location("t",&key,location.unwrap()).unwrap(),&expected);
                } else {prop_assert!(location.is_none());}
            }
            for (key,old) in &retired {prop_assert!(snapshot.resolve_row_location("t",key,*old).is_err());}
        }
    }
}
