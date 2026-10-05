use emilybase_catalog::{DataType, Key, Value};
use emilybase_commit_model::{MAX_EVENTS, MAX_SELECTED_INDEX_PAGES};
use emilybase_database::{Event, EventKind, MAX_ROWS};
use emilybase_index::RecordPointer;
#[path = "support/fragmented.rs"]
mod fragmented;
mod support;

#[test]
fn full_global_capacity_across_all_tables_selects_1536_fragmented_pages_atomically() {
    let names: Vec<_> = (1..=128).map(|table| format!("table_{table}")).collect();
    let schemas: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), DataType::Integer))
        .collect();
    let mut live = support::model(&schemas);
    for start in (0..MAX_ROWS).step_by(MAX_EVENTS) {
        let mut staged = live.begin().unwrap();
        for global in start..(start + MAX_EVENTS).min(MAX_ROWS) {
            staged
                .apply(Event {
                    table_id: (global % 128) as u64 + 1,
                    kind: EventKind::Insert(vec![
                        Value::Integer((global / 128) as i64),
                        Value::Text(format!("synthetic-{global}")),
                    ]),
                })
                .unwrap();
        }
        let refs: Vec<_> = names.iter().map(String::as_str).collect();
        support::indexes(&live, &mut staged, &refs);
        live.publish(staged.prepare().unwrap()).unwrap();
    }
    let old = live.clone();
    let old_fingerprint = live.fingerprint();
    let before_components = live.encoded_components().unwrap();
    let mut staged = live.begin().unwrap();
    let mut pages = 0;
    for (position, name) in names.iter().enumerate() {
        let count = if position < 16 { 79 } else { 78 };
        let mut entries = Vec::new();
        for number in 0..count {
            let key = Key::Integer(number);
            let location = live.view().row_location(name, &key).unwrap().unwrap();
            entries.push((
                key,
                RecordPointer {
                    page_id: location.page_id,
                    slot_id: location.slot_id,
                },
            ));
        }
        let tree = fragmented::tree(&entries, false);
        assert_eq!(tree.page_count(), 12);
        pages += tree.page_count();
        let (binding, index) = support::candidate(&live, &staged, name, tree);
        staged.index(binding, index).unwrap();
    }
    assert_eq!(pages, 1536);
    assert!(pages > 1024 && pages < MAX_SELECTED_INDEX_PAGES);
    let prepared = staged.prepare().unwrap();
    assert_eq!(prepared.view().row_count(), MAX_ROWS);
    let report = prepared.encoded_components().unwrap();
    assert_eq!(report.roots(), 128);
    assert_eq!(report.index_pages(), 1536);
    assert_eq!(report.index_bytes(), 6815744);
    assert_eq!(report.history_bytes(), before_components.history_bytes());
    assert_eq!(live.fingerprint(), old_fingerprint);
    live.publish(prepared).unwrap();
    assert_ne!(live.fingerprint(), old_fingerprint);
    assert_eq!(old.encoded_components().unwrap(), before_components);
    assert_eq!(old.fingerprint(), old_fingerprint);
    for global in 0..MAX_ROWS {
        let table = (global % 128) as u64 + 1;
        let name = &names[(table - 1) as usize];
        let key = Key::Integer((global / 128) as i64);
        let location = live.view().row_location(name, &key).unwrap().unwrap();
        let selection = live.selection(table).unwrap();
        assert_eq!(selection.binding().pages(), 12);
        assert_eq!(
            selection.index().tree.get(&key).unwrap(),
            Some(RecordPointer {
                page_id: location.page_id,
                slot_id: location.slot_id,
            })
        );
        assert_eq!(
            live.view().get(name, &key).unwrap().unwrap()[1],
            Value::Text(format!("synthetic-{global}"))
        );
    }
}
