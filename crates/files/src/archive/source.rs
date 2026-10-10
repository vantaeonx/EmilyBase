//! Bind an immutable restore image to its retained native input selection.
use super::FileArchiveReport;
use super::publication::{Target, private, read_image, unchanged};
use super::restore::{RestoreBoundary, restore_checked};
use crate::{Error, Result};
use emilybase_object_storage::ProjectId;
use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

struct Source {
    target: Target,
    file: File,
    baseline: Metadata,
}
impl Source {
    fn read(path: &Path) -> Result<(Self, Vec<u8>)> {
        let target = Target::new(path)?;
        let mut file = target.open_readonly()?;
        let baseline = private(&file)?;
        target.visible(&baseline)?;
        let bytes = read_image(&mut file, baseline.len() as usize)?;
        let mut source = Self {
            target,
            file,
            baseline,
        };
        source.check(&bytes)?;
        Ok((source, bytes))
    }
    fn check(&mut self, bytes: &[u8]) -> Result<()> {
        unchanged(&self.file, &self.baseline)?;
        self.target.visible(&self.baseline)?;
        self.file.seek(SeekFrom::Start(0))?;
        let mut scratch = [0; 8192];
        for expected in bytes.chunks(scratch.len()) {
            self.file.read_exact(&mut scratch[..expected.len()])?;
            if scratch[..expected.len()] != *expected {
                return Err(Error::Archive);
            }
        }
        unchanged(&self.file, &self.baseline)?;
        self.target.visible(&self.baseline)
    }
}

/// Restore a complete pair from an operator-selected private regular file.
/// The original readonly file and parent descriptors stay held across common
/// root selection; exact input bytes/identity/stability are rechecked. No-follow,
/// single-link0400/0600 and length admission precede image allocation. Any late
/// source failure is OutcomeUnknown and leaves the selected common root intact.
/// This does not authenticate archive origin or grant account/user authority.
pub fn restore_file_archive_file(
    path: impl AsRef<Path>,
    project: ProjectId,
    destination: impl AsRef<Path>,
) -> Result<FileArchiveReport> {
    restore_file_with(path.as_ref(), project, destination.as_ref(), || {}, |_| {})
}
fn restore_file_with(
    path: &Path,
    project: ProjectId,
    destination: &Path,
    read: impl FnOnce(),
    boundary: impl FnMut(RestoreBoundary),
) -> Result<FileArchiveReport> {
    let (mut source, bytes) = Source::read(path)?;
    read();
    restore_checked(&bytes, project, destination, boundary, || {
        source.check(&bytes)
    })
}

#[cfg(test)]
mod tests;
