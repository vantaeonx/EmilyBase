use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use rustix::fs::{AtFlags, Mode, OFlags};

use crate::directory::{Destination, sync};
use crate::{Error, Result};

pub(crate) fn temporary_name() -> Result<OsString> {
    let mut nonce = [0; 16];
    getrandom::fill(&mut nonce).map_err(|_| Error::Randomness)?;
    let hex = nonce
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!(".emilybase-backup-{hex}").into())
}

pub(crate) struct PendingFile {
    pub file: File,
    destination: Destination,
    name: OsString,
    published: bool,
}

impl PendingFile {
    pub fn check(&self) -> Result<()> {
        self.destination.check()?;
        if !self.destination.owns(&self.name, &self.file) {
            return Err(Error::PathChanged);
        }
        Ok(())
    }
}

impl Drop for PendingFile {
    fn drop(&mut self) {
        // Cleanup follows the owned parent, and never removes a substituted entry.
        if !self.published && self.destination.owns(&self.name, &self.file) {
            let _ = rustix::fs::unlinkat(&self.destination.parent, &self.name, AtFlags::empty());
        }
    }
}

/// Write and sync privately; publication is a separate descriptor-relative operation.
pub(crate) fn stage(bytes: &[u8], target: &Path) -> Result<PendingFile> {
    let destination = Destination::open(target)?;
    for _ in 0..32 {
        let name = temporary_name()?;
        match rustix::fs::openat(
            &destination.parent,
            &name,
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        ) {
            Ok(fd) => {
                let mut pending = PendingFile {
                    file: fd.into(),
                    destination,
                    name,
                    published: false,
                };
                pending.file.write_all(bytes)?;
                sync(&pending.file, "backup_file_sync")?;
                return Ok(pending);
            }
            Err(rustix::io::Errno::EXIST) => (),
            Err(error) => return Err(std::io::Error::from(error).into()),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "temporary path collision limit",
    )
    .into())
}

pub(crate) fn publish(mut pending: PendingFile, published: impl FnOnce()) -> Result<()> {
    pending.check()?;
    pending.destination.publish(&pending.name)?;
    pending.published = true;
    published();
    pending
        .destination
        .finish(&pending.file)
        .map_err(Error::PublicationUnknown)
}
