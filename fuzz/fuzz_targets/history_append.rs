#![no_main]
#![forbid(unsafe_code)]

use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_storage::{PAGE_SIZE, Page};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;

fn key(text: bool, number: u8) -> Key {
    if text {
        let suffix = if number == 0 {
            "λ".repeat(1534)
        } else {
            "λ".repeat(126)
        };
        Key::Text(format!("k{number:02}\0{suffix}"))
    } else {
        Key::Integer(i64::from(number))
    }
}
fn row(key: &Key, body: &str) -> Vec<Value> {
    vec![Value::Text(body.into()), key.to_value()]
}
fn images(snapshot: &Snapshot) -> Vec<[u8; PAGE_SIZE]> {
    snapshot.pages().map(Page::encode).collect()
}
fn delta(base: &Snapshot, next: &Snapshot) -> Vec<Page> {
    let last = base.pages().last().unwrap().encode();
    next.pages()
        .skip(base.page_count() - 1)
        .filter(|page| page.id() as usize != base.page_count() || page.encode() != last)
        .cloned()
        .collect()
}
fn verify(value: &Snapshot) {
    let full = Snapshot::from_pages(value.pages().cloned().collect()).unwrap();
    assert_eq!(images(value), images(&full));
    assert_eq!(value.schemas(), full.schemas());
    assert_eq!(value.row_count(), full.row_count());
    assert_eq!(value.event_count(), full.event_count());
    assert_eq!(value.next_table_id(), full.next_table_id());
    for schema in value.schemas() {
        assert_eq!(
            value.scan(&schema.name, 10000).unwrap(),
            full.scan(&schema.name, 10000).unwrap()
        );
        for row in value.scan(&schema.name, 10000).unwrap() {
            let key = schema.key(&row).unwrap();
            let location = value.row_location(&schema.name, &key).unwrap().unwrap();
            assert_eq!(
                full.row_location(&schema.name, &key).unwrap(),
                Some(location)
            );
            assert_eq!(
                value
                    .resolve_row_location(&schema.name, &key, location)
                    .unwrap(),
                &row
            );
        }
    }
}
fn repaired(mut image: [u8; PAGE_SIZE], offset: usize, xor: u8) -> [u8; PAGE_SIZE] {
    image[offset % PAGE_SIZE] ^= xor;
    let mut crc = crc32fast::Hasher::new();
    crc.update(&image[..28]);
    crc.update(&image[32..]);
    image[28..32].copy_from_slice(&crc.finalize().to_le_bytes());
    image
}

fuzz_target!(|bytes: &[u8]| {
    if bytes.is_empty() || bytes.len() > 8192 {
        return;
    }
    let text = bytes[0] & 1 != 0;
    let mut live = Snapshot::empty().unwrap();
    live.apply(Event {
        table_id: 1,
        kind: EventKind::Create(Schema {
            name: "items".into(),
            primary_key: 1,
            columns: vec![
                Column {
                    name: "body".into(),
                    data_type: DataType::Text,
                    nullable: false,
                },
                Column {
                    name: "id".into(),
                    data_type: if text {
                        DataType::Text
                    } else {
                        DataType::Integer
                    },
                    nullable: false,
                },
            ],
        }),
    })
    .unwrap();
    let mut expected = BTreeMap::<Key, String>::new();
    for number in 0..4 {
        let key = key(text, number);
        let body = if text {
            "v".repeat(512)
        } else {
            "v".repeat(3072)
        };
        live.apply(Event {
            table_id: 1,
            kind: EventKind::Insert(row(&key, &body)),
        })
        .unwrap();
        expected.insert(key, body);
    }
    for command in bytes[1..].as_chunks::<3>().0.iter().take(32) {
        let operation = command[0] % 5;
        let key = key(text, command[1] % 16);
        let body = "λ".repeat(usize::from(command[2]) * if text { 1 } else { 6 });
        let mut next = live.clone();
        let kind = match operation {
            0 => EventKind::Insert(row(&key, &body)),
            2 => EventKind::Delete(key.clone()),
            3 | 4 if !expected.contains_key(&key) => EventKind::Insert(row(&key, &body)),
            _ => EventKind::Replace(row(&key, &body)),
        };
        let succeeds = operation >= 3
            || if operation == 0 {
                !expected.contains_key(&key)
            } else {
                expected.contains_key(&key)
            };
        let before = images(&live);
        let accepted = next.apply(Event { table_id: 1, kind }).is_ok();
        assert_eq!(accepted, succeeds);
        if accepted && operation != 3 {
            let changes = delta(&live, &next);
            if operation == 4 {
                let image = repaired(
                    changes[0].encode(),
                    usize::from(command[1]) * 16 + usize::from(command[2]),
                    command[0] | 1,
                );
                if let Ok(page) = Page::decode(&image, changes[0].id()) {
                    let mut altered = changes.clone();
                    altered[0] = page;
                    if let Ok(candidate) = live.replay_append_pages(&altered) {
                        verify(&candidate);
                    }
                }
                assert_eq!(images(&live), before);
            }
            let replayed = live.replay_append_pages(&changes).unwrap();
            assert_eq!(images(&replayed), images(&next));
            verify(&replayed);
            if operation == 2 {
                expected.remove(&key);
            } else {
                expected.insert(key, body);
            }
            assert_eq!(images(&live), before);
            live = replayed;
        } else {
            assert_eq!(images(&live), before);
        }
        assert_eq!(live.row_count(), expected.len());
        for (key, body) in &expected {
            assert_eq!(live.get("items", key).unwrap().unwrap(), &row(key, body));
        }
    }
    if bytes.len().is_multiple_of(PAGE_SIZE) {
        let before = images(&live);
        let start = live.page_count() as u64;
        for initial in [start, start + 1] {
            let decoded = bytes
                .as_chunks::<PAGE_SIZE>()
                .0
                .iter()
                .enumerate()
                .map(|(offset, chunk)| Page::decode(chunk, initial + offset as u64))
                .collect::<Result<Vec<_>, _>>();
            if let Ok(pages) = decoded
                && let Ok(value) = live.replay_append_pages(&pages)
            {
                verify(&value);
            }
            assert_eq!(images(&live), before);
        }
    }
});
