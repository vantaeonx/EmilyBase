//! Offline, single-root restoration of an explicit registry/private inventory.
use super::{checkpoint, pending::Pending, read_bounded, restore_bytes_under, sync};
use crate::{
    BundledAccountReport, Error, MAX_PROJECTS, ProjectStore, RegistryBackupReport, Result,
    account_bundle, metadata,
};
use emilybase_auth::{
    accounts::{AccountStore, inspect_private_account_backup_bytes, restore_private_account_bytes},
    password::PasswordPool,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, TryLockError};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

const MANIFEST_LIMIT: usize = 8192;
const PREFIX: &str = ".emilybase-account-restore-";
mod initialize;
pub use initialize::initialize_account_root;
mod live;
pub use live::{AccountRoot, MAX_ACTIVE_PRIVATE_STORES};

/// Experimental root layout metadata, never proof of capture or authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountBundleRootManifest {
    pub version: u16,
    pub private_projects: Vec<String>,
    pub reset_at: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    payload: AccountBundleRootManifest,
    checksum: u32,
}
/// Counts/identities only. It does not expose rows, verifiers or session scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountBundleRootReport {
    pub registry: RegistryBackupReport,
    pub private_accounts: Vec<BundledAccountReport>,
    pub reset_at: u64,
}

fn manifest_bytes(manifest: &AccountBundleRootManifest) -> Result<Vec<u8>> {
    if manifest.version != 1
        || manifest.reset_at > i64::MAX as u64
        || manifest.private_projects.len() > MAX_PROJECTS
        || manifest
            .private_projects
            .iter()
            .any(|id| !emilybase_auth::valid_project_id(id))
        || manifest.private_projects.windows(2).any(|p| p[0] >= p[1])
    {
        return Err(Error::BundleRoot("manifest inventory"));
    }
    let payload = serde_json::to_vec(manifest).map_err(|_| Error::BundleRoot("manifest JSON"))?;
    let bytes = serde_json::to_vec(&Envelope {
        payload: manifest.clone(),
        checksum: crc32fast::hash(&payload),
    })
    .map_err(|_| Error::BundleRoot("manifest JSON"))?;
    if bytes.len() > MANIFEST_LIMIT {
        return Err(Error::Limit);
    }
    Ok(bytes)
}

/// Bounded canonical manifest inspection without filesystem or access authority.
pub fn inspect_account_bundle_root_manifest_bytes(
    bytes: &[u8],
) -> Result<AccountBundleRootManifest> {
    if bytes.len() > MANIFEST_LIMIT {
        return Err(Error::Limit);
    }
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|_| Error::BundleRoot("manifest JSON"))?;
    // Exact canonical encoding also checks the CRC and rejects duplicate fields,
    // trailing bytes, alternate key order/escaping and unrecognized fields.
    if manifest_bytes(&envelope.payload)? != bytes {
        return Err(Error::BundleRoot("manifest encoding or checksum"));
    }
    Ok(envelope.payload)
}

/// Trusted offline restore. No target is selected until every private scope is
/// freshly reset. Existing targets are never replaced; old API keys are preserved.
pub fn restore_account_bundle(
    archive: impl AsRef<Path>,
    target: impl AsRef<Path>,
    pool: PasswordPool,
    now: u64,
) -> Result<AccountBundleRootReport> {
    validate_time(now)?;
    let bytes = read_bounded(archive.as_ref(), crate::MAX_ACCOUNT_BUNDLE_BYTES)?;
    restore_account_bundle_bytes(&bytes, target, pool, now)
}

/// Same protocol for sensitive in-memory images, without intermediate archives.
pub fn restore_account_bundle_bytes(
    bytes: &[u8],
    target: impl AsRef<Path>,
    pool: PasswordPool,
    now: u64,
) -> Result<AccountBundleRootReport> {
    validate_time(now)?;
    let archive = account_bundle::decode(bytes)?;
    let manifest = AccountBundleRootManifest {
        version: 1,
        private_projects: archive.private.iter().map(|(id, _)| (*id).into()).collect(),
        reset_at: now,
    };
    let encoded = manifest_bytes(&manifest)?;
    let mut pending = Pending::directory_with_prefix(target.as_ref(), PREFIX)?;
    lock(&pending.owner)?;
    let root = pending.path();
    restore_bytes_under(archive.registry, &pending.owner)?;
    checkpoint("bundle_restore_registry_prepared");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(root.join("private"))?;
    let mut expected = Vec::with_capacity(archive.private.len());
    for (project, image) in &archive.private {
        let path = root.join("private").join(project);
        restore_private_account_bytes(image, &path, project, pool.clone(), now)?;
        // Bind final validation to the exact prepared history, including its
        // freshly generated incarnation; logical IDs/counts alone cannot do this.
        let mut store = AccountStore::open(&path, project, pool.clone())?;
        expected.push(hash(&store.backup_image()?));
        drop(store);
        checkpoint("bundle_restore_private_prepared");
    }
    sync(
        &metadata::open_directory(&root.join("private"))?,
        "bundle_restore_private_sync",
    )?;
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join("root.json"))?;
    file.write_all(&encoded)?;
    sync(&file, "bundle_restore_manifest_sync")?;
    checkpoint("bundle_restore_manifest_synced");
    let state = match inspect_owned(&root, &pending.owner, pool) {
        Ok(state) => state,
        Err(error) => {
            pending.retain();
            return Err(error);
        }
    };
    if state.registry != archive.registry
        || state.private_hashes != expected
        || state.report.registry != archive.report.registry
        || state.report.reset_at != now
    {
        pending.retain();
        return Err(Error::BundleRoot("prepared history changed"));
    }
    sync(&pending.owner, "bundle_restore_stage_sync")?;
    checkpoint("bundle_restore_stage_synced");
    if let Err(error) = check_manifest(&root, &file, &encoded)
        .and_then(|()| check_contents(&root, &pending.owner, &state.contents))
    {
        pending.retain();
        return Err(error);
    }
    pending.publish()?;
    checkpoint("bundle_restore_renamed");
    pending.finish("bundle_restore_parent_sync")?;
    checkpoint("bundle_restore_parent_synced");
    Ok(state.report)
}

fn check_manifest(root: &Path, file: &File, encoded: &[u8]) -> Result<()> {
    let selected = fs::symlink_metadata(root.join("root.json"))?;
    let owned = file.metadata()?;
    if (selected.dev(), selected.ino()) != (owned.dev(), owned.ino())
        || read_bounded(&root.join("root.json"), MANIFEST_LIMIT)? != encoded
    {
        return Err(Error::BundleRoot("prepared manifest changed"));
    }
    Ok(())
}

fn validate_time(now: u64) -> Result<()> {
    if now > i64::MAX as u64 {
        Err(Error::BundleRoot("trusted reset time"))
    } else {
        Ok(())
    }
}
fn lock(owner: &File) -> Result<()> {
    match owner.try_lock() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(Error::Busy),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}
fn descriptor(owner: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}/.", owner.as_raw_fd()))
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn names(path: &Path) -> Result<BTreeSet<String>> {
    let mut result = BTreeSet::new();
    for entry in fs::read_dir(path)? {
        let name = entry?.file_name().into_string().map_err(|_| Error::Path)?;
        if result.len() >= MAX_PROJECTS.max(3) {
            return Err(Error::Limit);
        }
        result.insert(name);
    }
    Ok(result)
}

struct State {
    report: AccountBundleRootReport,
    registry: Vec<u8>,
    private_hashes: Vec<[u8; 32]>,
    contents: Contents,
}
struct Contents {
    registry: ProjectStore,
    accounts: Vec<AccountStore>,
    account_owners: Vec<File>,
    private_owner: File,
    manifest_owner: File,
    encoded: Vec<u8>,
    manifest: AccountBundleRootManifest,
    ids: BTreeSet<String>,
}
// Shared retained-owner checks also run at the final initialization/restore
// boundary. Never replace these with a manifest-only final comparison.
fn check_contents(path: &Path, owner: &File, contents: &Contents) -> Result<()> {
    metadata::owned_directory(path, owner)?;
    let root = descriptor(owner);
    if names(&root)? != ["private".into(), "registry".into(), "root.json".into()].into()
        || names(&root.join("registry"))? != contents.ids
        || names(&root.join("private"))?
            != contents.manifest.private_projects.iter().cloned().collect()
    {
        return Err(Error::BundleRoot("service root inventory changed"));
    }
    metadata::owned_directory(&root.join("private"), &contents.private_owner)?;
    for (id, owner) in contents
        .manifest
        .private_projects
        .iter()
        .zip(&contents.account_owners)
    {
        metadata::owned_directory(&root.join("private").join(id), owner)?;
    }
    check_manifest(&root, &contents.manifest_owner, &contents.encoded)?;
    contents.registry.list()?;
    Ok(())
}
fn inspect_owned(path: &Path, owner: &File, pool: PasswordPool) -> Result<State> {
    inspect_owned_with(path, owner, pool, |_, _| Ok(()))
}
fn inspect_owned_with(
    path: &Path,
    owner: &File,
    pool: PasswordPool,
    captured: impl FnMut(&str, &[u8]) -> Result<()>,
) -> Result<State> {
    inspect_owned_limited(path, owner, pool, MAX_PROJECTS, captured)
}
fn inspect_owned_limited(
    path: &Path,
    owner: &File,
    pool: PasswordPool,
    private_limit: usize,
    mut captured: impl FnMut(&str, &[u8]) -> Result<()>,
) -> Result<State> {
    metadata::owned_directory(path, owner)?;
    let root = descriptor(owner);
    let expected_root = BTreeSet::from(["private".into(), "registry".into(), "root.json".into()]);
    if names(&root)? != expected_root {
        return Err(Error::BundleRoot("root entries"));
    }
    let fd = rustix::fs::open(
        root.join("root.json"),
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    let mut manifest_owner: File = fd.into();
    let encoded = super::read_file(&mut manifest_owner, MANIFEST_LIMIT)?;
    let manifest = inspect_account_bundle_root_manifest_bytes(&encoded)?;
    // Service admission occurs before opening any database or registry owner.
    if manifest.private_projects.len() > private_limit {
        return Err(Error::Limit);
    }
    let private = root.join("private");
    let private_owner = metadata::open_directory(&private)?;
    let roster: BTreeSet<_> = manifest.private_projects.iter().cloned().collect();
    if names(&private)? != roster {
        return Err(Error::BundleRoot("private entries"));
    }
    let mut registry = ProjectStore::open_existing(root.join("registry"))?;
    let projects = registry.list()?;
    let ids: BTreeSet<_> = projects.into_iter().map(|p| p.id).collect();
    if names(&root.join("registry"))? != ids || !roster.is_subset(&ids) {
        return Err(Error::BundleRoot("registry entries or private scope"));
    }
    let mut accounts = Vec::with_capacity(roster.len());
    let mut account_owners = Vec::with_capacity(roster.len());
    for project in &manifest.private_projects {
        account_owners.push(metadata::open_directory(&private.join(project))?);
        accounts.push(AccountStore::open(
            private.join(project),
            project,
            pool.clone(),
        )?);
    }
    checkpoint("bundle_restore_owners_locked");
    // Every private owner exists before the first data prefix; the registry
    // callback retains every data owner through this complete private capture.
    let (image, reports, hashes) = registry.capture_registry_with(|image| {
        let report = crate::inspect_registry_backup_bytes(&image)?;
        let mut identities: BTreeSet<_> = report.projects.iter().map(|p| p.database_id).collect();
        let mut reports = Vec::with_capacity(accounts.len());
        let mut hashes = Vec::with_capacity(accounts.len());
        let mut total = account_bundle::HEADER
            .checked_add(image.len())
            .ok_or(Error::Limit)?;
        for account in &mut accounts {
            let bytes = account.backup_image()?;
            total = account_bundle::extend_size(total, bytes.len())?;
            let inventory = inspect_private_account_backup_bytes(&bytes, account.project())?;
            if !matches!(inventory.private_version, 3 | 4)
                || inventory
                    .clock_floor
                    .is_none_or(|floor| floor < manifest.reset_at)
                || !identities.insert(inventory.database.database_id)
            {
                return Err(Error::BundleRoot(
                    "private reset state or database identity",
                ));
            }
            captured(account.project(), &bytes)?;
            hashes.push(hash(&bytes));
            reports.push(BundledAccountReport {
                project: account.project().into(),
                inventory,
            });
        }
        checkpoint("bundle_restore_inventory_validated");
        for (project, owner) in manifest.private_projects.iter().zip(&account_owners) {
            metadata::owned_directory(&private.join(project), owner)?;
        }
        metadata::owned_directory(path, owner)?;
        metadata::owned_directory(&private, &private_owner)?;
        check_manifest(&root, &manifest_owner, &encoded)?;
        if names(&root)? != expected_root
            || names(&private)? != roster
            || names(&root.join("registry"))? != ids
            || read_bounded(&root.join("root.json"), MANIFEST_LIMIT)? != encoded
        {
            return Err(Error::BundleRoot("root inventory changed"));
        }
        Ok((image, reports, hashes))
    })?;
    // Keep private ownership until the registry finishes its final identity checks.
    Ok(State {
        report: AccountBundleRootReport {
            registry: crate::inspect_registry_backup_bytes(&image)?,
            private_accounts: reports,
            reset_at: manifest.reset_at,
        },
        registry: image,
        private_hashes: hashes,
        contents: Contents {
            registry,
            accounts,
            account_owners,
            private_owner,
            manifest_owner,
            encoded,
            manifest,
            ids,
        },
    })
}

/// Offline full integrity/inventory inspection. Locks all supplied stores; it
/// neither resets sessions nor authenticates users. The manifest is not a roster
/// of independently attached services outside this restored root.
pub fn inspect_account_bundle_root(
    path: impl AsRef<Path>,
    pool: PasswordPool,
) -> Result<AccountBundleRootReport> {
    let path = path.as_ref();
    let owner = metadata::open_directory(path)?;
    lock(&owner)?;
    Ok(inspect_owned(path, &owner, pool)?.report)
}

/// Sensitive common-boundary image of this root's exact manifest roster. Source
/// owners span all prefixes and final inventory validation; no scope is reset.
pub fn capture_account_bundle_root(path: impl AsRef<Path>, pool: PasswordPool) -> Result<Vec<u8>> {
    let path = path.as_ref();
    let owner = metadata::open_directory(path)?;
    lock(&owner)?;
    let mut private = Vec::new();
    let state = inspect_owned_with(path, &owner, pool, |project, bytes| {
        let mut image = Vec::new();
        image
            .try_reserve_exact(bytes.len())
            .map_err(|_| Error::Limit)?;
        image.extend_from_slice(bytes);
        private.push((project.into(), image));
        Ok(())
    })?;
    account_bundle::encode(&state.registry, private)
}

/// Publish a verified private image outside the source root. Capture/backup do
/// not invalidate credentials. Existing destinations are never replaced.
pub fn backup_account_bundle_root(
    path: impl AsRef<Path>,
    target: impl AsRef<Path>,
    pool: PasswordPool,
) -> Result<crate::AccountBundleReport> {
    let path = path.as_ref();
    if super::parent(target.as_ref())
        .canonicalize()?
        .starts_with(path.canonicalize()?)
    {
        return Err(Error::Path);
    }
    let bytes = capture_account_bundle_root(path, pool)?;
    account_bundle::files::publish(&bytes, target.as_ref())
}
