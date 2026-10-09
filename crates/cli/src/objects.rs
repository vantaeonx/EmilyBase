//! Native operator-selected object namespaces; not service/user authorization.
use clap::{Args, Subcommand};
use emilybase_object_storage::{
    FileReport, MAX_PAYLOAD_BYTES, ObjectId, ProjectDirectory, ProjectId, WriteLimits,
};
use std::io::{IsTerminal, Read};
use std::path::PathBuf;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Args)]
pub struct Arguments {
    /// Existing private 0700 directory, never implicitly created or repaired.
    path: PathBuf,
    project: String,
    #[command(subcommand)]
    operation: Operation,
}
#[derive(Subcommand)]
enum Operation {
    /// Publish the project marker once in an existing directory.
    Init,
    /// Fully verify the bounded directory and print complete metadata only.
    List,
    /// Capture all objects and durably publish one fresh private archive.
    Backup { destination: PathBuf },
    /// Publish exact bounded binary stdin under a fresh typed object ID.
    Put { object: String },
    /// Publish after complete capacity admission under explicit native limits.
    PutBounded {
        object: String,
        #[arg(long)]
        max_objects: usize,
        #[arg(long)]
        max_bytes: u64,
    },
    /// Verify an existing object and print metadata without payload output.
    Inspect { object: String },
}
fn report(value: &FileReport) -> Result<()> {
    let sha256 = value
        .sha256
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    super::write_operator_metadata(&serde_json::json!({
        "format":1,"bytes":value.payload_bytes,"sha256":sha256
    }))
}
pub fn archive_report(
    project: ProjectId,
    value: &emilybase_object_storage::ArchiveReport,
) -> Result<()> {
    super::write_operator_metadata(&serde_json::json!({
        "format":1,"project":project.to_string(),"objects":value.objects,
        "bytes":value.payload_bytes,
        "digest":value.digest.iter().map(|b|format!("{b:02x}")).collect::<String>()
    }))
}
fn input(reader: impl Read, maximum: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(maximum + 1)
        .map_err(|_| "object input allocation failed")?;
    reader
        .take((maximum + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "object input unavailable")?;
    if bytes.len() > maximum {
        return Err("object input exceeds permitted bytes".into());
    }
    Ok(bytes)
}
pub fn run(arguments: Arguments) -> Result<()> {
    let project = arguments.project.parse::<ProjectId>()?;
    let object = match &arguments.operation {
        Operation::Put { object }
        | Operation::PutBounded { object, .. }
        | Operation::Inspect { object } => Some(object.parse::<ObjectId>()?),
        Operation::Init | Operation::List | Operation::Backup { .. } => None,
    };
    let limits = match &arguments.operation {
        Operation::PutBounded {
            max_objects,
            max_bytes,
            ..
        } => {
            let limits = WriteLimits::new(*max_objects, *max_bytes)?;
            if limits.objects() == 0 {
                return Err("object count limit forbids writes".into());
            }
            Some(limits)
        }
        _ => None,
    };
    // Buffer the bounded redirected stream before acquiring any namespace lock.
    let bytes = if matches!(
        arguments.operation,
        Operation::Put { .. } | Operation::PutBounded { .. }
    ) {
        if std::io::stdin().is_terminal() {
            return Err("object input requires redirected stdin".into());
        }
        let maximum = limits.map_or(MAX_PAYLOAD_BYTES, |value| {
            value.payload_bytes().min(MAX_PAYLOAD_BYTES as u64) as usize
        });
        Some(input(std::io::stdin().lock(), maximum)?)
    } else {
        None
    };
    match arguments.operation {
        Operation::PutBounded { .. } => {
            let mut owner = ProjectDirectory::open(arguments.path, project)?;
            let value = owner.put_bounded(
                object.ok_or("object identity unavailable")?,
                &bytes.ok_or("object input unavailable")?,
                limits.ok_or("object write limits unavailable")?,
            )?;
            super::write_operator_metadata(&serde_json::json!({
                "format":1,"project":project.to_string(),"object":value.object().to_string(),
                "bytes":value.report().payload_bytes,
                "sha256":value.report().sha256.iter().map(|b|format!("{b:02x}")).collect::<String>(),
                "objects":value.inventory().entries().len(),"total_bytes":value.inventory().payload_bytes(),
                "digest":value.inventory().digest().iter().map(|b|format!("{b:02x}")).collect::<String>()
            }))
        }
        Operation::Backup { destination } => {
            let owner = ProjectDirectory::open(arguments.path, project)?;
            let value = owner.backup_to(destination)?;
            archive_report(project, &value)
        }
        Operation::List => {
            let owner = ProjectDirectory::open(arguments.path, project)?;
            let inventory = owner.inventory()?;
            let objects = inventory.entries().iter().map(|entry| serde_json::json!({
                "object":entry.object().to_string(),"bytes":entry.report().payload_bytes,
                "sha256":entry.report().sha256.iter().map(|b|format!("{b:02x}")).collect::<String>()
            })).collect::<Vec<_>>();
            super::write_operator_metadata(&serde_json::json!({
                "format":1,"project":inventory.project().to_string(),"objects":objects,
                "bytes":inventory.payload_bytes(),
                "digest":inventory.digest().iter().map(|b|format!("{b:02x}")).collect::<String>()
            }))
        }
        Operation::Init => {
            let owner = ProjectDirectory::initialize(arguments.path, project)?;
            super::write_operator_metadata(
                &serde_json::json!({"format":1,"project":owner.project().to_string()}),
            )
        }
        Operation::Put { .. } => {
            let mut owner = ProjectDirectory::open(arguments.path, project)?;
            report(&owner.put(
                object.ok_or("object identity unavailable")?,
                &bytes.ok_or("object input unavailable")?,
            )?)
        }
        Operation::Inspect { .. } => {
            let owner = ProjectDirectory::open(arguments.path, project)?;
            report(
                owner
                    .get(object.ok_or("object identity unavailable")?)?
                    .report(),
            )
        }
    }
}
