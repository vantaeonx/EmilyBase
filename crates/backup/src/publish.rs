use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

pub(crate) fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

pub(crate) fn temporary_name(parent: &Path) -> Result<PathBuf> {
    let mut nonce = [0; 16];
    getrandom::fill(&mut nonce).map_err(|_| Error::Randomness)?;
    let hex = nonce
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(parent.join(format!(".emilybase-backup-{hex}")))
}

pub(crate) struct PendingFile {
    pub path: PathBuf,
    removed: bool,
}

impl Drop for PendingFile {
    fn drop(&mut self) {
        if !self.removed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Write and sync privately; publication is a separate no-clobber operation.
pub(crate) fn stage(bytes: &[u8], parent: &Path) -> Result<PendingFile> {
    for _ in 0..32 {
        let path = temporary_name(parent)?;
        let mut options = OpenOptions::new();
        options.write(true).read(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut file) => {
                let pending = PendingFile {
                    path,
                    removed: false,
                };
                file.write_all(bytes)?;
                file.sync_all()?;
                return Ok(pending);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "temporary path collision limit",
    )
    .into())
}

pub(crate) fn publish(
    mut pending: PendingFile,
    target: &Path,
    published: impl FnOnce(),
) -> Result<()> {
    fs::hard_link(&pending.path, target)?;
    published();
    let result = (|| {
        fs::remove_file(&pending.path)?;
        pending.removed = true;
        File::open(parent(target))?.sync_all()
    })();
    result.map_err(Error::PublicationUnknown)
}
