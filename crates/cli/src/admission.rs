//! Explicit offline admission, separate from schema migration and user revocation.
use clap::{Args, Subcommand};
use emilybase_auth::{accounts::PublicAdmissionReceipt, password::PasswordPool};
use emilybase_server::AccountRoot;
use serde::Serialize;
use std::io::Write;
use std::path::PathBuf;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Args)]
pub struct Arguments {
    /// Existing offline root; stop its server before selecting the same owner.
    path: PathBuf,
    /// Project identity from trusted operator metadata.
    project: String,
    /// Exact private service-key file; never pass the secret as an argument.
    #[arg(long)]
    key_file: PathBuf,
    #[command(subcommand)]
    command: Operation,
}
#[derive(Subcommand)]
enum Operation {
    /// Explicitly migrate private v4 to v5, closed; requires the policy catalog first.
    EnableCatalog,
    /// Read current admission metadata without changing time, sessions or policies.
    Status,
    /// Open using a canonical current revision; never implicitly migrate or retry.
    Open {
        #[arg(long, allow_hyphen_values = true)]
        expected: String,
    },
    /// Suspend user operations; closing admission does not revoke session families.
    Close {
        #[arg(long, allow_hyphen_values = true)]
        expected: String,
    },
}
#[derive(Serialize)]
struct Receipt {
    enabled: bool,
    revision: String,
    previous: String,
}
impl From<PublicAdmissionReceipt> for Receipt {
    fn from(value: PublicAdmissionReceipt) -> Self {
        Self {
            enabled: value.enabled,
            revision: value.revision.to_string(),
            previous: value.previous.to_string(),
        }
    }
}
fn expected(text: &str) -> Result<u64> {
    let revision = text
        .parse::<u64>()
        .map_err(|_| "invalid canonical expected admission revision")?;
    if text != revision.to_string() {
        return Err("invalid canonical expected admission revision".into());
    }
    Ok(revision)
}
fn operation<T>(value: emilybase_server::Result<T>) -> Result<T> {
    value.map_err(|error| match error {
        emilybase_server::Error::Accounts(account) => Box::new(account) as _,
        other => Box::new(other) as _,
    })
}
pub fn run(arguments: Arguments) -> Result<()> {
    let Arguments {
        path,
        project,
        key_file,
        command,
    } = arguments;
    if !emilybase_auth::valid_project_id(&project) {
        return Err("invalid admission project identity".into());
    }
    // Validate command input before acquiring any database owner. No stdin input.
    let revision = match &command {
        Operation::Open { expected: value } | Operation::Close { expected: value } => {
            Some(expected(value)?)
        }
        _ => None,
    };
    let key = emilybase_auth::key_file::read_api_key_file(&key_file)?;
    let mut root = AccountRoot::open(path, PasswordPool::new(1)?)?;
    let receipt = match command {
        Operation::EnableCatalog => {
            operation(root.enable_public_admission_catalog(&project, &key))?
        }
        Operation::Status => operation(root.public_admission(&project, &key))?,
        Operation::Open { .. } | Operation::Close { .. } => {
            let enabled = matches!(command, Operation::Open { .. });
            operation(root.set_public_admission(
                &project,
                &key,
                revision.ok_or("admission revision unavailable")?,
                enabled,
            ))?
        }
    };
    let bytes = serde_json::to_vec(&serde_json::json!({
        "private_version":operation(root.private_schema_version(&project, &key))?,
        "admission":Receipt::from(receipt)
    }))?;
    // The original durable operation completed before output. A missing result
    // requires inspection; do not infer rollback from a terminal or pipe failure.
    let mut output = std::io::stdout().lock();
    output
        .write_all(&bytes)
        .and_then(|()| output.write_all(b"\n"))
        .and_then(|()| output.flush())
        .map_err(|_| "admission output unavailable; inspect current state before retry")?;
    Ok(())
}

#[cfg(test)]
mod tests;
