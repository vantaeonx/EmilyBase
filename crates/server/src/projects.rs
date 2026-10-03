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

impl ProjectStore {
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
        File::open(&data)?.sync_all()?;
        metadata::write_new(&pending.path().join("project.json"), &project)?;
        File::open(pending.path())?.sync_all()?;
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            pending.path(),
            rustix::fs::CWD,
            self.root.join(&id),
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(std::io::Error::from)?;
        let _old = pending.keep();
        if let Err(error) = self.owner.sync_all() {
            self.poisoned = true;
            return Err(Error::PublicationUnknown(error));
        }
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
    /// Privileged operation; the future transport must require its administrative credential.
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
        pending.as_file().sync_all()?;
        if let Err(error) = pending.persist(path.join("project.json")) {
            self.poisoned = true;
            return Err(Error::PublicationUnknown(error.error));
        }
        if let Err(error) = File::open(&path).and_then(|directory| directory.sync_all()) {
            self.poisoned = true;
            return Err(Error::PublicationUnknown(error));
        }
        let response = CreatedProject {
            project: info(&changed),
            api_key,
        };
        self.projects.get_mut(id).ok_or(Error::Denied)?.metadata = changed;
        Ok(response)
    }
}
impl AuthorizedProject {
    pub fn execute(self, sql: &str, parameters: &[Value]) -> Result<Report> {
        let _gate = self.gate.lock().map_err(|_| Error::Poisoned)?;
        metadata::directory(&self.directory)?;
        metadata::directory(&self.directory.join("data"))?;
        let mut database = Database::open(self.directory.join("data"))?;
        Ok(emilybase_query::execute(&mut database, sql, parameters)?)
    }
}
fn info(metadata: &Metadata) -> ProjectInfo {
    ProjectInfo {
        id: metadata.id.clone(),
        name: metadata.name.clone(),
        key_epoch: metadata.epoch,
    }
}
