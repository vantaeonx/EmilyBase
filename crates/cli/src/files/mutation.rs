use emilybase_files::{FileId, FileQuota, NativeFileRoot};
use emilybase_object_storage::{ObjectId, ProjectId};
use std::path::PathBuf;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(clap::Args)]
pub struct ReferenceArguments {
    path: PathBuf,
    project: String,
    file: String,
}
impl ReferenceArguments {
    fn open(self) -> Result<(NativeFileRoot, FileId)> {
        let project: ProjectId = self.project.parse()?;
        let file = self.file.parse()?;
        Ok((NativeFileRoot::open(self.path, project)?, file))
    }
}
#[derive(clap::Args)]
pub struct RemoveArguments {
    #[command(flatten)]
    reference: ReferenceArguments,
    /// Exact logical revision from file-root-stat; a stale value conflicts.
    #[arg(long)]
    revision: u64,
}
#[derive(clap::Args)]
pub struct RenameArguments {
    #[command(flatten)]
    mutation: RemoveArguments,
    /// Bounded display text, never a pathname.
    #[arg(long)]
    name: String,
}
#[derive(clap::Args)]
pub struct QuotaArguments {
    path: PathBuf,
    project: String,
    /// Exact canonical database identity from file-root-info.
    #[arg(long)]
    database: String,
    /// Exact global metadata revision from file-root-info.
    #[arg(long)]
    revision: u64,
    #[arg(long)]
    max_objects: usize,
    #[arg(long)]
    max_bytes: u64,
}
pub fn stat(arguments: ReferenceArguments) -> Result<()> {
    let (mut root, id) = arguments.open()?;
    let info = root.file_info(id)?.ok_or(emilybase_files::Error::Missing)?;
    super::file_report(root.inspect()?.project(), &info)
}
pub fn rename(arguments: RenameArguments) -> Result<()> {
    let (mut root, id) = arguments.mutation.reference.open()?;
    let info = root.rename(id, arguments.mutation.revision, &arguments.name)?;
    super::file_report(root.inspect()?.project(), &info)
}
pub fn remove(arguments: RemoveArguments) -> Result<()> {
    let (mut root, id) = arguments.reference.open()?;
    let removed = root.remove(id, arguments.revision)?;
    super::super::write_operator_metadata(&serde_json::json!({
        "format":1,"project":root.inspect()?.project().to_string(),
        "file":removed.removed().id().to_string(),
        "object":removed.removed().object().to_string(),
        "removed_revision":removed.removed().revision().to_string(),
        "revision":removed.revision().to_string()
    }))
}
pub fn quota(arguments: QuotaArguments) -> Result<()> {
    let project: ProjectId = arguments.project.parse()?;
    let database: ObjectId = arguments.database.parse()?;
    let quota = FileQuota::new(arguments.max_objects, arguments.max_bytes)?;
    let mut root = NativeFileRoot::open(arguments.path, project)?;
    let expected = root.quota_state()?;
    if expected.database_id() != database.as_bytes() {
        return Err(emilybase_files::Error::Scope.into());
    }
    if expected.revision() != arguments.revision {
        return Err(emilybase_files::Error::Conflict.into());
    }
    root.set_quota(expected, quota)?;
    super::root_report(&root.inspect()?)
}
