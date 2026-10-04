use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
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
    let mut pending = publish::stage(&bytes, target)?;
    let written = read_file(&mut pending.file)?;
    if written != bytes || inspect_bytes(&written)? != report {
        return Err(Error::Format("staged backup differs from its source"));
    }
    synced();
    publish::publish(pending, published)?;
    Ok(report)
}

pub fn inspect(path: impl AsRef<Path>) -> Result<Report> {
    inspect_bytes(&read(path.as_ref())?)
}

pub(crate) fn read(path: &Path) -> Result<Vec<u8>> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    let mut file: File = fd.into();
    if !file.metadata()?.is_file() {
        return Err(Error::Path);
    }
    read_file(&mut file)
}

fn read_file(file: &mut File) -> Result<Vec<u8>> {
    if file.metadata()?.len() > MAX_BACKUP_BYTES as u64 {
        return Err(Error::Limit);
    }
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(0))?;
    file.take(MAX_BACKUP_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BACKUP_BYTES {
        return Err(Error::Limit);
    }
    Ok(bytes)
}
