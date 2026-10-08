use crate::metadata::{self, Metadata};
use crate::{Error, MAX_PROJECTS, Result};
use emilybase_auth::{KeyDigest, issue_key, issue_project_id, valid_project_id};
use emilybase_catalog::Value;
use emilybase_query::Report;
use emilybase_transactions::Database;
use std::collections::BTreeMap;
use std::fs::{File, TryLockError};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub key_epoch: u64,
}
#[derive(serde::Serialize)]
pub struct CreatedProject {
    pub project: ProjectInfo,
    pub api_key: String,
}
struct Project {
    metadata: Metadata,
    gate: Arc<Mutex<()>>,
    directory_owner: Arc<File>,
    data_owner: Arc<File>,
}
pub struct ProjectStore {
    root: PathBuf,
    owner: Arc<File>,
    projects: BTreeMap<String, Project>,
    poisoned: bool,
}
/// Per-request capability. It is consumed by execution and cannot be cloned or reused.
pub struct AuthorizedProject {
    root: PathBuf,
    directory: PathBuf,
    gate: Arc<Mutex<()>>,
    _owner: Arc<File>,
    directory_owner: Arc<File>,
    data_owner: Arc<File>,
}
#[derive(serde::Serialize)]
pub struct ProjectStatus {
    pub transaction: u64,
    pub tables: usize,
    pub rows: usize,
}

impl ProjectStore {
    /// Open a committed registry without creating a missing source path.
    pub fn open_existing(root: impl AsRef<Path>) -> Result<Self> {
        metadata::directory(root.as_ref())?;
        Self::open(root)
    }
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        match std::fs::DirBuilder::new().mode(0o700).create(root) {
            Ok(()) => File::open(
                root.parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
            )?
            .sync_all()?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
        metadata::directory(root)?;
        let owner = metadata::open_directory(root)?;
        match owner.try_lock() {
            Ok(()) => (),
            Err(TryLockError::WouldBlock) => return Err(Error::Busy),
            Err(TryLockError::Error(e)) => return Err(e.into()),
        }
        let root = root.canonicalize()?;
        metadata::owned_directory(&root, &owner)?;
        let mut projects = BTreeMap::new();
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            let id = entry.file_name().into_string().map_err(|_| Error::Path)?;
            if id.starts_with(".creating-") {
                continue;
            }
            if !valid_project_id(&id) {
                return Err(Error::Path);
            }
            if projects.len() >= MAX_PROJECTS {
                return Err(Error::Limit);
            }
            metadata::directory(&entry.path())?;
            metadata::directory(&entry.path().join("data"))?;
            let directory_owner = Arc::new(metadata::open_directory(&entry.path())?);
            let data_owner = Arc::new(metadata::open_directory(&entry.path().join("data"))?);
            let project = metadata::read(&entry.path().join("project.json"), &id)?;
            projects.insert(
                id,
                Project {
                    metadata: project,
                    gate: Arc::new(Mutex::new(())),
                    directory_owner,
                    data_owner,
                },
            );
        }
        Ok(Self {
            root,
            owner: Arc::new(owner),
            projects,
            poisoned: false,
        })
    }
    fn ready(&self) -> Result<()> {
        if self.poisoned {
            Err(Error::Poisoned)
        } else {
            Ok(())
        }
    }
    pub fn list(&self) -> Result<Vec<ProjectInfo>> {
        self.ready()?;
        metadata::owned_directory(&self.root, &self.owner)?;
        for (id, project) in &self.projects {
            self.check_project_directory(id, project)?;
        }
        Ok(self.projects.values().map(|p| info(&p.metadata)).collect())
    }
    /// Offline consistent registry image. Contains private digests and plaintext data.
    /// Refuses outstanding capabilities and holds every database owner until capture finishes.
    pub fn backup_image(&mut self) -> Result<Vec<u8>> {
        self.capture_registry_with(Ok)
    }

    /// A common boundary for registry data and the explicitly supplied private
    /// roster. All account owners already exist; all data owners are acquired
    /// before the first prefix and retained through private capture/validation.
    /// Sensitive bytes, not automatic discovery of every platform service/store.
    pub fn capture_account_bundle(
        &mut self,
        accounts: &mut [emilybase_auth::accounts::AccountStore],
    ) -> Result<Vec<u8>> {
        self.ready()?;
        if accounts.len() > MAX_PROJECTS {
            return Err(Error::Limit);
        }
        let mut ids = std::collections::BTreeSet::new();
        for account in accounts.iter() {
            if !self.projects.contains_key(account.project())
                || !ids.insert(account.project().to_owned())
            {
                return Err(Error::BundleFormat("private roster project or duplicate"));
            }
        }
        self.capture_registry_with(|registry| {
            let mut private = Vec::with_capacity(accounts.len());
            let mut total = crate::account_bundle::HEADER
                .checked_add(registry.len())
                .filter(|n| *n <= crate::MAX_ACCOUNT_BUNDLE_BYTES)
                .ok_or(Error::Limit)?;
            for account in accounts {
                let image = account.backup_image()?;
                total = crate::account_bundle::extend_size(total, image.len())?;
                private.push((account.project().to_owned(), image));
                #[cfg(test)]
                crate::durability::checkpoint("bundle_private_prefix_captured");
            }
            crate::account_bundle::encode(&registry, private)
        })
    }

    /// Publish a complete sensitive bundle as a private file without replacement.
    /// The explicit roster remains owned by the caller; no restored scope is reset.
    pub fn backup_account_bundle(
        &mut self,
        accounts: &mut [emilybase_auth::accounts::AccountStore],
        target: impl AsRef<Path>,
    ) -> Result<crate::AccountBundleReport> {
        if crate::registry_files::parent(target.as_ref())
            .canonicalize()?
            .starts_with(&self.root)
        {
            return Err(Error::Path);
        }
        let bytes = self.capture_account_bundle(accounts)?;
        crate::account_bundle::files::publish(&bytes, target.as_ref())
    }

    pub(crate) fn capture_registry_with<T>(
        &mut self,
        capture: impl FnOnce(Vec<u8>) -> Result<T>,
    ) -> Result<T> {
        self.ready()?;
        if Arc::strong_count(&self.owner) != 1 {
            return Err(Error::Busy);
        }
        self.check_backup_source()?;
        let mut estimated = crate::registry_archive::HEADER;
        for (id, project) in &self.projects {
            let data = self.root.join(id).join("data");
            metadata::directory(&data)?;
            let wal = data.join("redo.wal");
            metadata::file(&wal)?;
            let wal_bytes =
                usize::try_from(std::fs::metadata(wal)?.len()).map_err(|_| Error::Limit)?;
            let metadata_bytes = metadata::encoded(&project.metadata)?.len();
            estimated = estimated
                .checked_add(crate::registry_archive::ENTRY_HEADER)
                .and_then(|size| size.checked_add(metadata_bytes))
                .and_then(|size| size.checked_add(emilybase_backup::HEADER_SIZE))
                .and_then(|size| size.checked_add(wal_bytes))
                .ok_or(Error::Limit)?;
            if estimated > crate::MAX_REGISTRY_BACKUP_BYTES {
                return Err(Error::Limit);
            }
        }
        // Take all directory/WAL locks before reading the first acknowledged prefix.
        let mut databases = Vec::with_capacity(self.projects.len());
        for id in self.projects.keys() {
            databases.push(Database::open(self.root.join(id).join("data"))?);
        }
        #[cfg(test)]
        crate::durability::checkpoint("registry_capture_owners_locked");
        let mut bytes = vec![0; crate::registry_archive::HEADER];
        for ((_, project), database) in self.projects.iter().zip(&mut databases) {
            let archive = emilybase_backup::encode(&database.committed_wal()?)?;
            crate::registry_archive::append(&mut bytes, &project.metadata, &archive)?;
        }
        crate::registry_archive::finish(&mut bytes, self.projects.len())?;
        crate::inspect_registry_backup_bytes(&bytes)?;
        // Explicit last use makes ownership retention span the whole callback.
        let outcome = capture(bytes);
        self.check_backup_source()?;
        drop(databases);
        outcome
    }
    /// Publish a verified private archive outside the registry without replacing any path.
    pub fn backup(&mut self, target: impl AsRef<Path>) -> Result<crate::RegistryBackupReport> {
        if crate::registry_files::parent(target.as_ref())
            .canonicalize()?
            .starts_with(&self.root)
        {
            return Err(Error::Path);
        }
        let bytes = self.backup_image()?;
        crate::registry_files::publish(&bytes, target.as_ref())
    }
    fn check_backup_source(&self) -> Result<()> {
        metadata::owned_directory(&self.root, &self.owner)?;
        let mut found = 0;
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let id = entry.file_name().into_string().map_err(|_| Error::Path)?;
            if id.starts_with(".creating-") {
                continue;
            }
            let project = self.projects.get(&id).ok_or(Error::Metadata)?;
            self.check_project_directory(&id, project)?;
            let current = metadata::read(&entry.path().join("project.json"), &id)?;
            if metadata::encoded(&current)? != metadata::encoded(&project.metadata)? {
                return Err(Error::Metadata);
            }
            found += 1;
        }
        if found != self.projects.len() {
            return Err(Error::Metadata);
        }
        Ok(())
    }
    pub fn create(&mut self, name: &str) -> Result<CreatedProject> {
        self.ready()?;
        metadata::owned_directory(&self.root, &self.owner)?;
        metadata::validate_name(name)?;
        if self.projects.len() >= MAX_PROJECTS {
            return Err(Error::Limit);
        }
        let id = issue_project_id()?;
        let api_key = issue_key()?;
        let project = Metadata {
            version: 1,
            id: id.clone(),
            name: name.into(),
            key: KeyDigest::from_token(&api_key)?,
            epoch: 1,
        };
        let pending = tempfile::Builder::new()
            .prefix(".creating-")
            .tempdir_in(&self.root)?;
        std::fs::set_permissions(pending.path(), std::fs::Permissions::from_mode(0o700))?;
        let data = pending.path().join("data");
        drop(Database::create(&data)?);
        let data_owner = Arc::new(metadata::open_directory(&data)?);
        let directory_owner = Arc::new(metadata::open_directory(pending.path())?);
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700))?;
        directory_sync(&data_owner, "create_data_sync")?;
        #[cfg(test)]
        crate::durability::checkpoint("create_data_synced");
        metadata::write_new(&pending.path().join("project.json"), &project)?;
        #[cfg(test)]
        crate::durability::checkpoint("create_metadata_synced");
        directory_sync(&directory_owner, "create_stage_sync")?;
        #[cfg(test)]
        crate::durability::checkpoint("create_stage_synced");
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            pending.path(),
            rustix::fs::CWD,
            self.root.join(&id),
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(std::io::Error::from)?;
        let _old = pending.keep();
        #[cfg(test)]
        crate::durability::checkpoint("create_renamed");
        if let Err(error) = directory_sync(&self.owner, "create_root_sync") {
            self.poisoned = true;
            return Err(Error::PublicationUnknown(error));
        }
        #[cfg(test)]
        crate::durability::checkpoint("create_root_synced");
        let response = CreatedProject {
            project: info(&project),
            api_key,
        };
        self.projects.insert(
            id,
            Project {
                metadata: project,
                gate: Arc::new(Mutex::new(())),
                directory_owner,
                data_owner,
            },
        );
        Ok(response)
    }
    pub fn authorize(&self, id: &str, token: &str) -> Result<AuthorizedProject> {
        self.ready()?;
        if !valid_project_id(id) {
            return Err(Error::Denied);
        }
        let project = self.projects.get(id).ok_or(Error::Denied)?;
        if !project.metadata.key.verifies(token) {
            return Err(Error::Denied);
        }
        Ok(AuthorizedProject {
            root: self.root.clone(),
            directory: self.root.join(id),
            gate: Arc::clone(&project.gate),
            _owner: Arc::clone(&self.owner),
            directory_owner: Arc::clone(&project.directory_owner),
            data_owner: Arc::clone(&project.data_owner),
        })
    }
    /// Privileged operation; HTTP transport requires its administrative credential.
    pub fn rotate(&mut self, id: &str) -> Result<CreatedProject> {
        self.ready()?;
        metadata::owned_directory(&self.root, &self.owner)?;
        let project = self.projects.get(id).ok_or(Error::Denied)?;
        self.check_project_directory(id, project)?;
        let mut changed = project.metadata.clone();
        changed.epoch = changed.epoch.checked_add(1).ok_or(Error::Limit)?;
        let api_key = issue_key()?;
        changed.key = KeyDigest::from_token(&api_key)?;
        let path = self.root.join(id);
        metadata::directory(&path)?;
        metadata::file(&path.join("project.json"))?;
        let mut pending = tempfile::NamedTempFile::new_in(&path)?;
        pending.write_all(&metadata::encoded(&changed)?)?;
        directory_sync(pending.as_file(), "rotate_file_sync")?;
        #[cfg(test)]
        crate::durability::checkpoint("rotate_file_synced");
        if let Err(error) = pending.persist(path.join("project.json")) {
            self.poisoned = true;
            return Err(Error::PublicationUnknown(error.error));
        }
        #[cfg(test)]
        crate::durability::checkpoint("rotate_renamed");
        if let Err(error) = directory_sync(&project.directory_owner, "rotate_directory_sync") {
            self.poisoned = true;
            return Err(Error::PublicationUnknown(error));
        }
        #[cfg(test)]
        crate::durability::checkpoint("rotate_directory_synced");
        let response = CreatedProject {
            project: info(&changed),
            api_key,
        };
        self.projects.get_mut(id).ok_or(Error::Denied)?.metadata = changed;
        Ok(response)
    }
    fn check_project_directory(&self, id: &str, project: &Project) -> Result<()> {
        let path = self.root.join(id);
        metadata::owned_directory(&path, &project.directory_owner)?;
        metadata::owned_directory(&path.join("data"), &project.data_owner)
    }
}
fn directory_sync(file: &File, _boundary: &str) -> std::io::Result<()> {
    #[cfg(test)]
    crate::durability::fail(_boundary)?;
    file.sync_all()
}
impl AuthorizedProject {
    pub(crate) fn table_operation<T>(
        self,
        work: impl FnOnce(&mut Database) -> crate::table_api::Result<T>,
    ) -> Result<T> {
        let _gate = self.gate.lock().map_err(|_| Error::Poisoned)?;
        self.check_directory()?;
        let mut database = Database::open(self.directory.join("data"))?;
        Ok(work(&mut database)?)
    }
    pub(crate) fn export_table(self, table: &str, limit: usize) -> Result<Vec<u8>> {
        let _gate = self.gate.lock().map_err(|_| Error::Poisoned)?;
        self.check_directory()?;
        let database = Database::open(self.directory.join("data"))?;
        Ok(emilybase_transfer::export_table_bounded(
            database.view()?,
            table,
            limit,
        )?)
    }
    pub(crate) fn import_table(
        self,
        table: emilybase_transfer::VerifiedTable,
    ) -> Result<crate::http::ImportedTable> {
        let _gate = self.gate.lock().map_err(|_| Error::Poisoned)?;
        self.check_directory()?;
        let mut database = Database::open(self.directory.join("data"))?;
        let transfer = table.report().clone();
        let transaction = emilybase_transfer::import_table(&mut database, table)?;
        Ok(crate::http::ImportedTable {
            transfer,
            transaction,
        })
    }
    fn check_directory(&self) -> Result<()> {
        metadata::owned_directory(&self.root, &self._owner)?;
        metadata::owned_directory(&self.directory, &self.directory_owner)?;
        metadata::owned_directory(&self.directory.join("data"), &self.data_owner)
    }
    pub fn execute(self, sql: &str, parameters: &[Value]) -> Result<Report> {
        let _gate = self.gate.lock().map_err(|_| Error::Poisoned)?;
        self.check_directory()?;
        let mut database = Database::open(self.directory.join("data"))?;
        Ok(emilybase_query::execute(&mut database, sql, parameters)?)
    }
    pub fn explain(
        self,
        sql: &str,
        parameters: &[Value],
    ) -> Result<emilybase_query::PlanDescription> {
        let _gate = self.gate.lock().map_err(|_| Error::Poisoned)?;
        self.check_directory()?;
        let database = Database::open(self.directory.join("data"))?;
        Ok(emilybase_query::explain(database.view()?, sql, parameters)?)
    }
    pub fn status(self) -> Result<ProjectStatus> {
        let _gate = self.gate.lock().map_err(|_| Error::Poisoned)?;
        self.check_directory()?;
        let database = Database::open(self.directory.join("data"))?;
        Ok(ProjectStatus {
            transaction: database.last_transaction(),
            tables: database.view()?.table_count(),
            rows: database.view()?.row_count(),
        })
    }
}
fn info(metadata: &Metadata) -> ProjectInfo {
    ProjectInfo {
        id: metadata.id.clone(),
        name: metadata.name.clone(),
        key_epoch: metadata.epoch,
    }
}
