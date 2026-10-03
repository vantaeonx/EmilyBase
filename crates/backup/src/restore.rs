use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use emilybase_transactions::Database;

use crate::{Error, HEADER_SIZE, Report, Result, files, inspect_bytes, publish};

/// Validate, replay and sync privately; publish a complete directory atomically.
pub fn restore(backup: impl AsRef<Path>, target: impl AsRef<Path>) -> Result<Report> {
    restore_with(backup.as_ref(), target.as_ref(), || {}, || {})
}

pub(crate) fn restore_with(
    backup: &Path,
    target: &Path,
    synced: impl FnOnce(),
    published: impl FnOnce(),
) -> Result<Report> {
    let bytes = files::read(backup)?;
    let report = inspect_bytes(&bytes)?;
    let mut pending = PendingDirectory::new(publish::parent(target))?;
    let log = pending.path.join("redo.wal");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(log)?;
    file.write_all(&bytes[HEADER_SIZE..])?;
    file.sync_all()?;
    drop(file);
    let mut database = Database::open_bound(&pending.path, Some(report.database_id))?;
    if database.committed_wal()? != bytes[HEADER_SIZE..] {
        return Err(Error::Format(
            "restored journal differs from verified archive",
        ));
    }
    database.checkpoint()?;
    drop(database);
    File::open(&pending.path)?.sync_all()?;
    synced();
    pending.publish(target, published)?;
    Ok(report)
}

struct PendingDirectory {
    path: PathBuf,
    published: bool,
}

impl PendingDirectory {
    fn new(parent: &Path) -> Result<Self> {
        for _ in 0..32 {
            let path = publish::temporary_name(parent)?;
            let mut builder = DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => {
                    return Ok(Self {
                        path,
                        published: false,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(error) => return Err(error.into()),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "temporary directory collision limit",
        )
        .into())
    }

    fn publish(&mut self, target: &Path, published: impl FnOnce()) -> Result<()> {
        rename_no_replace(&self.path, target)?;
        self.published = true;
        published();
        File::open(publish::parent(target))
            .and_then(|parent| parent.sync_all())
            .map_err(Error::PublicationUnknown)
    }
}

impl Drop for PendingDirectory {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(target_os = "linux")]
fn rename_no_replace(source: &Path, target: &Path) -> Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, source, CWD, target, RenameFlags::NOREPLACE)
        .map_err(|error| Error::Io(error.into()))
}

#[cfg(not(target_os = "linux"))]
fn rename_no_replace(_source: &Path, _target: &Path) -> Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic no-replace directory publication requires Linux",
    )
    .into())
}
