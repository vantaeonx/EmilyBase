use std::fs::File;
use std::io::Read;
use std::path::Path;

use emilybase_transactions::Database;

use crate::{Error, MAX_BACKUP_BYTES, Report, Result, encode, inspect_bytes, publish};

pub fn create(database: &mut Database, target: impl AsRef<Path>) -> Result<Report> {
    create_with(database, target.as_ref(), || {}, || {})
}

pub(crate) fn create_with(
    database: &mut Database,
    target: &Path,
    synced: impl FnOnce(),
    published: impl FnOnce(),
) -> Result<Report> {
    let bytes = encode(&database.committed_wal()?)?;
    let report = inspect_bytes(&bytes)?;
    let pending = publish::stage(&bytes, publish::parent(target))?;
    let written = read(&pending.path)?;
    if written != bytes || inspect_bytes(&written)? != report {
        return Err(Error::Format("staged backup differs from its source"));
    }
    synced();
    publish::publish(pending, target, published)?;
    Ok(report)
}

pub fn inspect(path: impl AsRef<Path>) -> Result<Report> {
    inspect_bytes(&read(path.as_ref())?)
}

pub(crate) fn read(path: &Path) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_BACKUP_BYTES as u64 {
        return Err(Error::Limit);
    }
    let mut bytes = Vec::new();
    file.take(MAX_BACKUP_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BACKUP_BYTES {
        return Err(Error::Limit);
    }
    Ok(bytes)
}
