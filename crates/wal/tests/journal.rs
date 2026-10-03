use std::fs;
use std::io::Write;

use emilybase_storage::Page;
use emilybase_wal::{Error, FRAME_SIZE, HEADER_SIZE, MAX_TRANSACTION_PAGES, Wal, recover};

const ID: [u8; 16] = [7; 16];

fn page(id: u64, text: &[u8]) -> Page {
    let mut page = Page::new(id).unwrap();
    page.insert(text).unwrap();
    page
}

#[test]
fn committed_batches_reopen_with_order_and_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let mut wal = Wal::create(&path, ID).unwrap();
    assert_eq!(wal.append(&[page(1, b"first")]).unwrap(), 1);
    assert_eq!(
        wal.append(&[page(1, b"replacement"), page(2, b"second")])
            .unwrap(),
        2
    );
    let before = fs::read(&path).unwrap();
    drop(wal);
    let (mut wal, recovery) = Wal::open(&path, Some(ID)).unwrap();
    assert_eq!(recovery.committed.len(), 2);
    assert_eq!(recovery.committed[1].transaction, 2);
    assert_eq!(
        recovery.committed[1].pages[0].get(0).unwrap(),
        b"replacement"
    );
    assert_eq!(recovery.discarded_bytes, 0);
    assert_eq!(wal.database_id(), ID);
    assert_eq!(wal.last_transaction(), 2);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(wal.append(&[page(2, b"third")]).unwrap(), 3);
    drop(wal);
    assert!(matches!(
        Wal::open(&path, Some([8; 16])),
        Err(Error::Identity)
    ));
}

#[test]
fn rollback_and_dropped_pending_batches_are_not_commits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let mut wal = Wal::create(&path, ID).unwrap();
    wal.append(&[page(1, b"committed")]).unwrap();
    let committed = fs::read(&path).unwrap();
    wal.begin(&[page(1, b"rolled back")])
        .unwrap()
        .rollback()
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), committed);
    {
        let mut pending = wal.begin(&[page(1, b"abandoned")]).unwrap();
        pending.sync_uncommitted().unwrap();
    }
    drop(wal);
    let with_tail = fs::read(&path).unwrap();
    let (mut wal, recovery) = Wal::open(&path, Some(ID)).unwrap();
    assert_eq!(recovery.committed.len(), 1);
    assert_eq!(recovery.discarded_bytes, FRAME_SIZE);
    assert_eq!(fs::read(&path).unwrap(), with_tail);
    assert_eq!(wal.append(&[page(1, b"next valid commit")]).unwrap(), 2);
    drop(wal);
    let (_, recovery) = Wal::open(&path, Some(ID)).unwrap();
    assert_eq!(recovery.committed.len(), 2);
    assert_eq!(recovery.discarded_bytes, 0);
}

#[test]
fn every_cut_inside_a_batch_preserves_only_the_previous_commit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let mut wal = Wal::create(&path, ID).unwrap();
    wal.append(&[page(1, b"acknowledged")]).unwrap();
    let boundary = wal.valid_bytes() as usize;
    wal.append(&[page(1, b"not yet acknowledged"), page(2, b"extra")])
        .unwrap();
    drop(wal);
    let bytes = fs::read(path).unwrap();
    for cut in boundary..bytes.len() {
        let recovery = recover(&bytes[..cut], Some(ID)).unwrap();
        assert_eq!(recovery.committed.len(), 1, "cut {cut}");
        assert_eq!(recovery.valid_bytes, boundary);
        assert_eq!(
            recovery.committed[0].pages[0].get(0).unwrap(),
            b"acknowledged"
        );
    }
    assert_eq!(recover(&bytes, None).unwrap().committed.len(), 2);
}

#[test]
fn invalid_inputs_and_ownership_do_not_change_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let mut wal = Wal::create(&path, ID).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(matches!(Wal::open(&path, None), Err(Error::Busy)));
    assert!(Wal::create(&path, ID).is_err());
    assert!(wal.append(&[]).is_err());
    assert!(wal.append(&[page(2, b"a"), page(1, b"b")]).is_err());
    assert!(wal.append(&[page(1, b"a"), page(1, b"b")]).is_err());
    assert!(
        wal.append(
            &(1..=MAX_TRANSACTION_PAGES as u64 + 1)
                .map(|id| page(id, b"bounded"))
                .collect::<Vec<_>>()
        )
        .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    let zero = dir.path().join("zero.wal");
    assert!(Wal::create(&zero, [0; 16]).is_err());
    assert!(!zero.exists());
}

#[test]
fn complete_corruption_fails_closed_without_truncating() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let mut wal = Wal::create(&path, ID).unwrap();
    wal.append(&[page(1, b"committed")]).unwrap();
    drop(wal);
    let clean = fs::read(&path).unwrap();
    for offset in [0, 20, 63, HEADER_SIZE, HEADER_SIZE + 100, clean.len() - 1] {
        let mut damaged = clean.clone();
        damaged[offset] ^= 1;
        fs::write(&path, &damaged).unwrap();
        assert!(Wal::open(&path, None).is_err(), "offset {offset}");
        assert_eq!(fs::read(&path).unwrap(), damaged);
    }
}

#[test]
fn partial_tail_is_removed_before_the_next_commit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let mut wal = Wal::create(&path, ID).unwrap();
    wal.append(&[page(1, b"old")]).unwrap();
    drop(wal);
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"partial interrupted frame")
        .unwrap();
    let (mut wal, recovery) = Wal::open(&path, None).unwrap();
    assert_eq!(recovery.discarded_bytes, 25);
    wal.append(&[page(1, b"new")]).unwrap();
    drop(wal);
    let (_, recovery) = Wal::open(&path, None).unwrap();
    assert_eq!(recovery.discarded_bytes, 0);
    assert_eq!(recovery.committed[1].pages[0].get(0).unwrap(), b"new");
}

#[cfg(unix)]
#[test]
fn journal_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    drop(Wal::create(&path, ID).unwrap());
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn oversized_logs_are_rejected_before_reading_their_contents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oversized.wal");
    let file = fs::File::create_new(&path).unwrap();
    file.set_len(emilybase_wal::MAX_WAL_BYTES as u64 + 1)
        .unwrap();
    assert!(matches!(
        Wal::open(&path, None),
        Err(Error::Limit("journal bytes"))
    ));
    assert_eq!(
        file.metadata().unwrap().len(),
        emilybase_wal::MAX_WAL_BYTES as u64 + 1
    );
}

#[test]
fn largest_allowed_transaction_is_recoverable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.wal");
    let pages = (1..=MAX_TRANSACTION_PAGES as u64)
        .map(|id| page(id, b"synthetic bounded batch"))
        .collect::<Vec<_>>();
    let mut wal = Wal::create(&path, ID).unwrap();
    wal.append(&pages).unwrap();
    drop(wal);
    let (_, recovered) = Wal::open(path, None).unwrap();
    assert_eq!(recovered.committed.len(), 1);
    assert_eq!(recovered.committed[0].pages, pages);
}
