//! Native operator-selected object namespaces; not service/user authorization.
use clap::{Args, Subcommand};
use emilybase_object_storage::{
    FileReport, MAX_PAYLOAD_BYTES, ObjectId, ProjectDirectory, ProjectId,
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
    /// Publish exact bounded binary stdin under a fresh typed object ID.
    Put { object: String },
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
fn input(reader: impl Read) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(MAX_PAYLOAD_BYTES + 1)
        .map_err(|_| "object input allocation failed")?;
    reader
        .take((MAX_PAYLOAD_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "object input unavailable")?;
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err("object input exceeds 8 MiB".into());
    }
    Ok(bytes)
}
pub fn run(arguments: Arguments) -> Result<()> {
    let project = arguments.project.parse::<ProjectId>()?;
    let object = match &arguments.operation {
        Operation::Put { object } | Operation::Inspect { object } => {
            Some(object.parse::<ObjectId>()?)
        }
        Operation::Init => None,
    };
    // Buffer the bounded redirected stream before acquiring any namespace lock.
    let bytes = if matches!(arguments.operation, Operation::Put { .. }) {
        if std::io::stdin().is_terminal() {
            return Err("object input requires redirected stdin".into());
        }
        Some(input(std::io::stdin().lock())?)
    } else {
        None
    };
    match arguments.operation {
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
