//! Explicit native scopes, count-only reports and no filesystem roster adoption.
use emilybase_files::NativeFileRoot;
use emilybase_object_storage::ProjectId;
use emilybase_server::{AccountFileArchiveReport, AccountRoot, MAX_COMPONENT_FILE_STORES};
use rustix::fs::{Mode, OFlags};
use std::fs::File;
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::path::PathBuf;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(clap::Args)]
pub struct BackupArguments {
    root: PathBuf,
    archive: PathBuf,
    /// Existing private parent of explicitly selected project-named file roots.
    #[arg(long)]
    file_parent: Option<PathBuf>,
    /// Repeat per selected project (at most four); no directory discovery.
    #[arg(long)]
    file_project: Vec<String>,
}
pub fn backup(arguments: BackupArguments) -> Result<()> {
    if arguments.file_project.len() > MAX_COMPONENT_FILE_STORES
        || arguments.file_parent.is_some() == arguments.file_project.is_empty()
    {
        return Err("file parent requires one to four explicit unique projects".into());
    }
    let mut projects = arguments
        .file_project
        .iter()
        .map(|p| p.parse::<ProjectId>())
        .collect::<std::result::Result<Vec<_>, _>>()?;
    projects.sort_by_key(|p| p.to_string());
    if projects.windows(2).any(|p| p[0] == p[1]) {
        return Err("duplicate file project".into());
    }
    let pool = emilybase_auth::password::PasswordPool::new(1)?;
    let mut accounts = AccountRoot::open(arguments.root, pool)?;
    let declared = accounts.private_projects()?;
    if projects
        .iter()
        .any(|p| declared.binary_search(&p.to_string()).is_err())
    {
        return Err("file project is not in the declared private roster".into());
    }
    let parent = arguments
        .file_parent
        .map(|path| -> Result<File> {
            if path.as_os_str().as_bytes().len() > 4096 {
                return Err("invalid file parent".into());
            }
            let file: File = rustix::fs::open(
                &path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(std::io::Error::from)?
            .into();
            let m = file.metadata()?;
            if !m.is_dir() || m.mode() & 0o7777 != 0o700 {
                return Err("file parent must be private0700".into());
            }
            Ok(file)
        })
        .transpose()?;
    let mut files = Vec::with_capacity(projects.len());
    if let Some(parent) = &parent {
        for project in projects {
            files.push(NativeFileRoot::open_at(
                parent,
                &project.to_string(),
                project,
            )?);
        }
    }
    report(&emilybase_server::backup_account_file_components(
        &mut accounts,
        &mut files,
        arguments.archive,
    )?)
}
pub fn report(value: &AccountFileArchiveReport) -> Result<()> {
    super::write_operator_metadata(&serde_json::json!({
        "format":emilybase_server::ACCOUNT_FILE_ARCHIVE_VERSION,
        "projects":value.accounts().registry.projects.len(),
        "private_projects":value.manifest().private_projects().len(),
        "file_stores":value.manifest().file_stores().len(),
        "references":value.references(),"physical_objects":value.physical_objects(),
        "copied_bytes":value.copied_bytes(),"archive_bytes":value.encoded_bytes(),
        "reset_at":value.manifest().reset_at().to_string()
    }))
}
