use emilybase_storage::Page;
use emilybase_wal::{FRAME_SIZE, HEADER_SIZE, Wal, encode_header, recover};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..20000)) {
        let _ = recover(&bytes, None);
    }

    #[test]
    fn valid_headers_do_not_hide_arbitrary_frame_input(
        tail in prop::collection::vec(any::<u8>(), 0..20000)
    ) {
        let mut bytes = encode_header([9;16]).unwrap().to_vec();
        bytes.extend_from_slice(&tail);
        if let Ok(recovery) = recover(&bytes, None) {
            prop_assert!(tail.len() < FRAME_SIZE);
            prop_assert!(recovery.committed.is_empty());
        }
    }

    #[test]
    fn random_committed_pages_survive_reopen(
        values in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..512), 1..12)
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("redo.wal");
        let mut wal = Wal::create(&path, [9;16]).unwrap();
        for value in &values {
            let mut page = Page::new(1).unwrap();
            page.insert(value).unwrap();
            wal.append(&[page]).unwrap();
        }
        drop(wal);
        let bytes = std::fs::read(&path).unwrap();
        let recovery = recover(&bytes, Some([9;16])).unwrap();
        prop_assert_eq!(recovery.committed.len(), values.len());
        for (batch, value) in recovery.committed.iter().zip(&values) {
            prop_assert_eq!(batch.pages[0].get(0).unwrap(), value.as_slice());
        }
        for boundary in (HEADER_SIZE..bytes.len()).step_by(2 * FRAME_SIZE) {
            let partial = recover(&bytes[..boundary + FRAME_SIZE + 17], None).unwrap();
            prop_assert_eq!(partial.valid_bytes, boundary);
            prop_assert_eq!(partial.discarded_bytes, FRAME_SIZE + 17);
        }
    }
}
