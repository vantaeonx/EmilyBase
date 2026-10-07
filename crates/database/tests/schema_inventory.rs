use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, MAX_TABLES, Snapshot};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn schema(name: &str, width: usize) -> Schema {
    Schema {
        name: name.into(),
        columns: (0..width)
            .map(|index| Column {
                name: format!("c{index:02}_{}", "x".repeat(50)),
                data_type: DataType::Integer,
                nullable: false,
            })
            .collect(),
        primary_key: 0,
    }
}
fn create(snapshot: &mut Snapshot, name: &str, width: usize) {
    snapshot
        .apply(Event {
            table_id: snapshot.next_table_id(),
            kind: EventKind::Create(schema(name, width)),
        })
        .unwrap();
}
fn drop_table(snapshot: &mut Snapshot, name: &str) {
    snapshot
        .apply(Event {
            table_id: snapshot.table_id(name).unwrap(),
            kind: EventKind::Drop,
        })
        .unwrap();
}
fn verify(snapshot: &Snapshot, expected: &BTreeMap<u64, Schema>) {
    assert_eq!(snapshot.table_count(), expected.len());
    let mut iterator = snapshot.schema_refs();
    assert_eq!(iterator.size_hint(), (expected.len(), Some(expected.len())));
    for (index, (id, schema)) in expected.iter().enumerate() {
        assert_eq!(iterator.len(), expected.len() - index);
        let borrowed = iterator.next().unwrap();
        assert_eq!(borrowed, schema);
        assert_eq!(snapshot.table_id(&borrowed.name).unwrap(), *id);
        assert!(std::ptr::eq(
            borrowed,
            snapshot.schema(&borrowed.name).unwrap()
        ));
    }
    assert_eq!(iterator.len(), 0);
    assert_eq!(iterator.size_hint(), (0, Some(0)));
    assert!(iterator.next().is_none());
    assert!(iterator.next_back().is_none());
    assert!(iterator.next().is_none());
    assert_eq!(
        snapshot.schemas(),
        expected.values().cloned().collect::<Vec<_>>()
    );
}

#[test]
fn gaps_and_same_name_recreation_keep_table_id_order_and_double_ended_length() {
    let mut snapshot = Snapshot::empty().unwrap();
    for name in ["z", "b", "a", "d", "c"] {
        create(&mut snapshot, name, 2);
    }
    drop_table(&mut snapshot, "b");
    drop_table(&mut snapshot, "d");
    create(&mut snapshot, "b", 64);
    let mut iterator = snapshot.schema_refs();
    assert_eq!(iterator.len(), 4);
    assert_eq!(iterator.next().unwrap().name, "z");
    assert_eq!(iterator.next_back().unwrap().name, "b");
    assert_eq!(iterator.len(), 2);
    assert_eq!(iterator.next_back().unwrap().name, "c");
    assert_eq!(iterator.next().unwrap().name, "a");
    for _ in 0..3 {
        assert!(iterator.next().is_none());
        assert!(iterator.next_back().is_none());
    }
    let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
    assert_eq!(replay.schemas(), snapshot.schemas());
    assert_eq!(replay.table_count(), 4);
    assert_eq!(replay.table_id("b").unwrap(), 6);
}

#[test]
fn complete_maximum_inventory_and_refusals_preserve_every_schema_and_page() {
    let mut snapshot = Snapshot::empty().unwrap();
    let mut expected = BTreeMap::new();
    for id in 0..MAX_TABLES {
        let name = format!("t{id:03}_{}", "n".repeat(48));
        let value = schema(&name, 64);
        let table_id = snapshot.next_table_id();
        snapshot
            .apply(Event {
                table_id,
                kind: EventKind::Create(value.clone()),
            })
            .unwrap();
        expected.insert(table_id, value);
    }
    verify(&snapshot, &expected);
    let before = snapshot.page_fingerprint();
    assert!(
        snapshot
            .apply(Event {
                table_id: snapshot.next_table_id(),
                kind: EventKind::Create(schema("overflow", 1)),
            })
            .is_err()
    );
    assert_eq!(snapshot.page_fingerprint(), before);
    verify(&snapshot, &expected);
    let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
    verify(&replay, &expected);
}

#[test]
fn borrowed_old_metadata_survives_table_detachment_and_owned_copies_are_independent() {
    let mut live = Snapshot::empty().unwrap();
    create(&mut live, "wide", 64);
    create(&mut live, "untouched", 2);
    let old = live.clone();
    let old_schema = old.schema_refs().next().unwrap();
    assert!(std::ptr::eq(old_schema, live.schema("wide").unwrap()));
    let mut owned = old.schemas();
    owned[0].name = "caller_copy".into();
    owned[0].columns[0].name = "caller_column".into();
    live.apply(Event {
        table_id: 1,
        kind: EventKind::Insert(vec![Value::Integer(0); 64]),
    })
    .unwrap();
    assert!(!std::ptr::eq(old_schema, live.schema("wide").unwrap()));
    assert!(std::ptr::eq(
        old.schema("untouched").unwrap(),
        live.schema("untouched").unwrap()
    ));
    drop_table(&mut live, "wide");
    create(&mut live, "wide", 1);
    assert_eq!(old_schema.name, "wide");
    assert_eq!(old_schema.columns.len(), 64);
    assert_eq!(old.table_id("wide").unwrap(), 1);
    assert_eq!(live.table_id("wide").unwrap(), 3);
    assert_eq!(live.schema("wide").unwrap().columns.len(), 1);
    assert_eq!(old.row_count(), 0);
    assert_eq!(old.table_count(), 2);
}

#[test]
fn immutable_metadata_can_be_read_on_multiple_threads_while_another_view_changes() {
    let mut live = Snapshot::empty().unwrap();
    for name in ["first", "second", "third"] {
        create(&mut live, name, 64);
    }
    let old = live.clone();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let old = &old;
            scope.spawn(move || {
                for _ in 0..100 {
                    assert_eq!(old.table_count(), 3);
                    assert_eq!(
                        old.schema_refs().map(|s| s.columns.len()).sum::<usize>(),
                        192
                    );
                    assert_eq!(old.schema_refs().next_back().unwrap().name, "third");
                }
            });
        }
        drop_table(&mut live, "second");
        create(&mut live, "fourth", 1);
    });
    assert_eq!(old.table_id("second").unwrap(), 2);
    assert!(live.table_id("second").is_err());
    assert_eq!(live.table_id("fourth").unwrap(), 4);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn ordered_live_metadata_matches_independent_catalog_after_successes_and_refusals(
        commands in prop::collection::vec((any::<bool>(),0u8..12,1usize..65),0..64)
    ) {
        let mut snapshot=Snapshot::empty().unwrap();
        let mut expected=BTreeMap::<u64,Schema>::new();
        for (remove,number,width) in commands {
            let old=snapshot.clone();
            let old_expected=expected.clone();
            let name=format!("t{number}");
            let existing=expected.iter().find(|(_,s)|s.name==name).map(|(id,_)|*id);
            let table_id=if remove { existing.unwrap_or(u64::MAX) } else { snapshot.next_table_id() };
            let value=schema(&name,width);
            let kind=if remove { EventKind::Drop } else { EventKind::Create(value.clone()) };
            let before=snapshot.page_fingerprint();
            let result=snapshot.apply(Event {table_id,kind});
            let accepts=if remove {existing.is_some()} else {existing.is_none()};
            prop_assert_eq!(result.is_ok(),accepts);
            if accepts {
                if remove {expected.remove(&table_id);} else {expected.insert(table_id,value);}
            } else { prop_assert_eq!(snapshot.page_fingerprint(),before); }
            verify(&snapshot,&expected);
            verify(&old,&old_expected);
        }
        let replay=Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
        verify(&replay,&expected);
    }
}
