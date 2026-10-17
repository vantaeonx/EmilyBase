//! Explicit offline v2 restoration. Ordinary live startup continues to refuse v2.
use super::{checkpoint, sync};
use crate::{
    AccountFileRootManifest, BundledAccountReport, Error, RegistryBackupReport, Result,
    VerifiedAccountFileArchive, account_bundle, verify_account_file_archive,
};
use emilybase_auth::{accounts::with_restored_private_account_bytes_at, password::PasswordPool};
use emilybase_files::{FileArchiveReport, with_restored_file_archive_at};
use emilybase_storage::{PublishedPrivateDirectory, StagedPrivateDirectory};
use rustix::fs::{AtFlags, Mode, OFlags};
use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
mod tree;

/// Copied restored inventory, not a service owner or current user permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountFileRootReport {
    pub manifest: AccountFileRootManifest,
    pub registry: RegistryBackupReport,
    pub private_accounts: Vec<BundledAccountReport>,
    pub files: Vec<FileArchiveReport>,
}
fn private(file: &File) -> Result<()> {
    let m = file.metadata()?;
    if !m.is_dir() || m.mode() & 0o7777 != 0o700 {
        return Err(Error::Path);
    }
    Ok(())
}
fn directory(parent: &File, name: &str, create: bool) -> Result<File> {
    if create {
        rustix::fs::mkdirat(parent, name, Mode::RUSR | Mode::WUSR | Mode::XUSR)
            .map_err(std::io::Error::from)?;
    }
    let file = File::from(
        rustix::fs::openat(
            parent,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(std::io::Error::from)?,
    );
    selected(parent, name, &file)?;
    Ok(file)
}
fn selected(parent: &File, name: &str, owner: &File) -> Result<()> {
    private(owner)?;
    let actual = owner.metadata()?;
    let visible = rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(std::io::Error::from)?;
    if visible.st_mode & 0o170000 != 0o040000
        || (visible.st_dev, visible.st_ino) != (actual.dev(), actual.ino())
    {
        return Err(Error::Path);
    }
    Ok(())
}
fn roster(parent: &File, expected: &[&str]) -> Result<()> {
    let mut seen = Vec::new();
    for entry in rustix::fs::Dir::read_from(parent).map_err(std::io::Error::from)? {
        let entry = entry.map_err(std::io::Error::from)?;
        let name = entry.file_name().to_str().map_err(|_| Error::Path)?;
        if matches!(name, "." | "..") {
            continue;
        }
        if seen.len() >= expected.len() {
            return Err(Error::ComponentArchive("unexpected restored child"));
        }
        seen.push(name.to_owned());
    }
    seen.sort();
    if seen.iter().map(String::as_str).ne(expected.iter().copied()) {
        return Err(Error::ComponentArchive("restored child roster"));
    }
    Ok(())
}
struct Restore<'v, 'image> {
    view: &'v VerifiedAccountFileArchive<'image>,
    bundle: account_bundle::DecodedBundle<'v>,
    parent: &'v File,
    root: File,
    private: File,
    files: File,
    registry: Option<File>,
    manifest: File,
    manifest_stamp: Metadata,
    manifest_bytes: Vec<u8>,
    manifest_value: AccountFileRootManifest,
    stage: Option<StagedPrivateDirectory>,
    selected: Option<PublishedPrivateDirectory>,
    uncertain_publication: bool,
    proof: Option<tree::Tree>,
    pool: PasswordPool,
    now: u64,
    accounts: Vec<BundledAccountReport>,
    file_reports: Vec<FileArchiveReport>,
}
impl Restore<'_, '_> {
    fn scope(&mut self) -> Result<()> {
        private(self.parent)?;
        private(&self.root)?;
        selected(&self.root, "private", &self.private)?;
        selected(&self.root, "files", &self.files)?;
        selected(
            &self.root,
            "registry",
            self.registry.as_ref().ok_or(Error::Path)?,
        )?;
        roster(&self.root, &["files", "private", "registry", "root.json"])?;
        roster(
            &self.private,
            &self
                .manifest_value
                .private_projects()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )?;
        roster(
            &self.files,
            &self
                .manifest_value
                .file_stores()
                .iter()
                .map(|p| p.project())
                .collect::<Vec<_>>(),
        )?;
        let owned = self.manifest.metadata()?;
        let visible = rustix::fs::statat(&self.root, "root.json", AtFlags::SYMLINK_NOFOLLOW)
            .map_err(std::io::Error::from)?;
        let stamp = |m: &Metadata| {
            (
                m.dev(),
                m.ino(),
                m.len(),
                m.mode(),
                m.nlink(),
                m.mtime(),
                m.mtime_nsec(),
                m.ctime(),
                m.ctime_nsec(),
            )
        };
        if !owned.is_file()
            || owned.mode() & 0o7777 != 0o600
            || owned.nlink() != 1
            || stamp(&owned) != stamp(&self.manifest_stamp)
            || visible.st_mode & 0o170000 != 0o100000
            || (visible.st_dev, visible.st_ino) != (owned.dev(), owned.ino())
        {
            return Err(Error::ComponentArchive(
                "original restored manifest changed",
            ));
        }
        self.manifest.seek(SeekFrom::Start(0))?;
        let mut scratch = [0; 8192];
        for expected in self.manifest_bytes.chunks(scratch.len()) {
            self.manifest.read_exact(&mut scratch[..expected.len()])?;
            if &scratch[..expected.len()] != expected {
                return Err(Error::ComponentArchive("restored manifest bytes"));
            }
        }
        if stamp(&self.manifest.metadata()?) != stamp(&self.manifest_stamp) {
            return Err(Error::Path);
        }
        if let Some(published) = &self.selected {
            published.check()?;
        }
        Ok(())
    }
    fn accounts(&mut self, index: usize) -> Result<()> {
        if index == self.bundle.private.len() {
            return self.files(0);
        }
        let (project, image) = self.bundle.private[index];
        let parent = self.private.try_clone()?;
        with_restored_private_account_bytes_at(
            image,
            project,
            &parent,
            project,
            self.pool.clone(),
            self.now,
            |report| {
                self.accounts.push(BundledAccountReport {
                    project: project.into(),
                    inventory: report.clone(),
                });
                checkpoint("joined_restore_private_ready");
                self.accounts(index + 1)
            },
        )??;
        Ok(())
    }
    fn files(&mut self, index: usize) -> Result<()> {
        let view = self.view;
        if index == view.files().len() {
            return self.publish();
        }
        let binding = &view.manifest().file_stores()[index];
        let image = view
            .file_bytes(index)
            .ok_or(Error::ComponentArchive("file image missing"))?;
        let parent = self.files.try_clone()?;
        with_restored_file_archive_at(
            image,
            view.files()[index].project(),
            &parent,
            binding.project(),
            |report| {
                self.file_reports.push(report.clone());
                checkpoint("joined_restore_file_ready");
                self.files(index + 1)
            },
        )??;
        Ok(())
    }
    fn publish(&mut self) -> Result<()> {
        self.scope()?;
        // Capture while every original component writer/payload reader is held.
        // These independent original readers outlive nested component unwind.
        self.proof = Some(tree::Tree::capture(&self.root)?);
        checkpoint("joined_restore_prepared");
        self.check()?;
        sync(&self.private, "joined_restore_private_sync")?;
        sync(&self.files, "joined_restore_files_sync")?;
        sync(&self.manifest, "joined_restore_manifest_confirm_sync")?;
        self.check()?;
        let stage = self.stage.take().ok_or(Error::Path)?;
        stage.check()?;
        match stage.publish() {
            Ok(selected) => self.selected = Some(selected),
            Err(error @ emilybase_storage::Error::PublicationUnknown(_)) => {
                self.uncertain_publication = true;
                return Err(error.into());
            }
            Err(error) => return Err(error.into()),
        }
        checkpoint("joined_restore_selected");
        self.check()?;
        Ok(())
    }
    fn check(&mut self) -> Result<()> {
        self.scope()?;
        self.proof.as_mut().ok_or(Error::Path)?.check()?;
        self.scope()
    }
}

/// Verify a complete sensitive joined archive, prepare all components under one
/// original private locked root, and select it without replacing existing targets.
/// Original component writers span common selection. Independent original physical
/// readers and the common lock span final proof after nested writers unwind.
/// v1 live startup is unchanged; opening this v2 root needs explicit future adoption.
pub fn restore_account_file_archive_bytes_at(
    bytes: &[u8],
    parent: &File,
    name: &str,
    pool: PasswordPool,
    now: u64,
) -> Result<AccountFileRootReport> {
    if now > i64::MAX as u64 {
        return Err(Error::BundleRoot("trusted timestamp"));
    }
    let view = verify_account_file_archive(bytes)?;
    let bundle = account_bundle::decode(view.account_bytes())?;
    let manifest_value = AccountFileRootManifest::new(
        view.manifest().private_projects().to_vec(),
        view.manifest().file_stores().to_vec(),
        now,
    )?;
    let manifest_bytes = manifest_value.to_bytes()?;
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.len() > 255
        || name.contains('/')
        || name.as_bytes().contains(&0)
    {
        return Err(Error::Path);
    }
    private(parent)?;
    let stage = StagedPrivateDirectory::at(parent, name)?;
    let root = stage.directory().try_clone()?;
    root.try_lock().map_err(|e| match e {
        std::fs::TryLockError::WouldBlock => Error::Busy,
        std::fs::TryLockError::Error(e) => e.into(),
    })?;
    let private = directory(&root, "private", true)?;
    let files = directory(&root, "files", true)?;
    let mut manifest = File::from(
        rustix::fs::openat(
            &root,
            "root.json",
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(std::io::Error::from)?,
    );
    manifest.write_all(&manifest_bytes)?;
    sync(&manifest, "joined_restore_manifest_sync")?;
    let manifest_stamp = manifest.metadata()?;
    let mut restore = Restore {
        view: &view,
        bundle,
        parent,
        root,
        private,
        files,
        registry: None,
        manifest,
        manifest_stamp,
        manifest_bytes,
        manifest_value,
        stage: Some(stage),
        selected: None,
        uncertain_publication: false,
        proof: None,
        pool,
        now,
        accounts: Vec::new(),
        file_reports: Vec::new(),
    };
    let registry = restore.bundle.registry;
    let owner = restore.root.try_clone()?;
    let result = (|| -> Result<AccountFileRootReport> {
        let registry_report = crate::with_restored_registry_backup_bytes_at(
            registry,
            &owner,
            "registry",
            |report| -> Result<RegistryBackupReport> {
                restore.registry = Some(directory(&restore.root, "registry", false)?);
                checkpoint("joined_restore_registry_ready");
                restore.accounts(0)?;
                Ok(report.clone())
            },
        )??;
        // Component writers have finished their own proofs. Keep the common lock,
        // all actual original physical readers and exact root manifest until return.
        checkpoint("joined_restore_finalizing");
        restore.check()?;
        sync(parent, "joined_restore_confirm_parent_sync")?;
        restore.check()?;
        Ok(AccountFileRootReport {
            manifest: restore.manifest_value.clone(),
            registry: registry_report,
            private_accounts: restore.accounts.clone(),
            files: restore.file_reports.clone(),
        })
    })();
    let selected = restore.selected.is_some() || restore.uncertain_publication;
    drop(restore);
    drop(owner);
    let report = if selected {
        result.map_err(|e| Error::ComponentRestoreUnknown(Box::new(e)))
    } else {
        result
    }?;
    Ok(report)
}
