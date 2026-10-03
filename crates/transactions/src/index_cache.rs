//! Private optional caches. Relational commits remain authoritative.
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

use crate::{Database, Error, IndexImageReport, MAX_INDEX_IMAGE_BYTES, Result};
use emilybase_database::PrimaryIndexInfo;
use rustix::fs::{AtFlags, Mode, OFlags};

pub const MAX_CACHE_WARMUP_BYTES: usize = 16 * 1024 * 1024;

/// Counts only. Budget includes a one-byte growth probe, reserved before every read.
/// It bounds cache input work, not total recovery memory or elapsed disk time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct PrimaryCacheWarmup {
    pub loaded: usize,
    pub missing: usize,
    pub rejected: usize,
    pub skipped: usize,
    pub bytes_budgeted: usize,
}

impl Database {
    /// Try current table caches within a whole-database input budget.
    /// Optional-file errors become rejected/skipped counts; WAL state is unchanged.
    pub fn warm_primary_index_caches(&mut self) -> Result<PrimaryCacheWarmup> {
        let schemas = self.view()?.schemas();
        let mut remaining = MAX_CACHE_WARMUP_BYTES;
        let mut report = PrimaryCacheWarmup::default();
        for schema in schemas {
            let result = (|| {
                self.cache_directory()?;
                let name = self.cache_name(&schema.name)?;
                let active = self.cache_file_limited(&name, &mut remaining)?;
                self.cache_directory()?;
                match active {
                    Some(active) => self
                        .load_primary_index_image(&schema.name, &active.bytes)
                        .map(Some),
                    None => Ok(None),
                }
            })();
            match result {
                Ok(Some(_)) => report.loaded += 1,
                Ok(None) => report.missing += 1,
                Err(Error::CacheWarmupBudget) => report.skipped += 1,
                Err(_) => report.rejected += 1,
            }
        }
        report.bytes_budgeted = MAX_CACHE_WARMUP_BYTES - remaining;
        Ok(report)
    }
    /// Historical counts from this owner's startup, not current cache freshness.
    pub fn primary_cache_startup(&self) -> Result<PrimaryCacheWarmup> {
        self.ready()?;
        Ok(self.cache_startup)
    }
    /// Save the current table image under an ID-derived, private filename.
    /// A stale valid image from this same database/table can be replaced.
    /// Damaged, foreign, linked or unsafe existing files are preserved and rejected.
    pub fn save_primary_index_cache(&self, table: &str) -> Result<IndexImageReport> {
        self.ready()?;
        let bytes = self.primary_index_image(table)?;
        let report = self.verify_primary_index_image(table, &bytes)?;
        self.cache_directory()?;
        let name = self.cache_name(table)?;
        let previous = self.cache_file(&name)?;
        if let Some(previous) = &previous {
            self.cache_scope(table, &previous.bytes)?;
        }
        let mut pending = Pending::create(&self.ownership)?;
        pending.file.write_all(&bytes)?;
        sync(&pending.file, "file_sync")?;
        boundary("file_synced");
        if read(&mut pending.file)? != bytes {
            return Err(Error::IndexCache("staged image changed"));
        }
        self.cache_directory()?;
        pending.check()?;
        let actual = self.cache_file(&name)?;
        if actual != previous {
            return Err(Error::IndexCache("active image changed"));
        }
        if previous.is_some() {
            rustix::fs::renameat(&self.ownership, &pending.name, &self.ownership, &name)
                .map_err(std::io::Error::from)?;
        } else {
            rustix::fs::renameat_with(
                &self.ownership,
                &pending.name,
                &self.ownership,
                &name,
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .map_err(std::io::Error::from)?;
        }
        pending.published = true;
        boundary("renamed");
        sync(&self.ownership, "directory_sync").map_err(Error::CachePublicationUnknown)?;
        boundary("directory_synced");
        self.cache_directory()?;
        Ok(report)
    }

    /// Explicitly load a matching optional cache. Absence returns None.
    /// Invalid/stale caches return an error, without poisoning relational data.
    pub fn load_primary_index_cache(&mut self, table: &str) -> Result<Option<PrimaryIndexInfo>> {
        self.ready()?;
        self.cache_directory()?;
        let name = self.cache_name(table)?;
        let Some(active) = self.cache_file(&name)? else {
            return Ok(None);
        };
        self.cache_directory()?;
        Ok(Some(self.load_primary_index_image(table, &active.bytes)?))
    }

    fn cache_name(&self, table: &str) -> Result<String> {
        Ok(format!(
            "primary-{}.table-index",
            self.view()?.table_id(table)?
        ))
    }
    fn cache_scope(&self, table: &str, bytes: &[u8]) -> Result<()> {
        let report = crate::inspect_primary_index_image(bytes)?;
        if report.table_id != self.view()?.table_id(table)? || bytes[16..32] != self.database_id() {
            return Err(Error::IndexCache("foreign active image"));
        }
        Ok(())
    }
    fn cache_directory(&self) -> Result<()> {
        let current = OpenOptions::new()
            .read(true)
            .custom_flags((OFlags::NOFOLLOW | OFlags::DIRECTORY | OFlags::NONBLOCK).bits() as i32)
            .open(&self.path)?;
        let metadata = current.metadata()?;
        let owned = self.ownership.metadata()?;
        if !metadata.is_dir()
            || metadata.permissions().mode() & 0o7777 != 0o700
            || (metadata.dev(), metadata.ino(), metadata.uid())
                != (owned.dev(), owned.ino(), owned.uid())
        {
            return Err(Error::IndexCache("private owned directory required"));
        }
        Ok(())
    }
    fn cache_file(&self, name: &str) -> Result<Option<Active>> {
        self.cache_file_limited(name, &mut (MAX_INDEX_IMAGE_BYTES + 1))
    }
    fn cache_file_limited(&self, name: &str, remaining: &mut usize) -> Result<Option<Active>> {
        let descriptor = match rustix::fs::openat(
            &self.ownership,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(descriptor) => descriptor,
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(error) => return Err(std::io::Error::from(error).into()),
        };
        let mut file = File::from(descriptor);
        let metadata = private(&file)?;
        if metadata.uid() != self.ownership.metadata()?.uid() {
            return Err(Error::IndexCache("file owner mismatch"));
        }
        let bytes = read_limited(&mut file, remaining)?;
        let after = private(&file)?;
        if metadata.len() != after.len() || bytes.len() as u64 != after.len() {
            return Err(Error::IndexCache("file changed during read"));
        }
        Ok(Some(Active {
            device: metadata.dev(),
            inode: metadata.ino(),
            bytes,
        }))
    }
}

#[derive(PartialEq, Eq)]
struct Active {
    device: u64,
    inode: u64,
    bytes: Vec<u8>,
}

struct Pending<'a> {
    owner: &'a File,
    file: File,
    name: String,
    published: bool,
}
impl<'a> Pending<'a> {
    fn create(owner: &'a File) -> Result<Self> {
        for _ in 0..32 {
            let mut nonce = [0; 16];
            getrandom::fill(&mut nonce).map_err(|_| Error::Randomness)?;
            let hex = nonce
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let name = format!(".emilybase-table-index-{hex}");
            match rustix::fs::openat(
                owner,
                &name,
                OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(descriptor) => {
                    let file = File::from(descriptor);
                    let pending = Self {
                        owner,
                        file,
                        name,
                        published: false,
                    };
                    rustix::fs::fchmod(&pending.file, Mode::RUSR | Mode::WUSR)
                        .map_err(std::io::Error::from)?;
                    pending.check()?;
                    return Ok(pending);
                }
                Err(rustix::io::Errno::EXIST) => (),
                Err(error) => return Err(std::io::Error::from(error).into()),
            }
        }
        Err(Error::IndexCache("temporary collision limit"))
    }
    fn check(&self) -> Result<()> {
        let metadata = private(&self.file)?;
        let named = rustix::fs::statat(self.owner, &self.name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(std::io::Error::from)?;
        if (named.st_dev, named.st_ino) != (metadata.dev(), metadata.ino()) {
            return Err(Error::IndexCache("staged file identity changed"));
        }
        Ok(())
    }
}
impl Drop for Pending<'_> {
    fn drop(&mut self) {
        if !self.published && self.check().is_ok() {
            let _ = rustix::fs::unlinkat(self.owner, &self.name, AtFlags::empty());
        }
    }
}
fn private(file: &File) -> Result<std::fs::Metadata> {
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.permissions().mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(Error::IndexCache(
            "private regular single-link file required",
        ));
    }
    Ok(metadata)
}
fn read(file: &mut File) -> Result<Vec<u8>> {
    read_limited(file, &mut (MAX_INDEX_IMAGE_BYTES + 1))
}
fn read_limited(file: &mut File, remaining: &mut usize) -> Result<Vec<u8>> {
    let length = private(file)?.len();
    if length > MAX_INDEX_IMAGE_BYTES as u64 {
        return Err(Error::IndexCache("image file limit"));
    }
    let cost = length as usize + 1;
    if cost > *remaining {
        return Err(Error::CacheWarmupBudget);
    }
    *remaining -= cost;
    boundary("read_budgeted");
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(cost as u64).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != length {
        return Err(Error::IndexCache("image changed during bounded read"));
    }
    Ok(bytes)
}
fn sync(file: &File, _point: &str) -> std::io::Result<()> {
    #[cfg(test)]
    crate::index_cache_tests::fail(_point, false)?;
    file.sync_all()?;
    #[cfg(test)]
    crate::index_cache_tests::fail(_point, true)?;
    Ok(())
}
fn boundary(_point: &str) {
    #[cfg(test)]
    crate::index_cache_tests::boundary(_point);
}
