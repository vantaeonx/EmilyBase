use std::fs::{File, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::{Error, PAGE_SIZE, Page, Result, header};

pub const MAX_PAGES: u64 = 65536;

/// Exclusive synchronous access to a page file. Not a transaction manager.
pub struct Pager {
    file: LockedFile,
    pages: u64,
    poisoned: bool,
}

impl Pager {
    /// Publish a fully initialized header without replacing an existing path.
    /// Requires local Linux no-replace rename and directory-sync support.
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        Self::create_with_pages(path, &[])
    }

    /// Initialize all supplied pages before atomic no-clobber publication.
    pub fn create_with_pages(path: impl AsRef<Path>, pages: &[Page]) -> Result<Self> {
        Self::create_with(path.as_ref(), pages, || {}, || {})
    }

    /// Publish a single-component name in the exact directory supplied by its owner.
    /// Namespace renames do not redirect this explicit descriptor-based operation.
    pub fn create_with_pages_at(
        directory: &File,
        name: impl AsRef<std::ffi::OsStr>,
        pages: &[Page],
    ) -> Result<Self> {
        validate_initial(pages)?;
        let pending = crate::creation::Pending::at(directory, name.as_ref())?;
        Self::initialize(pending, pages, || {}, || {})
    }

    pub(crate) fn create_with(
        path: &Path,
        pages: &[Page],
        synced: impl FnOnce(),
        published: impl FnOnce(),
    ) -> Result<Self> {
        validate_initial(pages)?;
        let pending = crate::creation::Pending::new(path)?;
        Self::initialize(pending, pages, synced, published)
    }

    fn initialize(
        mut pending: crate::creation::Pending,
        pages: &[Page],
        synced: impl FnOnce(),
        published: impl FnOnce(),
    ) -> Result<Self> {
        // Duplicate before locking: the guard owns the authorization lifetime
        // from first acquisition, including every failed construction path.
        let file = lock(pending.file.try_clone()?)?;
        pending.file.write_all(&header::encode())?;
        for page in pages {
            pending.file.write_all(&page.encode())?;
        }
        crate::creation::sync(&pending.file, "file_sync")?;
        synced();
        pending.check()?;
        verify_initial(&mut pending.file, pages)?;
        pending.publish(published)?;
        Ok(Self {
            file,
            pages: pages.len() as u64,
            poisoned: false,
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(path.as_ref(), |_| {})
    }

    fn open_with(path: &Path, locked: impl FnOnce(&File)) -> Result<Self> {
        let fd = rustix::fs::open(
            path,
            rustix::fs::OFlags::RDWR
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(std::io::Error::from)?;
        let file: File = fd.into();
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(Error::Path);
        }
        let mut file = lock(file)?;
        locked(&file);
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

struct LockedFile(File);
impl std::ops::Deref for LockedFile {
    type Target = File;
    fn deref(&self) -> &File {
        &self.0
    }
}
impl std::ops::DerefMut for LockedFile {
    fn deref_mut(&mut self) -> &mut File {
        &mut self.0
    }
}
impl Drop for LockedFile {
    fn drop(&mut self) {
        // End this Pager owner's authorization explicitly. Unexposed duplicate
        // descriptions (for example a transient fork) must not prolong it.
        // Unlock affects this description, not a separately opened successor.
        let _ = self.0.unlock();
    }
}

fn lock(file: File) -> Result<LockedFile> {
    match file.try_lock() {
        Ok(()) => Ok(LockedFile(file)),
        Err(TryLockError::WouldBlock) => Err(Error::Busy),
        Err(TryLockError::Error(error)) => Err(Error::Io(error)),
    }
}

fn validate_initial(pages: &[Page]) -> Result<()> {
    if pages.len() as u64 > MAX_PAGES {
        return Err(Error::PageLimit);
    }
    for (index, page) in pages.iter().enumerate() {
        if page.id() != index as u64 + 1 {
            return Err(Error::PageId(page.id()));
        }
    }
    Ok(())
}

fn verify_initial(file: &mut File, pages: &[Page]) -> Result<()> {
    let expected_length = (pages.len() as u64 + 1) * PAGE_SIZE as u64;
    let length = file.metadata()?.len();
    if length != expected_length {
        return Err(Error::FileLength(length));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = [0; PAGE_SIZE];
    file.read_exact(&mut bytes)?;
    header::decode(&bytes)?;
    if bytes != header::encode() {
        return Err(Error::Layout("staged header differs from source"));
    }
    for page in pages {
        file.read_exact(&mut bytes)?;
        if bytes != page.encode() {
            return Err(Error::Layout("staged page differs from source"));
        }
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod ownership_tests;
