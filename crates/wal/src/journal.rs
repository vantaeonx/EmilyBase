use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use emilybase_storage::{MAX_PAGES, Page};

use crate::codec::{Frame, Payload};
use crate::{
    DatabaseId, Error, FRAME_SIZE, HEADER_SIZE, MAX_TRANSACTION_PAGES, MAX_WAL_BYTES, Recovery,
    Result, encode_header, recover,
};

pub struct Wal {
    file: File,
    id: DatabaseId,
    valid_bytes: u64,
    next_transaction: u64,
    next_sequence: u64,
    poisoned: bool,
}

impl Wal {
    /// Exclusive no-clobber creation. The enclosing database publishes this log.
    pub fn create(path: impl AsRef<Path>, id: DatabaseId) -> Result<Self> {
        let header = encode_header(id)?;
        let path = path.as_ref();
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        lock(&file)?;
        file.write_all(&header)?;
        file.sync_all()?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        File::open(parent)?.sync_all()?;
        Ok(Self {
            file,
            id,
            valid_bytes: HEADER_SIZE as u64,
            next_transaction: 1,
            next_sequence: 1,
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
            file,
            id: recovery.database_id,
            valid_bytes: recovery.valid_bytes as u64,
            next_transaction: recovery.committed.len() as u64 + 1,
            next_sequence: recovery.next_sequence,
            poisoned: false,
        };
        Ok((wal, recovery))
    }

    pub fn database_id(&self) -> DatabaseId {
        self.id
    }

    pub fn last_transaction(&self) -> u64 {
        self.next_transaction - 1
    }

    pub fn valid_bytes(&self) -> u64 {
        self.valid_bytes
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
            bytes.extend_from_slice(&frame.encode()?);
        }
        let commit = Frame {
            transaction: self.next_transaction,
            sequence: next_sequence - 1,
            payload: Payload::Commit {
                count: pages.len() as u32,
                digest: crc32fast::hash(&bytes),
            },
        }
        .encode()?;
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
        if self.file.metadata()?.len() != self.valid_bytes {
            self.file.set_len(self.valid_bytes)?;
            self.file.sync_all()?;
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
        let result = self.wal.file.sync_all().map_err(Error::Io);
        self.wal.finish_io(result)
    }

    pub fn commit(self) -> Result<u64> {
        self.wal.ready()?;
        let transaction = self.wal.next_transaction;
        if let Err(source) = self
            .wal
            .file
            .write_all(&self.commit)
            .and_then(|()| self.wal.file.sync_all())
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
