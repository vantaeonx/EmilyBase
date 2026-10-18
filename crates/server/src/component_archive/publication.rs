//! Private original-inode publication/inspection, never common-root restoration.
use super::*;
use crate::{AccountFileSnapshot, metadata};
use rustix::fs::{AtFlags, Mode, OFlags};
use std::ffi::OsString;
use std::fs::{File, Metadata};
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::path::{Path, PathBuf};

/// Copied checked counts/identities only. Not a source/destination owner, current
/// user permission, file lease or common restored root. Debug omits names/data.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountFileArchiveReport {
    manifest: AccountFileRootManifest,
    accounts: AccountBundleReport,
    references: usize,
    objects: usize,
    copied_bytes: usize,
    encoded_bytes: usize,
}
impl AccountFileArchiveReport {
    pub fn manifest(&self) -> &AccountFileRootManifest {
        &self.manifest
    }
    pub fn accounts(&self) -> &AccountBundleReport {
        &self.accounts
    }
    pub const fn references(&self) -> usize {
        self.references
    }
    pub const fn physical_objects(&self) -> usize {
        self.objects
    }
    pub const fn copied_bytes(&self) -> usize {
        self.copied_bytes
    }
    pub const fn encoded_bytes(&self) -> usize {
        self.encoded_bytes
    }
    fn from_snapshot(snapshot: &AccountFileSnapshot, encoded_bytes: usize) -> Self {
        let report = snapshot.accounts().report();
        Self {
            manifest: snapshot.manifest().clone(),
            accounts: AccountBundleReport {
                registry: report.registry.clone(),
                private_accounts: report.private_accounts.clone(),
                archive_bytes: snapshot.accounts().bundle_bytes().len(),
            },
            references: snapshot.files().iter().map(|f| f.files().len()).sum(),
            objects: snapshot.physical_objects(),
            copied_bytes: snapshot.copied_bytes(),
            encoded_bytes,
        }
    }
    fn from_verified(view: &VerifiedAccountFileArchive<'_>) -> Self {
        Self {
            manifest: view.manifest().clone(),
            accounts: view.account_report().clone(),
            references: view.files().iter().map(|f| f.files().len()).sum(),
            objects: view.physical_objects(),
            copied_bytes: view.copied_bytes(),
            encoded_bytes: view.encoded_bytes(),
        }
    }
}
impl std::fmt::Debug for AccountFileArchiveReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountFileArchiveReport")
            .field("file_stores", &self.manifest.file_stores().len())
            .field("references", &self.references)
            .field("physical_objects", &self.objects)
            .field("encoded_bytes", &self.encoded_bytes)
            .finish_non_exhaustive()
    }
}

pub(super) struct Target {
    pub(super) parent: File,
    path: PathBuf,
    name: OsString,
}
impl Target {
    pub(super) fn new(path: &Path) -> Result<Self> {
        if path.as_os_str().as_bytes().len() > 4096 {
            return Err(Error::Path);
        }
        let name = path.file_name().ok_or(Error::Path)?.to_os_string();
        if name.as_bytes().is_empty() || name.as_bytes().len() > 255 || name.as_bytes().contains(&0)
        {
            return Err(Error::Path);
        }
        let parent = crate::registry_files::parent(path);
        let path = if parent.is_absolute() {
            parent.to_path_buf()
        } else {
            std::env::current_dir()?.join(parent)
        };
        let parent = metadata::open_directory(&path)?;
        let value = Self { parent, path, name };
        value.check()?;
        Ok(value)
    }
    fn check(&self) -> Result<()> {
        metadata::owned_directory(&self.path, &self.parent)
    }
    pub(super) fn visible(&self, expected: &Metadata) -> Result<()> {
        self.check()?;
        let visible = rustix::fs::statat(&self.parent, &self.name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(io::Error::from)?;
        if (visible.st_dev, visible.st_ino) != (expected.dev(), expected.ino())
            || visible.st_mode & 0o170000 != 0o100000
            || !matches!(visible.st_mode & 0o7777, 0o600 | 0o400)
            || visible.st_nlink != 1
        {
            return Err(Error::Path);
        }
        Ok(())
    }
    pub(super) fn open(&self) -> Result<File> {
        let fd = rustix::fs::openat(
            &self.parent,
            &self.name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(io::Error::from)?;
        Ok(fd.into())
    }
}
pub(super) fn private(file: &File) -> Result<Metadata> {
    let m = file.metadata()?;
    if !m.is_file() || !matches!(m.mode() & 0o7777, 0o600 | 0o400) || m.nlink() != 1 {
        return Err(Error::Path);
    }
    if m.len() > MAX_ACCOUNT_FILE_ARCHIVE_BYTES as u64 {
        return Err(Error::Limit);
    }
    Ok(m)
}
pub(super) fn unchanged(file: &File, before: &Metadata) -> Result<()> {
    let m = private(file)?;
    let key = |m: &Metadata| {
        (
            m.dev(),
            m.ino(),
            m.len(),
            m.mode(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )
    };
    if key(before) != key(&m) {
        return Err(Error::Path);
    }
    Ok(())
}
struct Guarded<'a> {
    reader: AccountFileArchiveReader<'a>,
    target: &'a Target,
    outside: &'a [emilybase_storage::OutsideDirectoryGuard],
    started: bool,
    eof: bool,
}
impl Guarded<'_> {
    fn check(&self) -> io::Result<()> {
        check_target(self.target, self.outside)
            .map_err(|_| io::Error::other("private archive containing directory changed"))
    }
}
impl Read for Guarded<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        let n = self.reader.read(out)?;
        self.check()?;
        if !self.started && n != 0 {
            self.started = true;
            checkpoint("joined_archive_copying");
            self.check()?;
        }
        if self.started && !self.eof && n == 0 && !out.is_empty() {
            self.eof = true;
            checkpoint("joined_archive_copied");
            self.check()?;
        }
        Ok(n)
    }
}
impl Seek for Guarded<'_> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.check()?;
        let at = self.reader.seek(from)?;
        self.check()?;
        Ok(at)
    }
}

/// Trusted native maintenance. Use an existing private backup directory outside
/// every live inventory. Retains the actual selected inode/parent through exact
/// bounded readback and final fsync/proofs. Existing names are never replaced.
/// Late uncertainty preserves the destination; inspect rather than blind retry.
/// No source lease, user authorization or common restored root is returned.
pub fn publish_account_file_archive(
    snapshot: &AccountFileSnapshot,
    path: impl AsRef<Path>,
) -> Result<AccountFileArchiveReport> {
    let target = Target::new(path.as_ref())?;
    Ok(publish_retained(snapshot, &target, &[])?.report)
}
pub(super) struct RetainedArchive {
    file: File,
    before: Metadata,
    pub(super) report: AccountFileArchiveReport,
}
impl RetainedArchive {
    pub(super) fn check(
        &self,
        target: &Target,
        outside: &[emilybase_storage::OutsideDirectoryGuard],
    ) -> Result<()> {
        check_target(target, outside)?;
        unchanged(&self.file, &self.before)?;
        target.visible(&self.before)
    }
}
pub(super) fn check_target(
    target: &Target,
    outside: &[emilybase_storage::OutsideDirectoryGuard],
) -> Result<()> {
    target.check()?;
    for guard in outside {
        guard.check()?;
    }
    Ok(())
}
pub(super) fn publish_retained(
    snapshot: &AccountFileSnapshot,
    target: &Target,
    outside: &[emilybase_storage::OutsideDirectoryGuard],
) -> Result<RetainedArchive> {
    let reader = AccountFileArchiveReader::from_snapshot(snapshot)?;
    let length = reader.encoded_bytes();
    let mut source = Guarded {
        reader,
        target,
        outside,
        started: false,
        eof: false,
    };
    checkpoint("joined_archive_prepared");
    check_target(target, outside)?;
    let mut file = match emilybase_storage::publish_private_reader_at_retained(
        &target.parent,
        &target.name,
        &mut source,
        length,
        MAX_ACCOUNT_FILE_ARCHIVE_BYTES,
    ) {
        Ok(file) => file,
        Err(error @ emilybase_storage::Error::PublicationUnknown(_)) => {
            return Err(Error::ComponentPublicationUnknown(Box::new(error.into())));
        }
        Err(error) => return Err(error.into()),
    };
    (|| {
        let before = private(&file)?;
        checkpoint("joined_archive_selected");
        check_target(target, outside)?;
        target.visible(&before)?;
        unchanged(&file, &before)?;
        if before.len() != length as u64 {
            return Err(Error::ComponentArchive("selected archive length"));
        }
        file.seek(SeekFrom::Start(0))?;
        source.seek(SeekFrom::Start(0))?;
        let mut expected = [0; 8192];
        let mut actual = [0; 8192];
        let mut left = length;
        while left != 0 {
            let n = left.min(actual.len());
            source.read_exact(&mut expected[..n])?;
            file.read_exact(&mut actual[..n])?;
            if expected[..n] != actual[..n] {
                return Err(Error::ComponentArchive("selected archive readback"));
            }
            left -= n;
        }
        eof(&mut file)?;
        eof(&mut source)?;
        unchanged(&file, &before)?;
        target.visible(&before)?;
        checkpoint("joined_archive_readback");
        check_target(target, outside)?;
        sync(&file, "joined_archive_file_ack_sync")?;
        sync(&target.parent, "joined_archive_parent_ack_sync")?;
        checkpoint("joined_archive_acked");
        check_target(target, outside)?;
        unchanged(&file, &before)?;
        target.visible(&before)?;
        Ok(RetainedArchive {
            file,
            before,
            report: AccountFileArchiveReport::from_snapshot(snapshot, length),
        })
    })()
    .map_err(|error| Error::ComponentPublicationUnknown(Box::new(error)))
}

/// Bounded readonly private regular-file inspection through the actual retained
/// inode/parent. Allocates the encoded image and nested models under existing
/// bounds; this is not a whole-process memory reservation or source authority.
pub fn inspect_account_file_archive(path: impl AsRef<Path>) -> Result<AccountFileArchiveReport> {
    let target = Target::new(path.as_ref())?;
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
    checkpoint("joined_archive_input_read");
    unchanged(&file, &before)?;
    target.visible(&before)?;
    let view = verify_account_file_archive(&bytes)?;
    let report = AccountFileArchiveReport::from_verified(&view);
    checkpoint("joined_archive_input_verified");
    unchanged(&file, &before)?;
    target.visible(&before)?;
    Ok(report)
}
pub(super) fn eof(reader: &mut impl Read) -> Result<()> {
    let mut probe = [0; 1];
    loop {
        match reader.read(&mut probe) {
            Ok(0) => return Ok(()),
            Ok(_) => return Err(Error::ComponentArchive("trailing file bytes")),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
}
fn sync(file: &File, _point: &'static str) -> Result<()> {
    #[cfg(test)]
    crate::durability::fail(_point)?;
    file.sync_all()?;
    #[cfg(test)]
    crate::durability::fail(&format!("{_point}_after"))?;
    Ok(())
}
pub(super) fn checkpoint(_point: &str) {
    #[cfg(test)]
    crate::durability::checkpoint(_point);
}
