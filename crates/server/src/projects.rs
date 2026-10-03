use crate::metadata::{self, Metadata};
use crate::{Error, MAX_PROJECTS, Result};
use emilybase_auth::{KeyDigest, issue_key, issue_project_id, valid_project_id};
use emilybase_catalog::Value;
use emilybase_query::Report;
use emilybase_transactions::Database;
use std::collections::BTreeMap;
use std::fs::{File, TryLockError};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
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
}
pub struct ProjectStore {
    root: PathBuf,
    owner: Arc<File>,
    projects: BTreeMap<String, Project>,
    poisoned: bool,
}
/// Per-request capability. It is consumed by execution and cannot be cloned or reused.
pub struct AuthorizedProject {
    directory: PathBuf,
    gate: Arc<Mutex<()>>,
    _owner: Arc<File>,
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
        let owner = File::open(root)?;
        match owner.try_lock() {
            Ok(()) => (),
            Err(TryLockError::WouldBlock) => return Err(Error::Busy),
            Err(TryLockError::Error(e)) => return Err(e.into()),
        }
        let root = root.canonicalize()?;
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
            let project = metadata::read(&entry.path().join("project.json"), &id)?;
            projects.insert(
                id,
                Project {
                    metadata: project,
                    gate: Arc::new(Mutex::new(())),
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
        Ok(self.projects.values().map(|p| info(&p.metadata)).collect())
    }
    /// Offline consistent registry image. Contains private digests and plaintext data.
    /// Refuses outstanding capabilities and holds every database owner until capture finishes.
    pub fn backup_image(&mut self) -> Result<Vec<u8>> {
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
        let mut bytes = vec![0; crate::registry_archive::HEADER];
        for ((_, project), database) in self.projects.iter().zip(&mut databases) {
            let archive = emilybase_backup::encode(&database.committed_wal()?)?;
            crate::registry_archive::append(&mut bytes, &project.metadata, &archive)?;
        }
        crate::registry_archive::finish(&mut bytes, self.projects.len())?;
        crate::inspect_registry_backup_bytes(&bytes)?;
        self.check_backup_source()?;
        Ok(bytes)
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
        metadata::directory(&self.root)?;
        let current = std::fs::symlink_metadata(&self.root)?;
        let owned = self.owner.metadata()?;
        if (current.dev(), current.ino()) != (owned.dev(), owned.ino()) {
            return Err(Error::Path);
        }
        let mut found = 0;
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let id = entry.file_name().into_string().map_err(|_| Error::Path)?;
            if id.starts_with(".creating-") {
                continue;
            }
            let project = self.projects.get(&id).ok_or(Error::Metadata)?;
            metadata::directory(&entry.path())?;
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
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700))?;
        directory_sync(&File::open(&data)?, "create_data_sync")?;
        #[cfg(test)]
        crate::durability::checkpoint("create_data_synced");
        metadata::write_new(&pending.path().join("project.json"), &project)?;
        #[cfg(test)]
        crate::durability::checkpoint("create_metadata_synced");
        directory_sync(&File::open(pending.path())?, "create_stage_sync")?;
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
            directory: self.root.join(id),
            gate: Arc::clone(&project.gate),
            _owner: Arc::clone(&self.owner),
        })
    }
    /// Privileged operation; HTTP transport requires its administrative credential.
    pub fn rotate(&mut self, id: &str) -> Result<CreatedProject> {
        self.ready()?;
        let project = self.projects.get(id).ok_or(Error::Denied)?;
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
        if let Err(error) = File::open(&path)
            .and_then(|directory| directory_sync(&directory, "rotate_directory_sync"))
        {
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
}
fn directory_sync(file: &File, _boundary: &str) -> std::io::Result<()> {
    #[cfg(test)]
    crate::durability::fail(_boundary)?;
    file.sync_all()
}
impl AuthorizedProject {
    pub fn execute(self, sql: &str, parameters: &[Value]) -> Result<Report> {
        let _gate = self.gate.lock().map_err(|_| Error::Poisoned)?;
        metadata::directory(&self.directory)?;
        metadata::directory(&self.directory.join("data"))?;
        let mut database = Database::open(self.directory.join("data"))?;
        Ok(emilybase_query::execute(&mut database, sql, parameters)?)
    }
    pub fn explain(
        self,
        sql: &str,
        parameters: &[Value],
    ) -> Result<emilybase_query::PlanDescription> {
        let _gate = self.gate.lock().map_err(|_| Error::Poisoned)?;
        metadata::directory(&self.directory)?;
        metadata::directory(&self.directory.join("data"))?;
        let database = Database::open(self.directory.join("data"))?;
        Ok(emilybase_query::explain(database.view()?, sql, parameters)?)
    }
    pub fn status(self) -> Result<ProjectStatus> {
        let _gate = self.gate.lock().map_err(|_| Error::Poisoned)?;
        metadata::directory(&self.directory)?;
        metadata::directory(&self.directory.join("data"))?;
        let database = Database::open(self.directory.join("data"))?;
        Ok(ProjectStatus {
            transaction: database.last_transaction(),
            tables: database.view()?.schemas().len(),
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
