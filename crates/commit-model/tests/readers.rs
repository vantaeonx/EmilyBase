use std::sync::mpsc;
use std::time::Duration;

use emilybase_catalog::{DataType, Key, Value};
use emilybase_database::{Event, EventKind};
mod support;

#[test]
fn four_historical_readers_keep_exact_rows_roots_and_components_during_publication() {
    let mut live = support::model(&[("items", DataType::Integer)]);
    let mut initial = live.begin().unwrap();
    for key in 0..180 {
        initial
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![
                    Value::Integer(key),
                    Value::Text(format!("synthetic-old-{key}")),
                ]),
            })
            .unwrap();
    }
    support::indexes(&live, &mut initial, &["items"]);
    live.publish(initial.prepare().unwrap()).unwrap();
    let old = live.clone();
    let fingerprint = old.fingerprint();
    let index_fingerprint = old.selection(1).unwrap().index_fingerprint();
    let components = old.encoded_components().unwrap();
    std::thread::scope(|scope| {
        let mut readers = Vec::new();
        let (acknowledge, completed) = mpsc::channel();
        for reader in 0..4 {
            let view = old.clone();
            let (request, receive) = mpsc::channel::<usize>();
            let acknowledge = acknowledge.clone();
            readers.push(request);
            scope.spawn(move || {
                while let Ok(turn) = receive.recv() {
                    assert_eq!(view.fingerprint(), fingerprint);
                    assert_eq!(view.encoded_components().unwrap(), components);
                    let selection = view.selection(1).unwrap();
                    assert_eq!(selection.index_fingerprint(), index_fingerprint);
                    assert_eq!(selection.index().fingerprint().unwrap(), index_fingerprint);
                    for key in 0..180 {
                        let key = Key::Integer(key);
                        let row = view.view().get("items", &key).unwrap().unwrap();
                        let number = match key {
                            Key::Integer(number) => number,
                            _ => unreachable!(),
                        };
                        assert_eq!(row[1], Value::Text(format!("synthetic-old-{number}")));
                        let location = view.view().row_location("items", &key).unwrap().unwrap();
                        let pointer = selection.index().tree.get(&key).unwrap().unwrap();
                        assert_eq!(
                            (pointer.page_id, pointer.slot_id),
                            (location.page_id, location.slot_id)
                        );
                    }
                    acknowledge.send((reader, turn)).unwrap();
                }
            });
        }
        for turn in 0..32 {
            let mut staged = live.begin().unwrap();
            staged
                .apply(Event {
                    table_id: 1,
                    kind: EventKind::Replace(vec![
                        Value::Integer(turn as i64),
                        Value::Text(format!("synthetic-new-{turn}")),
                    ]),
                })
                .unwrap();
            support::indexes(&live, &mut staged, &["items"]);
            let prepared = staged.prepare().unwrap();
            for request in &readers {
                request.send(turn).unwrap();
            }
            live.publish(prepared).unwrap();
            let mut seen = [false; 4];
            for _ in 0..4 {
                // A failed reader cannot leave this test blocked on a barrier.
                let (reader, actual_turn) =
                    completed.recv_timeout(Duration::from_secs(30)).unwrap();
                assert_eq!(actual_turn, turn);
                assert!(!seen[reader]);
                seen[reader] = true;
            }
            assert_ne!(live.fingerprint(), fingerprint);
        }
        drop(readers);
    });
    assert_eq!(old.fingerprint(), fingerprint);
    assert_eq!(old.encoded_components().unwrap(), components);
}
