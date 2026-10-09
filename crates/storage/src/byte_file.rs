//! Bounded private byte publication using the original owned no-replace stage.
use crate::{
    Error, Result,
    creation::{Pending, sync},
};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Native filesystem maintenance only, not database/user authority. Existing
/// names are never replaced. PublicationUnknown requires explicit inspection.
pub fn publish_private_file(path: impl AsRef<Path>, bytes: &[u8], maximum: usize) -> Result<()> {
    publish_with(path.as_ref(), bytes, maximum, || {}, || {})
}
/// Publish one component in an already-owned directory; pathname moves cannot
/// redirect the operation. The caller supplies filesystem, not user, authority.
pub fn publish_private_file_at(
    directory: &std::fs::File,
    name: impl AsRef<std::ffi::OsStr>,
    bytes: &[u8],
    maximum: usize,
) -> Result<()> {
    publish_private_file_at_retained(directory, name, bytes, maximum).map(drop)
}
/// The same publication, retaining the actual selected inode for caller
/// readback. The handle is positioned at EOF; it is not a namespace lease.
/// An uncertain result does not return authority to remove or replace a file.
pub fn publish_private_file_at_retained(
    directory: &std::fs::File,
    name: impl AsRef<std::ffi::OsStr>,
    bytes: &[u8],
    maximum: usize,
) -> Result<std::fs::File> {
    if bytes.len() > maximum {
        return Err(Error::FileLength(bytes.len() as u64));
    }
    initialize(Pending::at(directory, name.as_ref())?, bytes, || {}, || {})
}
pub(crate) fn publish_with(
    path: &Path,
    bytes: &[u8],
    maximum: usize,
    synced: impl FnOnce(),
    published: impl FnOnce(),
) -> Result<()> {
    if bytes.len() > maximum {
        return Err(Error::FileLength(bytes.len() as u64));
    }
    initialize(Pending::new(path)?, bytes, synced, published).map(drop)
}
fn initialize(
    mut pending: Pending,
    bytes: &[u8],
    synced: impl FnOnce(),
    published: impl FnOnce(),
) -> Result<std::fs::File> {
    pending.file.write_all(bytes)?;
    sync(&pending.file, "file_sync")?;
    synced();
    pending.check()?;
    if pending.file.metadata()?.len() != bytes.len() as u64 {
        return Err(Error::PathChanged);
    }
    pending.file.seek(SeekFrom::Start(0))?;
    let mut buffer = [0; 8192];
    for chunk in bytes.chunks(buffer.len()) {
        pending.file.read_exact(&mut buffer[..chunk.len()])?;
        if &buffer[..chunk.len()] != chunk {
            return Err(Error::PathChanged);
        }
    }
    // Clone before selection so a descriptor-allocation failure is still a
    // prepublication error with the original owned-stage cleanup contract.
    let retained = pending.file.try_clone()?;
    pending.publish(published)?;
    Ok(retained)
}
