use super::*;
use crate::{DATABASE_MARKER, EventKind, MAX_EVENTS};
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use proptest::prelude::*;
use std::sync::Arc;

fn create(name: &str, id: u64, primary: u16) -> Event {
    Event {
        table_id: id,
        kind: EventKind::Create(Schema {
            name: name.into(),
            primary_key: primary,
            columns: vec![
                Column {
                    name: "id".into(),
                    data_type: DataType::Integer,
                    nullable: false,
                },
                Column {
                    name: "body".into(),
                    data_type: DataType::Text,
                    nullable: false,
                },
            ],
        }),
    }
}
fn insert(table: u64, id: i64, body: &str) -> Event {
    Event {
        table_id: table,
        kind: EventKind::Insert(vec![Value::Integer(id), Value::Text(body.into())]),
    }
}
fn replace(table: u64, id: i64, body: &str) -> Event {
    Event {
        table_id: table,
        kind: EventKind::Replace(vec![Value::Integer(id), Value::Text(body.into())]),
    }
}
fn fixture() -> Snapshot {
    let mut base = Snapshot::empty().unwrap();
    base.apply(create("left", 1, 0)).unwrap();
    base.apply(create("right", 2, 0)).unwrap();
    for id in 0..16 {
        base.apply(insert(1, id, &"λ".repeat(1536))).unwrap();
        base.apply(insert(2, id, &"r".repeat(3072))).unwrap();
    }
    base
}
fn changed(base: &Snapshot, next: &Snapshot) -> Vec<Page> {
    let start = base.page_count() - 1;
    next.pages()
        .skip(start)
        .filter(|page| {
            page.id() as usize != base.page_count()
                || page.encode() != base.pages().last().unwrap().encode()
        })
        .cloned()
        .collect()
}
fn images(value: &Snapshot) -> Vec<[u8; 4096]> {
    value.pages().map(Page::encode).collect()
}
fn same(left: &Snapshot, right: &Snapshot) {
    assert_eq!(images(left), images(right));
    assert_eq!(left.event_count(), right.event_count());
    assert_eq!(left.next_table_id(), right.next_table_id());
    assert_eq!(left.row_count(), right.row_count());
    assert_eq!(left.schemas(), right.schemas());
    for schema in left.schemas() {
        assert_eq!(
            left.scan(&schema.name, 10000).unwrap(),
            right.scan(&schema.name, 10000).unwrap()
        );
        for row in left.scan(&schema.name, 10000).unwrap() {
            let key = schema.key(&row).unwrap();
            let a = left.row_location(&schema.name, &key).unwrap().unwrap();
            let b = right.row_location(&schema.name, &key).unwrap().unwrap();
            assert_eq!(a, b);
            assert_eq!(
                right.resolve_row_location(&schema.name, &key, b).unwrap(),
                &row
            );
        }
    }
}

#[test]
fn appended_events_match_full_replay_and_share_untouched_tables_pages_and_rows() {
    let base = fixture();
    let frozen = images(&base);
    let mut next = base.clone();
    next.apply(replace(1, 0, &"n".repeat(3072))).unwrap();
    let delta = changed(&base, &next);
    let replayed = base.replay_append_pages(&delta).unwrap();
    same(&next, &replayed);
    same(
        &replayed,
        &Snapshot::from_pages(next.pages().cloned().collect()).unwrap(),
    );
    assert_eq!(images(&base), frozen);
    assert!(Arc::ptr_eq(
        &base.state.tables[&2],
        &replayed.state.tables[&2]
    ));
    assert!(!Arc::ptr_eq(
        &base.state.tables[&1],
        &replayed.state.tables[&1]
    ));
    for id in 1..16 {
        assert!(std::ptr::eq(
            base.get("left", &Key::Integer(id)).unwrap().unwrap(),
            replayed.get("left", &Key::Integer(id)).unwrap().unwrap()
        ));
    }
    assert!(!std::ptr::eq(
        base.get("left", &Key::Integer(0)).unwrap().unwrap(),
        replayed.get("left", &Key::Integer(0)).unwrap().unwrap()
    ));
    for (old, new) in base
        .pages
        .iter()
        .zip(&replayed.pages)
        .take(base.page_count() - 1)
    {
        assert!(Arc::ptr_eq(old, new));
    }
    let empty = base.replay_append_pages(&[]).unwrap();
    for (old, new) in base.pages.iter().zip(&empty.pages) {
        assert!(Arc::ptr_eq(old, new));
    }
}

#[test]
fn empty_noop_deleted_prefix_and_reordered_pages_refuse_without_touching_base() {
    let base = fixture();
    let frozen = images(&base);
    let mut next = base.clone();
    for id in 0..3 {
        next.apply(replace(1, id, &"n".repeat(3072))).unwrap();
    }
    let delta = changed(&base, &next);
    assert_eq!(delta.len(), 3);
    let mut wrongs = vec![
        vec![Page::new(base.page_count() as u64 + 1).unwrap()],
        vec![base.pages().last().unwrap().clone()],
        vec![delta[1].clone()],
        vec![delta[0].clone(), delta[0].clone()],
        vec![delta[1].clone(), delta[0].clone()],
    ];
    let mut deleted = delta[0].clone();
    deleted.delete(0).unwrap();
    wrongs.push(vec![deleted]);
    let mut prefix = base.pages().last().unwrap().clone();
    prefix
        .update(0, &replace(2, 15, "foreign").encode().unwrap())
        .unwrap();
    prefix
        .insert(&replace(1, 0, "new").encode().unwrap())
        .unwrap();
    wrongs.push(vec![prefix]);
    wrongs.push(vec![Page::new(u64::MAX).unwrap()]);
    for wrong in wrongs {
        assert!(base.replay_append_pages(&wrong).is_err());
        assert_eq!(images(&base), frozen);
    }
    same(&next, &base.replay_append_pages(&delta).unwrap());
}

#[test]
fn valid_events_cannot_force_noncanonical_new_page_placement() {
    let mut base = Snapshot::empty().unwrap();
    base.apply(create("items", 1, 0)).unwrap();
    let event = insert(1, 1, "fits existing tail");
    let mut wrongly_appended = Page::new(2).unwrap();
    wrongly_appended.insert(&event.encode().unwrap()).unwrap();
    let before = images(&base);
    assert!(matches!(
        base.replay_append_pages(&[wrongly_appended]),
        Err(Error::Event("noncanonical history append placement"))
    ));
    assert_eq!(images(&base), before);
    assert_eq!(base.row_count(), 0);
    let mut valid = base.clone();
    valid.apply(event).unwrap();
    same(
        &valid,
        &base.replay_append_pages(&changed(&base, &valid)).unwrap(),
    );
}

#[test]
fn later_invalid_event_discards_every_preceding_private_change() {
    let mut base = Snapshot::empty().unwrap();
    base.apply(create("items", 1, 0)).unwrap();
    let mut tail = base.pages().last().unwrap().clone();
    tail.insert(&insert(1, 1, "accepted first").encode().unwrap())
        .unwrap();
    tail.insert(&insert(1, 1, "duplicate second").encode().unwrap())
        .unwrap();
    let before = images(&base);
    let tables = base.state.tables.clone();
    assert!(matches!(
        base.replay_append_pages(&[tail]),
        Err(Error::DuplicateKey)
    ));
    assert_eq!(images(&base), before);
    assert_eq!(base.row_count(), 0);
    for (id, old) in tables {
        assert!(Arc::ptr_eq(&old, &base.state.tables[&id]));
    }
}

#[test]
fn root_malformed_events_unknown_tables_and_deleted_new_slots_are_rejected() {
    let mut base = Snapshot::empty().unwrap();
    base.apply(create("items", 1, 0)).unwrap();
    let before = images(&base);
    for bytes in [
        DATABASE_MARKER.to_vec(),
        vec![0xff; 12],
        insert(99, 1, "unknown").encode().unwrap(),
    ] {
        let mut tail = base.pages().last().unwrap().clone();
        tail.insert(&bytes).unwrap();
        assert!(base.replay_append_pages(&[tail]).is_err());
        assert_eq!(images(&base), before);
    }
    let mut tail = base.pages().last().unwrap().clone();
    tail.insert(&insert(1, 1, "a").encode().unwrap()).unwrap();
    tail.delete(2).unwrap();
    assert!(base.replay_append_pages(&[tail]).is_err());
    assert_eq!(images(&base), before);
}

#[test]
fn exact_event_bound_applies_before_private_mutation_and_next_event_refuses() {
    let mut base = Snapshot::empty().unwrap();
    base.apply(create("items", 1, 0)).unwrap();
    base.apply(insert(1, 1, "0")).unwrap();
    let mut next = base.clone();
    for number in 0..MAX_APPEND_EVENTS {
        next.apply(replace(1, 1, &number.to_string())).unwrap();
    }
    let before = images(&base);
    let delta = changed(&base, &next);
    same(&next, &base.replay_append_pages(&delta).unwrap());
    next.apply(replace(1, 1, "extra")).unwrap();
    assert!(matches!(
        base.replay_append_pages(&changed(&base, &next)),
        Err(Error::Limit("history append events"))
    ));
    assert_eq!(images(&base), before);
}

#[test]
fn exact_page_bound_and_maximum_values_replay_with_shared_prefix() {
    let mut base = Snapshot::empty().unwrap();
    base.apply(create("items", 1, 0)).unwrap();
    base.apply(insert(1, 0, &"b".repeat(3072))).unwrap();
    let mut next = base.clone();
    for id in 1..=MAX_APPEND_PAGES {
        next.apply(insert(1, id as i64, &"n".repeat(3072))).unwrap();
    }
    let delta = changed(&base, &next);
    assert_eq!(delta.len(), MAX_APPEND_PAGES);
    let replayed = base.replay_append_pages(&delta).unwrap();
    same(&next, &replayed);
    assert!(Arc::ptr_eq(&base.pages[0], &replayed.pages[0]));
    next.apply(insert(1, 257, &"n".repeat(3072))).unwrap();
    assert!(matches!(
        base.replay_append_pages(&changed(&base, &next)),
        Err(Error::Limit("history append pages"))
    ));
}

#[test]
fn drop_and_recreate_with_nonfirst_text_primary_preserve_monotonic_ids() {
    let mut base = Snapshot::empty().unwrap();
    base.apply(create("items", 1, 1)).unwrap();
    base.apply(insert(1, 1, "λ\0key")).unwrap();
    let old = base.clone();
    let mut next = base.clone();
    next.apply(Event {
        table_id: 1,
        kind: EventKind::Drop,
    })
    .unwrap();
    next.apply(create("items", 2, 1)).unwrap();
    next.apply(insert(2, 2, "λ\0key")).unwrap();
    let replayed = base.replay_append_pages(&changed(&base, &next)).unwrap();
    same(&next, &replayed);
    assert_eq!(replayed.table_id("items").unwrap(), 2);
    assert_eq!(old.table_id("items").unwrap(), 1);
    assert_eq!(
        old.get("items", &Key::Text("λ\0key".into()))
            .unwrap()
            .unwrap()[0],
        Value::Integer(1)
    );
}

#[test]
fn maximum_total_history_refuses_one_more_event_without_changing_rows() {
    let mut base = Snapshot::empty().unwrap();
    base.apply(create("items", 1, 0)).unwrap();
    base.apply(insert(1, 1, "a")).unwrap();
    while base.event_count() < MAX_EVENTS - 1 {
        base.apply(replace(1, 1, "b")).unwrap();
    }
    let mut next = base.clone();
    next.apply(replace(1, 1, "last")).unwrap();
    let full = base.replay_append_pages(&changed(&base, &next)).unwrap();
    assert_eq!(full.event_count(), MAX_EVENTS);
    let mut tail = full.pages().last().unwrap().clone();
    tail.insert(&replace(1, 1, "refuse").encode().unwrap())
        .unwrap();
    let before = images(&full);
    assert!(matches!(
        full.replay_append_pages(&[tail]),
        Err(Error::Limit("events"))
    ));
    assert_eq!(images(&full), before);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn append_matches_independent_operations_and_complete_history_replay(ops in prop::collection::vec((0u8..3,0i64..16,0u8..32),0..64)) {
        let mut current=Snapshot::empty().unwrap();current.apply(create("items",1,0)).unwrap();
        let mut expected=std::collections::BTreeMap::new();
        for (operation,key,value) in ops {
            let before=current.clone();let old_images=images(&before);let body=format!("synthetic-{value}");
            let kind=match operation {0=>EventKind::Insert(vec![Value::Integer(key),Value::Text(body.clone())]),1=>EventKind::Replace(vec![Value::Integer(key),Value::Text(body.clone())]),_=>EventKind::Delete(Key::Integer(key))};
            let mut next=before.clone();let accepted=next.apply(Event {table_id:1,kind}).is_ok();
            prop_assert_eq!(accepted,if operation==0 {!expected.contains_key(&key)}else{expected.contains_key(&key)});
            if accepted {
                let delta=changed(&before,&next);current=before.replay_append_pages(&delta).unwrap();same(&next,&current);
                let full=Snapshot::from_pages(current.pages().cloned().collect()).unwrap();same(&full,&current);
                if operation==2 {expected.remove(&key);}else {expected.insert(key,body);}
            }
            prop_assert_eq!(images(&before),old_images);
            prop_assert_eq!(current.row_count(),expected.len());
            for (key,body) in &expected {prop_assert_eq!(&current.get("items",&Key::Integer(*key)).unwrap().unwrap()[1],&Value::Text(body.clone()));}
        }
    }
}

fn extended_tail(base: &Snapshot, event: Event) -> Page {
    let bytes = event.encode().unwrap();
    let mut page = base.pages().last().unwrap().clone();
    match page.insert(&bytes) {
        Ok(_) => page,
        Err(emilybase_storage::Error::PageFull) => {
            let mut next = Page::new(page.id() + 1).unwrap();
            next.insert(&bytes).unwrap();
            next
        }
        Err(error) => panic!("fixture append failed: {error}"),
    }
}

#[test]
fn full_live_row_capacity_refuses_growth_but_accepts_replacement_and_reclamation() {
    let mut base = Snapshot::empty().unwrap();
    base.apply(create("items", 1, 0)).unwrap();
    for id in 0..crate::MAX_ROWS {
        base.apply(insert(1, id as i64, "a")).unwrap();
    }
    let before = images(&base);
    let extra = extended_tail(&base, insert(1, crate::MAX_ROWS as i64, "extra"));
    assert!(matches!(
        base.replay_append_pages(&[extra]),
        Err(Error::Limit("live rows"))
    ));
    assert_eq!(images(&base), before);
    let mut next = base.clone();
    next.apply(replace(1, 0, "replacement")).unwrap();
    let replaced = base.replay_append_pages(&changed(&base, &next)).unwrap();
    same(&replaced, &next);
    assert!(std::ptr::eq(
        base.get("items", &Key::Integer(1)).unwrap().unwrap(),
        replaced.get("items", &Key::Integer(1)).unwrap().unwrap()
    ));
    let mut next = base.clone();
    next.apply(Event {
        table_id: 1,
        kind: EventKind::Delete(Key::Integer(0)),
    })
    .unwrap();
    next.apply(insert(1, crate::MAX_ROWS as i64, "reclaimed"))
        .unwrap();
    let replayed = base.replay_append_pages(&changed(&base, &next)).unwrap();
    same(&replayed, &next);
    assert_eq!(replayed.row_count(), crate::MAX_ROWS);
    assert_eq!(images(&base), before);
}

#[test]
fn full_table_capacity_reclaims_dropped_slot_without_reusing_identity() {
    let mut base = Snapshot::empty().unwrap();
    for id in 1..=crate::MAX_TABLES {
        base.apply(create(&format!("t{id}"), id as u64, 0)).unwrap();
    }
    let before = images(&base);
    let extra = extended_tail(&base, create("extra", 129, 0));
    assert!(matches!(
        base.replay_append_pages(&[extra]),
        Err(Error::Limit("tables"))
    ));
    let mut next = base.clone();
    next.apply(Event {
        table_id: 1,
        kind: EventKind::Drop,
    })
    .unwrap();
    next.apply(create("t1", 129, 0)).unwrap();
    let replayed = base.replay_append_pages(&changed(&base, &next)).unwrap();
    same(&next, &replayed);
    assert_eq!(replayed.table_id("t1").unwrap(), 129);
    assert_eq!(base.table_id("t1").unwrap(), 1);
    assert_eq!(images(&base), before);
}
