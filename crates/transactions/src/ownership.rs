use std::fs::{File, TryLockError};
use std::path::Path;

use crate::{Error, Result};

/// The directory inode remains stable when the journal is atomically replaced.
/// Keep this owner alive until journal handles are released. Paths are trusted.
pub(crate) fn lock_directory(path: &Path) -> Result<File> {
    let file = File::open(path)?;
    if !file.metadata()?.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "managed database path must be a directory",
        )
        .into());
    }
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(Error::Wal(emilybase_wal::Error::Busy)),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}
