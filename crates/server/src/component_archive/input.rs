//! Sensitive immutable input and actual original source retained through work.
use super::publication::{Target, checkpoint, eof, private, unchanged};
use crate::{Error, Result};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// No descriptor escapes. Trusted work must confirm source at its own preparation
/// and last locked proof, rather than waiting until its selected owners close.
pub(crate) fn with_retained<T>(
    path: &Path,
    work: impl FnOnce(&[u8], &mut dyn FnMut() -> Result<()>) -> Result<T>,
) -> Result<T> {
    let target = Target::new(path)?;
    let mut file = target.open()?;
    let before = private(&file)?;
    target.visible(&before)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(before.len() as usize)
        .map_err(|_| Error::Limit)?;
    bytes.resize(before.len() as usize, 0);
    file.read_exact(&mut bytes)?;
    eof(&mut file)?;
    checkpoint("joined_restore_input_read");
    let mut confirm = || -> Result<()> {
        unchanged(&file, &before)?;
        target.visible(&before)?;
        file.seek(SeekFrom::Start(0))?;
        let mut scratch = [0; 8192];
        for expected in bytes.chunks(scratch.len()) {
            file.read_exact(&mut scratch[..expected.len()])?;
            if scratch[..expected.len()] != *expected {
                return Err(Error::ComponentArchive("original restore input changed"));
            }
        }
        eof(&mut file)?;
        unchanged(&file, &before)?;
        target.visible(&before)
    };
    confirm()?;
    work(&bytes, &mut confirm)
}
