//! Pin actual private WAL selection while its original owner performs the write.
use super::*;
use std::os::unix::fs::MetadataExt;

pub(super) struct PrivateWal {
    directory: File,
    file: File,
}
impl PrivateWal {
    pub(super) fn new(account: &AccountStore, directory: &File) -> Result<Self> {
        account.check_directory_owner(directory)?;
        let file = File::from(
            rustix::fs::openat(
                directory,
                "redo.wal",
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::NONBLOCK
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(std::io::Error::from)?,
        );
        let guard = Self {
            directory: directory.try_clone()?,
            file,
        };
        guard.check()?;
        account.check_directory_owner(directory)?;
        guard.check()?;
        Ok(guard)
    }
    pub(super) fn check(&self) -> Result<()> {
        let actual = self.file.metadata()?;
        let selected = rustix::fs::statat(
            &self.directory,
            "redo.wal",
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(std::io::Error::from)?;
        if !actual.is_file()
            || actual.nlink() != 1
            || actual.mode() & 0o077 != 0
            || selected.st_mode & 0o170000 != 0o100000
            || selected.st_mode & 0o077 != 0
            || selected.st_nlink != 1
            || (actual.dev(), actual.ino()) != (selected.st_dev, selected.st_ino)
        {
            return Err(Error::BundleRoot("original private WAL selection changed"));
        }
        // Legitimate original append changes length/timestamps. This guard pins
        // inode/selection, not a stale pre-commit byte image or namespace lease.
        Ok(())
    }
}
