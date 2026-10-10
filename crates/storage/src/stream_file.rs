//! Bounded reader publication through the original owned no-replace stage.
use crate::{
    Error, Result,
    creation::{Pending, sync},
};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;

/// Publish an exact immutable source in an already-owned directory and retain
/// the selected inode at EOF. The caller must keep source bytes stable across
/// both passes. Standard Read/Seek contracts and native filesystem trust apply.
/// No user authority, time bound or namespace lease is provided. A result of
/// PublicationUnknown requires inspection rather than overwrite or blind retry.
pub fn publish_private_reader_at_retained(
    directory: &File,
    name: impl AsRef<std::ffi::OsStr>,
    source: &mut (impl Read + Seek),
    exact_bytes: usize,
    maximum: usize,
) -> Result<File> {
    if exact_bytes > maximum {
        return Err(Error::FileLength(exact_bytes as u64));
    }
    initialize(
        Pending::at(directory, name.as_ref())?,
        source,
        exact_bytes,
        || {},
        || {},
    )
}

fn initialize(
    mut pending: Pending,
    source: &mut (impl Read + Seek),
    exact_bytes: usize,
    synced: impl FnOnce(),
    published: impl FnOnce(),
) -> Result<File> {
    source.seek(SeekFrom::Start(0))?;
    let mut source_buffer = [0; 8192];
    let mut remaining = exact_bytes;
    while remaining != 0 {
        let count = remaining.min(source_buffer.len());
        source_exact(source, &mut source_buffer[..count])?;
        pending.file.write_all(&source_buffer[..count])?;
        remaining -= count;
    }
    source_eof(source)?;
    sync(&pending.file, "file_sync")?;
    synced();
    pending.check()?;
    let before = pending.file.metadata()?;
    if before.len() != exact_bytes as u64 {
        return Err(Error::PathChanged);
    }
    pending.file.seek(SeekFrom::Start(0))?;
    source.seek(SeekFrom::Start(0))?;
    let mut file_buffer = [0; 8192];
    remaining = exact_bytes;
    while remaining != 0 {
        let count = remaining.min(source_buffer.len());
        source_exact(source, &mut source_buffer[..count])?;
        pending.file.read_exact(&mut file_buffer[..count])?;
        if source_buffer[..count] != file_buffer[..count] {
            return Err(Error::Readback);
        }
        remaining -= count;
    }
    source_eof(source)?;
    let after = pending.file.metadata()?;
    if (
        before.len(),
        before.mtime(),
        before.mtime_nsec(),
        before.ctime(),
        before.ctime_nsec(),
    ) != (
        after.len(),
        after.mtime(),
        after.mtime_nsec(),
        after.ctime(),
        after.ctime_nsec(),
    ) {
        return Err(Error::PathChanged);
    }
    pending.check()?;
    // Clone before selection: descriptor-allocation failure must remain a
    // prepublication error with the original owned-stage cleanup contract.
    let retained = pending.file.try_clone()?;
    pending.publish(published)?;
    Ok(retained)
}

fn source_exact(source: &mut impl Read, out: &mut [u8]) -> Result<()> {
    source.read_exact(out).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            Error::SourceLength
        } else {
            Error::Io(error)
        }
    })
}

fn source_eof(source: &mut impl Read) -> Result<()> {
    let mut probe = [0];
    loop {
        match source.read(&mut probe) {
            Ok(0) => return Ok(()),
            Ok(_) => return Err(Error::SourceLength),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(Error::Io(error)),
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod crash_tests;
#[cfg(all(test, target_os = "linux"))]
mod tests;
