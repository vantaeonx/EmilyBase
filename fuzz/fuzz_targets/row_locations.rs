#![no_main]
#![forbid(unsafe_code)]

use emilybase_database::{RowLocation, Snapshot};
use emilybase_transactions::recover_snapshot;
use libfuzzer_sys::fuzz_target;
mod support;

fn verify(snapshot: Snapshot) {
    let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
    for schema in snapshot.schemas() {
        // A bounded sample also reaches validated large histories without quadratic work.
        for row in snapshot.scan(&schema.name, 32).unwrap() {
            let key = schema.key(&row).unwrap();
            let location = snapshot.row_location(&schema.name, &key).unwrap().unwrap();
            assert_eq!(
                replay.row_location(&schema.name, &key).unwrap(),
                Some(location)
            );
            assert_eq!(
                snapshot
                    .resolve_row_location(&schema.name, &key, location)
                    .unwrap(),
                &row
            );
            assert_eq!(
                replay
                    .resolve_row_location(&schema.name, &key, location)
                    .unwrap(),
                &row
            );
            for bad in [
                RowLocation {
                    page_id: u64::MAX,
                    ..location
                },
                RowLocation {
                    slot_id: u16::MAX,
                    ..location
                },
                RowLocation {
                    table_id: 0,
                    ..location
                },
            ] {
                assert!(
                    snapshot
                        .resolve_row_location(&schema.name, &key, bad)
                        .is_err()
                );
            }
            let mut changed = location;
            changed.fingerprint[0] ^= 1;
            assert!(
                snapshot
                    .resolve_row_location(&schema.name, &key, changed)
                    .is_err()
            );
        }
    }
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
    let _ = serde_json::from_slice::<RowLocation>(bytes);
});
