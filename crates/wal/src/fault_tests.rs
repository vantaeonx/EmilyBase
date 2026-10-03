use std::cell::Cell;
use std::io::{self, ErrorKind};

use super::*;

#[derive(Default)]
struct Schedule {
    max_write: Option<usize>,
    interrupt_write: Option<usize>,
    fail_write: Option<usize>,
    fail_sync: Option<usize>,
    sync_before_error: bool,
    fail_truncate: bool,
    fail_read: bool,
}

/// Wrap the actual locked file; only the requested operation is perturbed.
struct FaultIo {
    inner: Box<dyn JournalIo>,
    schedule: Schedule,
    writes: usize,
    syncs: Cell<usize>,
}

impl Read for FaultIo {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.schedule.fail_read {
            return Err(io::Error::other("synthetic read failure"));
        }
        self.inner.read(bytes)
    }
}

impl Seek for FaultIo {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

impl Write for FaultIo {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        if self.schedule.interrupt_write == Some(self.writes) {
            return Err(ErrorKind::Interrupted.into());
        }
        if self.schedule.fail_write == Some(self.writes) {
            return Err(ErrorKind::StorageFull.into());
        }
        let length = self.schedule.max_write.unwrap_or(bytes.len());
        self.inner.write(&bytes[..length.min(bytes.len())])
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl JournalIo for FaultIo {
    fn length(&self) -> io::Result<u64> {
        self.inner.length()
    }

    fn truncate(&self, length: u64) -> io::Result<()> {
        if self.schedule.fail_truncate {
            return Err(io::Error::other("synthetic truncate failure"));
        }
        self.inner.truncate(length)
    }

    fn sync(&self) -> io::Result<()> {
        let call = self.syncs.get() + 1;
        self.syncs.set(call);
        if self.schedule.fail_sync == Some(call) {
            if self.schedule.sync_before_error {
                self.inner.sync()?;
            }
            return Err(io::Error::other("synthetic sync failure"));
        }
        self.inner.sync()
    }
}

fn install(wal: &mut Wal, path: &Path, schedule: Schedule) {
    // Moving the existing owner into the wrapper retains its advisory lock.
    let inner = std::mem::replace(&mut wal.file, Box::new(File::open(path).unwrap()));
    wal.file = Box::new(FaultIo {
        inner,
        schedule,
        writes: 0,
        syncs: Cell::new(0),
    });
}

fn page(text: &[u8]) -> Page {
    let mut page = Page::new(1).unwrap();
    page.insert(text).unwrap();
    page
}

fn baseline(path: &Path) -> Wal {
    let mut wal = Wal::create(path, [7; 16]).unwrap();
    assert_eq!(wal.append(&[page(b"acknowledged")]).unwrap(), 1);
    wal
}

fn check_reopen(path: &Path, expected: usize) -> Recovery {
    let (_, recovered) = Wal::open(path, Some([7; 16])).unwrap();
    assert_eq!(recovered.committed.len(), expected);
    assert_eq!(recovered.committed[0].pages, vec![page(b"acknowledged")]);
    recovered
}

#[test]
fn short_writes_and_interrupted_syscalls_complete_without_losing_bytes() {
    for schedule in [
        Schedule {
            max_write: Some(137),
            ..Schedule::default()
        },
        Schedule {
            max_write: Some(137),
            interrupt_write: Some(1),
            ..Schedule::default()
        },
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("redo.wal");
        let mut wal = baseline(&path);
        install(&mut wal, &path, schedule);
        assert_eq!(wal.append(&[page(b"next acknowledged")]).unwrap(), 2);
        drop(wal);
        let recovered = check_reopen(&path, 2);
        assert_eq!(recovered.discarded_bytes, 0);
        assert_eq!(
            recovered.committed[1].pages,
            vec![page(b"next acknowledged")]
        );
    }
}

#[test]
fn zero_length_write_is_an_error_and_prevents_further_acknowledgments() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let mut wal = baseline(&path);
    install(
        &mut wal,
        &path,
        Schedule {
            max_write: Some(0),
            ..Schedule::default()
        },
    );
    assert!(matches!(
        wal.append(&[page(b"never committed")]),
        Err(Error::Io(error)) if error.kind() == ErrorKind::WriteZero
    ));
    assert_eq!(wal.last_transaction(), 1);
    assert!(matches!(wal.committed_bytes(), Err(Error::Poisoned)));
    assert!(matches!(
        wal.append(&[page(b"retry")]),
        Err(Error::Poisoned)
    ));
    drop(wal);
    assert_eq!(check_reopen(&path, 1).discarded_bytes, 0);
}

#[test]
fn storage_full_in_a_page_frame_discards_only_the_uncommitted_tail() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let mut wal = baseline(&path);
    install(
        &mut wal,
        &path,
        Schedule {
            max_write: Some(137),
            fail_write: Some(3),
            ..Schedule::default()
        },
    );
    assert!(matches!(
        wal.append(&[page(b"partial page")]),
        Err(Error::Io(error)) if error.kind() == ErrorKind::StorageFull
    ));
    assert_eq!(wal.last_transaction(), 1);
    assert!(matches!(
        wal.append(&[page(b"retry")]),
        Err(Error::Poisoned)
    ));
    drop(wal);
    assert_eq!(check_reopen(&path, 1).discarded_bytes, 274);
    let (mut wal, _) = Wal::open(&path, None).unwrap();
    assert_eq!(wal.append(&[page(b"after reopen")]).unwrap(), 2);
    drop(wal);
    assert_eq!(check_reopen(&path, 2).discarded_bytes, 0);
}

#[test]
fn storage_full_before_or_during_commit_reports_unknown_outcome() {
    for (max_write, fail_write, discarded) in
        [(FRAME_SIZE, 2, FRAME_SIZE), (137, 33, FRAME_SIZE + 137)]
    {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("redo.wal");
        let mut wal = baseline(&path);
        install(
            &mut wal,
            &path,
            Schedule {
                max_write: Some(max_write),
                fail_write: Some(fail_write),
                ..Schedule::default()
            },
        );
        assert!(matches!(
            wal.append(&[page(b"unacknowledged")]),
            Err(Error::OutcomeUnknown { transaction: 2, source })
                if source.kind() == ErrorKind::StorageFull
        ));
        assert_eq!(wal.last_transaction(), 1);
        assert!(matches!(
            wal.append(&[page(b"retry")]),
            Err(Error::Poisoned)
        ));
        drop(wal);
        assert_eq!(check_reopen(&path, 1).discarded_bytes, discarded);
    }
}

#[test]
fn sync_error_can_leave_a_complete_commit_without_an_acknowledgment() {
    for sync_before_error in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("redo.wal");
        let mut wal = baseline(&path);
        install(
            &mut wal,
            &path,
            Schedule {
                fail_sync: Some(1),
                sync_before_error,
                ..Schedule::default()
            },
        );
        assert!(matches!(
            wal.append(&[page(b"outcome must be inspected")]),
            Err(Error::OutcomeUnknown { transaction: 2, .. })
        ));
        assert_eq!(wal.last_transaction(), 1);
        assert!(matches!(
            wal.append(&[page(b"retry")]),
            Err(Error::Poisoned)
        ));
        drop(wal);
        // Reopen observes filesystem bytes, not an emulated power failure.
        let recovered = check_reopen(&path, 2);
        assert_eq!(recovered.discarded_bytes, 0);
        assert_eq!(
            recovered.committed[1].pages,
            vec![page(b"outcome must be inspected")]
        );
    }
}

#[test]
fn uncommitted_sync_error_cannot_be_followed_by_commit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let mut wal = baseline(&path);
    install(
        &mut wal,
        &path,
        Schedule {
            fail_sync: Some(1),
            ..Schedule::default()
        },
    );
    let mut pending = wal.begin(&[page(b"no commit frame")]).unwrap();
    assert!(matches!(pending.sync_uncommitted(), Err(Error::Io(_))));
    assert!(matches!(pending.commit(), Err(Error::Poisoned)));
    drop(wal);
    assert_eq!(check_reopen(&path, 1).discarded_bytes, FRAME_SIZE);
}

#[test]
fn failed_rollback_truncation_preserves_old_commits_and_requires_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let mut wal = baseline(&path);
    install(
        &mut wal,
        &path,
        Schedule {
            fail_truncate: true,
            ..Schedule::default()
        },
    );
    let pending = wal.begin(&[page(b"abandoned")]).unwrap();
    assert!(matches!(pending.rollback(), Err(Error::Io(_))));
    assert!(matches!(
        wal.append(&[page(b"retry")]),
        Err(Error::Poisoned)
    ));
    drop(wal);
    assert_eq!(check_reopen(&path, 1).discarded_bytes, FRAME_SIZE);
}

#[test]
fn failed_tail_sync_prevents_any_new_transaction_frame() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let mut wal = baseline(&path);
    {
        let _pending = wal.begin(&[page(b"abandoned")]).unwrap();
    }
    install(
        &mut wal,
        &path,
        Schedule {
            fail_sync: Some(1),
            ..Schedule::default()
        },
    );
    assert!(matches!(wal.begin(&[page(b"new")]), Err(Error::Io(_))));
    assert!(matches!(wal.begin(&[page(b"retry")]), Err(Error::Poisoned)));
    drop(wal);
    assert_eq!(check_reopen(&path, 1).discarded_bytes, 0);
}

#[test]
fn failed_committed_export_poisons_owner_without_modifying_old_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redo.wal");
    let mut wal = baseline(&path);
    let before = std::fs::read(&path).unwrap();
    install(
        &mut wal,
        &path,
        Schedule {
            fail_read: true,
            ..Schedule::default()
        },
    );
    assert!(matches!(wal.committed_bytes(), Err(Error::Io(_))));
    assert!(matches!(
        wal.append(&[page(b"retry")]),
        Err(Error::Poisoned)
    ));
    drop(wal);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    check_reopen(&path, 1);
}
