use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use emilybase_storage::{MAX_PAGES, Page};

use crate::codec::{Frame, Payload};
use crate::io::JournalIo;
use crate::{
    DatabaseId, Error, FRAME_SIZE, HEADER_SIZE, MAX_TRANSACTION_PAGES, MAX_WAL_BYTES, Recovery,
    Result, encode_header, recover,
};

pub struct Wal {
    file: Box<dyn JournalIo>,
    id: DatabaseId,
    version: u16,
    valid_bytes: u64,
    next_transaction: u64,
    next_sequence: u64,
    poisoned: bool,
}

impl Wal {
    /// Exclusive no-clobber creation. The enclosing database publishes this log.
    pub fn create(path: impl AsRef<Path>, id: DatabaseId) -> Result<Self> {
        let header = encode_header(id)?;
        let file = create_file(path.as_ref(), &header)?;
        Ok(Self {
            file: Box::new(file),
            id,
            version: crate::WAL_VERSION,
            valid_bytes: HEADER_SIZE as u64,
            next_transaction: 1,
            next_sequence: 1,
            poisoned: false,
        })
    }

    /// A synced, self-contained version-2 baseline. The caller publishes it only
    /// after relational validation; the existing destination is never replaced.
    pub fn create_snapshot(
        path: impl AsRef<Path>,
        id: DatabaseId,
        transaction: u64,
        pages: &[Page],
    ) -> Result<Self> {
        let bytes = crate::encode_snapshot(id, transaction, pages)?;
        let file = create_file(path.as_ref(), &bytes)?;
        Ok(Self {
            file: Box::new(file),
            id,
            version: crate::SNAPSHOT_WAL_VERSION,
            valid_bytes: bytes.len() as u64,
            next_transaction: transaction + 1,
            next_sequence: pages.len() as u64 + 2,
            poisoned: false,
        })
    }

    /// Validation is read-only. Tail removal happens only before another write.
    pub fn open(
        path: impl AsRef<Path>,
        expected_id: Option<DatabaseId>,
    ) -> Result<(Self, Recovery)> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        lock(&file)?;
        if file.metadata()?.len() > MAX_WAL_BYTES as u64 {
            return Err(Error::Limit("journal bytes"));
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_WAL_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        let recovery = recover(&bytes, expected_id)?;
        let wal = Self {
            file: Box::new(file),
            id: recovery.database_id,
            version: recovery.format_version,
            valid_bytes: recovery.valid_bytes as u64,
            next_transaction: recovery.last_transaction() + 1,
            next_sequence: recovery.next_sequence,
            poisoned: false,
        };
        Ok((wal, recovery))
    }

    pub fn database_id(&self) -> DatabaseId {
        self.id
    }

    pub fn format_version(&self) -> u16 {
        self.version
    }

    pub fn last_transaction(&self) -> u64 {
        self.next_transaction - 1
    }

    pub fn valid_bytes(&self) -> u64 {
        self.valid_bytes
    }

    /// Read only acknowledged frames while retaining exclusive ownership.
    pub fn committed_bytes(&mut self) -> Result<Vec<u8>> {
        self.ready()?;
        let result = (|| {
            if self.file.length()? < self.valid_bytes {
                return Err(Error::Format("committed journal was externally truncated"));
            }
            self.file.seek(SeekFrom::Start(0))?;
            let mut bytes = vec![0; self.valid_bytes as usize];
            self.file.read_exact(&mut bytes)?;
            let recovered = recover(&bytes, Some(self.id))?;
            if recovered.discarded_bytes != 0
                || recovered.last_transaction() != self.last_transaction()
                || recovered.format_version != self.version
                || recovered.next_sequence != self.next_sequence
            {
                return Err(Error::Format("committed journal metadata changed"));
            }
            Ok(bytes)
        })();
        self.finish_io(result)
    }

    /// Write uncommitted page images. Mutable borrowing serializes pending work.
    pub fn begin(&mut self, pages: &[Page]) -> Result<Pending<'_>> {
        self.ready()?;
        if pages.is_empty() || pages.len() > MAX_TRANSACTION_PAGES {
            return Err(Error::Limit("transaction pages"));
        }
        if pages.iter().any(|page| page.id() > MAX_PAGES)
            || pages.windows(2).any(|pair| pair[0].id() >= pair[1].id())
        {
            return Err(Error::Format("page image IDs"));
        }
        let end = self.valid_bytes + ((pages.len() + 1) * FRAME_SIZE) as u64;
        if end > MAX_WAL_BYTES as u64 {
            return Err(Error::Limit("journal bytes"));
        }
        let next_sequence = self
            .next_sequence
            .checked_add(pages.len() as u64 + 1)
            .ok_or(Error::Limit("sequence numbers"))?;
        let next_transaction = self
            .next_transaction
            .checked_add(1)
            .ok_or(Error::Limit("transaction identifiers"))?;
        let mut bytes = Vec::with_capacity(pages.len() * FRAME_SIZE);
        for (index, page) in pages.iter().enumerate() {
            let frame = Frame {
                transaction: self.next_transaction,
                sequence: self.next_sequence + index as u64,
                payload: Payload::Page(page.clone()),
            };
            bytes.extend_from_slice(&frame.encode_version(self.version)?);
        }
        let commit = Frame {
            transaction: self.next_transaction,
            sequence: next_sequence - 1,
            payload: Payload::Commit {
                count: pages.len() as u32,
                digest: crc32fast::hash(&bytes),
            },
        }
        .encode_version(self.version)?;
        let result = (|| {
            self.trim_tail()?;
            self.file.seek(SeekFrom::Start(self.valid_bytes))?;
            self.file.write_all(&bytes)?;
            Ok(())
        })();
        self.finish_io(result)?;
        Ok(Pending {
            wal: self,
            commit,
            end,
            next_sequence,
            next_transaction,
        })
    }

    pub fn append(&mut self, pages: &[Page]) -> Result<u64> {
        self.begin(pages)?.commit()
    }

    fn ready(&self) -> Result<()> {
        if self.poisoned {
            Err(Error::Poisoned)
        } else {
            Ok(())
        }
    }

    fn trim_tail(&mut self) -> Result<()> {
        if self.file.length()? != self.valid_bytes {
            self.file.truncate(self.valid_bytes)?;
            self.file.sync()?;
        }
        Ok(())
    }

    fn finish_io<T>(&mut self, result: Result<T>) -> Result<T> {
        if matches!(result, Err(Error::Io(_))) {
            self.poisoned = true;
        }
        result
    }
}

fn create_file(path: &Path, bytes: &[u8]) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    lock(&file)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    File::open(parent)?.sync_all()?;
    Ok(file)
}

/// Dropping a pending batch leaves a recoverable, uncommitted tail.
/// Explicit rollback truncates and syncs it; neither path exposes a commit.
pub struct Pending<'a> {
    wal: &'a mut Wal,
    commit: [u8; FRAME_SIZE],
    end: u64,
    next_sequence: u64,
    next_transaction: u64,
}

impl Pending<'_> {
    pub fn sync_uncommitted(&mut self) -> Result<()> {
        let result = self.wal.file.sync().map_err(Error::Io);
        self.wal.finish_io(result)
    }

    pub fn commit(self) -> Result<u64> {
        self.wal.ready()?;
        let transaction = self.wal.next_transaction;
        if let Err(source) = self
            .wal
            .file
            .write_all(&self.commit)
            .and_then(|()| self.wal.file.sync())
        {
            self.wal.poisoned = true;
            return Err(Error::OutcomeUnknown {
                transaction,
                source,
            });
        }
        self.wal.valid_bytes = self.end;
        self.wal.next_sequence = self.next_sequence;
        self.wal.next_transaction = self.next_transaction;
        Ok(transaction)
    }

    pub fn rollback(self) -> Result<()> {
        self.wal.ready()?;
        let result = self.wal.trim_tail();
        self.wal.finish_io(result)
    }
}

fn lock(file: &File) -> Result<()> {
    match file.try_lock() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(Error::Busy),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}

#[cfg(test)]
#[path = "fault_tests.rs"]
mod fault_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_page_write_poisons_the_writer_without_a_commit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("redo.wal");
        let mut wal = Wal::create(&path, [7; 16]).unwrap();
        let mut page = Page::new(1).unwrap();
        page.insert(b"synthetic").unwrap();
        // Use a real OS write failure, without altering persisted test bytes.
        wal.file = Box::new(File::open(&path).unwrap());
        assert!(matches!(wal.begin(&[page.clone()]), Err(Error::Io(_))));
        assert!(matches!(wal.begin(&[page]), Err(Error::Poisoned)));
        drop(wal);
        let (_, recovered) = Wal::open(path, None).unwrap();
        assert!(recovered.committed.is_empty());
    }

    #[test]
    fn failed_commit_write_reports_unknown_outcome_and_requires_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("redo.wal");
        let mut wal = Wal::create(&path, [7; 16]).unwrap();
        let mut page = Page::new(1).unwrap();
        page.insert(b"synthetic").unwrap();
        let pending = wal.begin(&[page.clone()]).unwrap();
        pending.wal.file = Box::new(File::open(&path).unwrap());
        assert!(matches!(
            pending.commit(),
            Err(Error::OutcomeUnknown { transaction: 1, .. })
        ));
        assert!(matches!(wal.append(&[page]), Err(Error::Poisoned)));
        drop(wal);
        let (_, recovered) = Wal::open(path, None).unwrap();
        assert!(recovered.committed.is_empty());
        assert_eq!(recovered.discarded_bytes, FRAME_SIZE);
    }
}
