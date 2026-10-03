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

    #[test]
    fn generated_baselines_preserve_values_and_continue_from_original_anchor(
        anchor in 1u64..100000,
        values in prop::collection::vec(prop::collection::vec(any::<u8>(),0..512),1..8)
    ) {
        let dir=tempfile::tempdir().unwrap();
        let path=dir.path().join("redo.wal");
        let pages=values.iter().enumerate().map(|(index,value)| {
            let mut page=Page::new(index as u64+1).unwrap();
            page.insert(value).unwrap();
            page
        }).collect::<Vec<_>>();
        let mut wal=Wal::create_snapshot(&path,[9;16],anchor,&pages).unwrap();
        let baseline=wal.committed_bytes().unwrap();
        let recovered=recover(&baseline,Some([9;16])).unwrap();
        prop_assert_eq!(recovered.last_transaction(),anchor);
        prop_assert_eq!(&recovered.baseline.as_ref().unwrap().pages,&pages);
        prop_assert_eq!(wal.append(&[pages.last().unwrap().clone()]).unwrap(),anchor+1);
        let committed=wal.committed_bytes().unwrap();
        wal.begin(&[pages[0].clone()]).unwrap().rollback().unwrap();
        prop_assert_eq!(&wal.committed_bytes().unwrap(),&committed);
        drop(wal);
        let (_,recovered)=Wal::open(path,Some([9;16])).unwrap();
        prop_assert_eq!(recovered.last_transaction(),anchor+1);
        prop_assert_eq!(&recovered.baseline.as_ref().unwrap().pages,&pages);
        prop_assert_eq!(recovered.committed[0].pages[0].get(0).unwrap(),values.last().unwrap().as_slice());
    }
}
