#![no_main]
#![forbid(unsafe_code)]

use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_transactions::recover_snapshot;
use libfuzzer_sys::fuzz_target;
use sha2::{Digest, Sha256};
mod support;

fn physical(snapshot: &Snapshot) -> [u8; 32] {
    let mut hash = Sha256::new();
    for page in snapshot.pages() {
        hash.update(page.encode());
    }
    hash.finalize().into()
}
fn check(mut snapshot: Snapshot, input: &[u8]) {
    let expected = physical(&snapshot);
    if input.first().is_some_and(|byte| byte & 1 == 0) {
        assert_eq!(snapshot.page_fingerprint(), expected);
    }
    let old = snapshot.clone();
    for schema in snapshot.schemas() {
        if let Some(row) = snapshot.scan(&schema.name, 1).unwrap().first().cloned() {
            let key = schema.key(&row).unwrap();
            snapshot
                .apply(Event {
                    table_id: snapshot.table_id(&schema.name).unwrap(),
                    kind: EventKind::Replace(row.clone()),
                })
                .unwrap();
            assert_eq!(snapshot.get(&schema.name, &key).unwrap(), Some(&row));
            assert_ne!(physical(&snapshot), expected);
            break;
        }
    }
    assert_eq!(snapshot.page_fingerprint(), physical(&snapshot));
    assert_eq!(old.page_fingerprint(), expected);
    let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
    assert_eq!(replay.page_fingerprint(), snapshot.page_fingerprint());
}

fuzz_target!(|bytes: &[u8]| {
    if bytes.len() > 20000 {
        return;
    }
    if let Ok(snapshot) = recover_snapshot(bytes, None) {
        check(snapshot, bytes);
    }
    if let Some(repaired) = support::repaired_wal(bytes)
        && let Ok(snapshot) = recover_snapshot(&repaired, Some([7; 16]))
    {
        check(snapshot, bytes);
    }
});
