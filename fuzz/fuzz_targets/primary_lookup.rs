#![no_main]
#![forbid(unsafe_code)]

use emilybase_catalog::Key;
use emilybase_database::{MAX_ROWS, Snapshot};
use emilybase_index::MAX_KEY_BYTES;
use emilybase_transactions::recover_snapshot;
use libfuzzer_sys::fuzz_target;
mod support;

fn verify(snapshot: Snapshot) {
    let before = snapshot
        .pages()
        .map(|page| page.encode())
        .collect::<Vec<_>>();
    for schema in snapshot.schemas() {
        let rows = snapshot.scan(&schema.name, MAX_ROWS).unwrap();
        let short = rows.iter().filter(|row| {
            !matches!(schema.key(row).unwrap(), Key::Text(text) if text.len() > MAX_KEY_BYTES)
        }).count();
        let info = snapshot.primary_index_info(&schema.name).unwrap();
        assert_eq!(info.entries, short);
        assert_eq!(info.excluded_long_keys, rows.len() - short);
        assert!(info.pages > 0);
        let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
        assert_eq!(replay.primary_index_info(&schema.name).unwrap(), info);
        for row in rows.iter().take(32) {
            let key = schema.key(row).unwrap();
            assert_eq!(snapshot.get(&schema.name, &key).unwrap(), Some(row));
            assert_eq!(replay.get(&schema.name, &key).unwrap(), Some(row));
        }
    }
    assert_eq!(
        snapshot
            .pages()
            .map(|page| page.encode())
            .collect::<Vec<_>>(),
        before
    );
}

fuzz_target!(|bytes: &[u8]| {
    if bytes.len() > 20000 {
        return;
    }
    if let Ok(snapshot) = recover_snapshot(bytes, None) {
        verify(snapshot);
    }
    if let Some(repaired) = support::repaired_wal(bytes)
        && let Ok(snapshot) = recover_snapshot(&repaired, Some([7; 16]))
    {
        verify(snapshot);
    }
});
