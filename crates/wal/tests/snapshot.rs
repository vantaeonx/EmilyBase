use std::fs;

use emilybase_storage::Page;
use emilybase_wal::{
    Error, FRAME_SIZE, HEADER_SIZE, MAX_TRANSACTION_PAGES, MAX_WAL_BYTES, SNAPSHOT_WAL_VERSION,
    Wal, encode_snapshot, recover,
};

const ID: [u8; 16] = [7; 16];

fn page(id: u64, value: &[u8]) -> Page {
    let mut page = Page::new(id).unwrap();
    page.insert(value).unwrap();
    page
}

fn pages() -> Vec<Page> {
    vec![
        page(1, b"acknowledged first"),
        page(2, b"acknowledged second"),
    ]
}

fn crc(bytes: &mut [u8]) {
    let end = bytes.len() - 4;
    let checksum = crc32fast::hash(&bytes[..end]);
    bytes[end..].copy_from_slice(&checksum.to_le_bytes());
}

#[test]
fn complete_baseline_binds_pages_identity_and_existing_transaction_number() {
    let bytes = encode_snapshot(ID, 42, &pages()).unwrap();
    assert_eq!(&bytes[..16], b"EMILYWAL\x02\0\0\x10\x40\x10\0\0");
    assert_eq!(&bytes[32..40], &42u64.to_le_bytes());
    assert_eq!(&bytes[40..44], &2u32.to_le_bytes());
    assert_eq!(bytes.len(), HEADER_SIZE + 3 * FRAME_SIZE);
    let recovered = recover(&bytes, Some(ID)).unwrap();
    assert_eq!(recovered.format_version, SNAPSHOT_WAL_VERSION);
    assert_eq!(recovered.last_transaction(), 42);
    assert!(recovered.committed.is_empty());
    assert_eq!(recovered.baseline.unwrap().pages, pages());
}

#[test]
fn every_baseline_byte_cut_and_single_byte_mutation_fails_closed() {
    let original = encode_snapshot(ID, 42, &pages()).unwrap();
    for cut in 0..original.len() {
        assert!(recover(&original[..cut], None).is_err(), "cut {cut}");
    }
    for offset in 0..original.len() {
        let mut changed = original.clone();
        changed[offset] ^= 1;
        assert!(recover(&changed, None).is_err(), "byte {offset}");
    }
}

#[test]
fn new_commits_rollback_and_export_continue_after_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let mut wal = Wal::create_snapshot(&path, ID, 42, &pages()).unwrap();
    assert_eq!(wal.format_version(), 2);
    assert_eq!(wal.last_transaction(), 42);
    let baseline = fs::read(&path).unwrap();
    assert_eq!(wal.committed_bytes().unwrap(), baseline);
    wal.begin(&[page(2, b"rollback")])
        .unwrap()
        .rollback()
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), baseline);
    assert_eq!(wal.append(&[page(2, b"next committed")]).unwrap(), 43);
    let boundary = wal.valid_bytes();
    {
        let mut pending = wal.begin(&[page(2, b"uncommitted")]).unwrap();
        pending.sync_uncommitted().unwrap();
    }
    assert_eq!(wal.committed_bytes().unwrap().len() as u64, boundary);
    drop(wal);
    let (mut wal, recovered) = Wal::open(&path, Some(ID)).unwrap();
    assert_eq!(recovered.last_transaction(), 43);
    assert_eq!(recovered.committed.len(), 1);
    assert_eq!(recovered.committed[0].transaction, 43);
    assert_eq!(
        recovered.committed[0].pages,
        vec![page(2, b"next committed")]
    );
    assert_eq!(recovered.baseline.unwrap().pages, pages());
    assert_eq!(recovered.discarded_bytes, FRAME_SIZE);
    assert_eq!(wal.append(&[page(2, b"after abandoned tail")]).unwrap(), 44);
    drop(wal);
    assert_eq!(
        recover(&fs::read(path).unwrap(), None)
            .unwrap()
            .last_transaction(),
        44
    );
}

#[test]
fn every_cut_after_complete_baseline_discards_only_uncommitted_frames() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let mut wal = Wal::create_snapshot(&path, ID, 42, &pages()).unwrap();
    let boundary = wal.valid_bytes() as usize;
    wal.append(&[page(2, b"replacement"), page(3, b"extra")])
        .unwrap();
    drop(wal);
    let bytes = fs::read(path).unwrap();
    for cut in boundary..bytes.len() {
        let recovered = recover(&bytes[..cut], Some(ID)).unwrap();
        assert_eq!(recovered.last_transaction(), 42, "cut {cut}");
        assert_eq!(recovered.valid_bytes, boundary);
        assert!(recovered.committed.is_empty());
        assert_eq!(recovered.baseline.unwrap().pages, pages());
    }
    assert_eq!(recover(&bytes, None).unwrap().last_transaction(), 43);
}

#[test]
fn header_crc_cannot_hide_invalid_baseline_metadata() {
    let original = encode_snapshot(ID, 42, &pages()).unwrap();
    for (offset, value) in [(8, 3), (32, 0), (40, 0), (40, 1), (40, 3), (44, 1)] {
        let mut changed = original.clone();
        changed[offset] = value;
        crc(&mut changed[..HEADER_SIZE]);
        assert!(recover(&changed, None).is_err(), "offset {offset}");
    }
    for transaction in [0, u64::MAX] {
        let mut changed = original.clone();
        changed[32..40].copy_from_slice(&transaction.to_le_bytes());
        crc(&mut changed[..HEADER_SIZE]);
        assert!(recover(&changed, None).is_err());
    }
    let mut changed = original;
    changed[8..10].copy_from_slice(&1u16.to_le_bytes());
    crc(&mut changed[..HEADER_SIZE]);
    assert!(recover(&changed, None).is_err());
}

#[test]
fn repaired_frame_crc_cannot_bypass_baseline_order_ids_count_or_digest() {
    let original = encode_snapshot(ID, 42, &pages()).unwrap();
    for (frame, offset, value) in [
        (0, 6, 1),
        (0, 8, 41),
        (0, 16, 2),
        (0, 24, 2),
        (1, 6, 4),
        (2, 6, 2),
        (2, 60, 1),
        (2, 64, 0),
    ] {
        let mut changed = original.clone();
        let start = HEADER_SIZE + frame * FRAME_SIZE;
        if offset == 64 {
            changed[start + offset] ^= 1;
        } else {
            changed[start + offset] = value;
        }
        crc(&mut changed[start..start + FRAME_SIZE]);
        assert!(
            recover(&changed, None).is_err(),
            "frame {frame} field {offset}"
        );
    }
}

#[test]
fn baseline_has_separate_page_limit_without_weakening_transaction_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.wal");
    let pages = (1..=MAX_TRANSACTION_PAGES as u64 + 1)
        .map(|id| page(id, b"bounded baseline"))
        .collect::<Vec<_>>();
    let mut wal = Wal::create_snapshot(&path, ID, 77, &pages).unwrap();
    assert!(matches!(
        wal.append(&pages),
        Err(Error::Limit("transaction pages"))
    ));
    let recovered = recover(&wal.committed_bytes().unwrap(), Some(ID)).unwrap();
    assert_eq!(recovered.baseline.as_ref().unwrap().pages, pages);
    assert_eq!(recovered.last_transaction(), 77);
}

#[test]
fn invalid_snapshot_inputs_never_create_files_or_replace_existing_paths() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("redo.wal");
    for (id, transaction, images) in [
        ([0; 16], 42, pages()),
        (ID, 0, pages()),
        (ID, u64::MAX, pages()),
        (ID, 42, vec![]),
        (ID, 42, vec![page(2, b"gap")]),
        (ID, 42, vec![page(1, b"a"), page(1, b"duplicate")]),
    ] {
        assert!(Wal::create_snapshot(&target, id, transaction, &images).is_err());
        assert!(!target.exists());
    }
    let too_many = (0..(MAX_WAL_BYTES - HEADER_SIZE) / FRAME_SIZE)
        .map(|index| Page::new(index as u64 + 1).unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(
        Wal::create_snapshot(&target, ID, 42, &too_many),
        Err(Error::Limit(_))
    ));
    assert!(!target.exists());
    fs::write(&target, b"preserve unrelated file").unwrap();
    assert!(Wal::create_snapshot(&target, ID, 42, &pages()).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"preserve unrelated file");
}

#[test]
fn baseline_creation_retains_exclusive_owner_and_private_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let wal = Wal::create_snapshot(&path, ID, 42, &pages()).unwrap();
    assert!(matches!(Wal::open(&path, None), Err(Error::Busy)));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    drop(wal);
    let before = fs::read(&path).unwrap();
    assert!(matches!(
        Wal::open(&path, Some([8; 16])),
        Err(Error::Identity)
    ));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn duplicate_baseline_and_mixed_frame_versions_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let mut wal = Wal::create_snapshot(&path, ID, 42, &pages()).unwrap();
    let boundary = wal.valid_bytes() as usize;
    wal.append(&[page(2, b"new")]).unwrap();
    drop(wal);
    let original = fs::read(path).unwrap();
    for (offset, value) in [(6, 3), (4, 1)] {
        let mut changed = original.clone();
        changed[boundary + offset] = value;
        crc(&mut changed[boundary..boundary + FRAME_SIZE]);
        assert!(recover(&changed, None).is_err());
    }
}
