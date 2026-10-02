use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{Error, PAGE_SIZE, Page, Result, header};

pub const MAX_PAGES: u64 = 65536;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Exclusive synchronous access to a page file. Not a transaction manager.
pub struct Pager {
    file: File,
    pages: u64,
    poisoned: bool,
}

impl Pager {
    /// Publish a fully initialized header without replacing an existing path.
    /// Requires local hard-link and directory-sync support.
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let (mut file, mut pending) = temporary_file(parent)?;
        lock(&file)?;
        file.write_all(&header::encode())?;
        file.sync_all()?;
        fs::hard_link(&pending.path, path)?;
        fs::remove_file(&pending.path)?;
        pending.removed = true;
        File::open(parent)?.sync_all()?;
        Ok(Self {
            file,
            pages: 0,
            poisoned: false,
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        lock(&file)?;
        let length = file.metadata()?.len();
        if length < PAGE_SIZE as u64
            || length % PAGE_SIZE as u64 != 0
            || length / PAGE_SIZE as u64 > MAX_PAGES + 1
        {
            return Err(Error::FileLength(length));
        }
        let mut bytes = [0; PAGE_SIZE];
        file.read_exact(&mut bytes)?;
        header::decode(&bytes)?;
        Ok(Self {
            file,
            pages: length / PAGE_SIZE as u64 - 1,
            poisoned: false,
        })
    }

    pub fn page_count(&self) -> u64 {
        self.pages
    }

    pub fn read_page(&mut self, id: u64) -> Result<Page> {
        self.ready()?;
        if id == 0 || id > self.pages {
            return Err(Error::PageId(id));
        }
        let result = (|| {
            self.file.seek(SeekFrom::Start(id * PAGE_SIZE as u64))?;
            let mut bytes = [0; PAGE_SIZE];
            self.file.read_exact(&mut bytes)?;
            Page::decode(&bytes, id)
        })();
        self.finish_io(result)
    }

    /// Sync an existing page or append the next page. A torn write cannot be repaired yet.
    pub fn write_page(&mut self, page: &Page) -> Result<()> {
        self.ready()?;
        let id = page.id();
        if id == 0 || id > self.pages + 1 {
            return Err(Error::PageId(id));
        }
        if id > MAX_PAGES {
            return Err(Error::PageLimit);
        }
        let bytes = page.encode();
        let result = (|| {
            self.file.seek(SeekFrom::Start(id * PAGE_SIZE as u64))?;
            self.file.write_all(&bytes)?;
            self.file.sync_all()?;
            Ok(())
        })();
        self.finish_io(result)?;
        self.pages = self.pages.max(id);
        Ok(())
    }

    pub fn verify(&mut self) -> Result<()> {
        self.ready()?;
        for id in 1..=self.pages {
            self.read_page(id)?;
        }
        Ok(())
    }

    fn ready(&self) -> Result<()> {
        if self.poisoned {
            Err(Error::Poisoned)
        } else {
            Ok(())
        }
    }

    fn finish_io<T>(&mut self, result: Result<T>) -> Result<T> {
        if matches!(result, Err(Error::Io(_))) {
            self.poisoned = true;
        }
        result
    }
}

fn lock(file: &File) -> Result<()> {
    match file.try_lock() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(Error::Busy),
        Err(TryLockError::Error(error)) => Err(Error::Io(error)),
    }
}

struct Pending {
    path: PathBuf,
    removed: bool,
}

impl Drop for Pending {
    fn drop(&mut self) {
        // Best effort on an error path; the destination is never removed.
        if !self.removed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn temporary_file(parent: &Path) -> Result<(File, Pending)> {
    for _ in 0..32 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".emilybase-create-{}-{sequence}",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => {
                return Ok((
                    file,
                    Pending {
                        path,
                        removed: false,
                    },
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "temporary path collision limit",
    )
    .into())
}
