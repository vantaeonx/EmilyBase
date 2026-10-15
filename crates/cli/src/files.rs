//! Count-only native archive reports; no names, owners or payloads on stdout.
use emilybase_files::FileArchiveReport;
use std::io::IsTerminal;

mod mutation;
pub use mutation::{QuotaArguments, ReferenceArguments, RemoveArguments, RenameArguments};
pub use mutation::{quota, remove, rename, stat};

#[derive(clap::Args)]
pub struct PutArguments {
    path: std::path::PathBuf,
    project: String,
    file: String,
    object: String,
    /// Trusted native owner metadata, never a current account/session grant.
    #[arg(long)]
    owner: String,
    /// Bounded display text, never a pathname or an HTTP header.
    #[arg(long)]
    name: String,
}
pub fn put(arguments: PutArguments) -> Result<(), Box<dyn std::error::Error>> {
    let project = arguments
        .project
        .parse::<emilybase_object_storage::ProjectId>()?;
    let id = arguments.file.parse::<emilybase_files::FileId>()?;
    let object = arguments
        .object
        .parse::<emilybase_object_storage::ObjectId>()?;
    let owner = arguments
        .owner
        .parse::<emilybase_object_storage::ObjectId>()?;
    if std::io::stdin().is_terminal() {
        return Err("file input requires redirected stdin".into());
    }
    // Complete bounded input buffering before acquiring either native owner.
    let bytes = super::objects::input(
        std::io::stdin().lock(),
        emilybase_object_storage::MAX_PAYLOAD_BYTES,
    )?;
    let mut root = emilybase_files::NativeFileRoot::open(arguments.path, project)?;
    let info = root.publish(id, object, *owner.as_bytes(), &arguments.name, &bytes)?;
    file_report(project, &info)
}
fn file_report(
    project: emilybase_object_storage::ProjectId,
    info: &emilybase_files::FileInfo,
) -> Result<(), Box<dyn std::error::Error>> {
    super::write_operator_metadata(&serde_json::json!({
        "format":1,"project":project.to_string(),"file":info.id().to_string(),
        "object":info.object().to_string(),"revision":info.revision().to_string(),
        "bytes":info.report().payload_bytes,
        "sha256":info.report().sha256.iter().map(|b|format!("{b:02x}")).collect::<String>()
    }))
}

pub fn root_report(
    value: &emilybase_files::NativeFileRootInfo,
) -> Result<(), Box<dyn std::error::Error>> {
    let usage = value.usage();
    super::write_operator_metadata(&serde_json::json!({
        "format":1,"project":value.project().to_string(),
        "database_id":value.database_id().iter().map(|b|format!("{b:02x}")).collect::<String>(),
        "revision":value.revision().to_string(),
        "quota":{"max_objects":value.quota().objects(),"max_bytes":value.quota().payload_bytes()},
        "usage":{"objects":usage.physical_objects,"bytes":usage.payload_bytes,
            "references":usage.references,"orphans":usage.orphans}
    }))
}

pub fn report(value: &FileArchiveReport) -> Result<(), Box<dyn std::error::Error>> {
    let metadata = value.metadata();
    super::write_operator_metadata(&serde_json::json!({
        "format":1,
        "project":value.project().to_string(),
        "metadata":{
            "database_id":metadata.database_id.iter()
                .map(|byte|format!("{byte:02x}")).collect::<String>(),
            "last_transaction":metadata.last_transaction.to_string(),
            "wal_version":metadata.wal_version,
            "wal_bytes":metadata.wal_bytes,
            "tables":metadata.tables,"rows":metadata.rows,"pages":metadata.pages
        },
        "quota":{
            "max_objects":value.quota().objects(),
            "max_bytes":value.quota().payload_bytes()
        },
        "references":value.references(),
        "objects":{
            "count":value.objects().objects,
            "bytes":value.objects().payload_bytes,
            "digest":value.objects().digest.iter()
                .map(|byte|format!("{byte:02x}")).collect::<String>()
        }
    }))
}
