//! Offline administration through the original retained root owner.
use clap::{Args, Subcommand};
use emilybase_auth::{accounts::PolicyReceipt, password::PasswordPool, row_policy};
use emilybase_server::AccountRoot;
use serde::Serialize;
use std::io::{Read, Write};
use std::path::PathBuf;
use zeroize::Zeroizing;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Args)]
pub struct Arguments {
    /// Existing offline private root; stop its server first.
    path: PathBuf,
    /// Existing project identity from trusted operator metadata.
    project: String,
    /// Private exact service-key file, never the secret as an argument.
    #[arg(long)]
    key_file: PathBuf,
    #[command(subcommand)]
    command: Operation,
}
#[derive(Subcommand)]
enum Operation {
    /// Explicitly migrate private v3 to v4; never reset sessions or time.
    Enable,
    /// Print complete current receipt metadata without policy definitions.
    List,
    /// Read the exact bounded policy definition from stdin and install atomically.
    Install {
        table: String,
        /// Canonical u64 revision; zero means no policy for the actual table ID.
        #[arg(long, allow_hyphen_values = true)]
        expected: String,
    },
}
#[derive(Serialize)]
struct Receipt {
    table: String,
    revision: String,
    previous: String,
    sha256: String,
}
impl From<PolicyReceipt> for Receipt {
    fn from(value: PolicyReceipt) -> Self {
        Self {
            table: value.table.to_string(),
            revision: value.revision.to_string(),
            previous: value.previous.to_string(),
            sha256: value.sha256.iter().map(|b| format!("{b:02x}")).collect(),
        }
    }
}
fn expected(text: &str) -> Result<u64> {
    let value = text
        .parse::<u64>()
        .map_err(|_| "invalid canonical expected policy revision")?;
    if text != value.to_string() {
        return Err("invalid canonical expected policy revision".into());
    }
    Ok(value)
}
fn document(input: impl Read) -> Result<Zeroizing<Vec<u8>>> {
    let mut bytes = Zeroizing::new(Vec::new());
    input
        .take((row_policy::MAX_DOCUMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "policy input unavailable")?;
    row_policy::decode(&bytes)?;
    Ok(bytes)
}
fn output(value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > 65_536 {
        return Err("policy output outcome requires inspection".into());
    }
    let mut out = std::io::stdout().lock();
    out.write_all(&bytes)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}
pub fn run(arguments: Arguments) -> Result<()> {
    let Arguments {
        path,
        project,
        key_file,
        command,
    } = arguments;
    if !emilybase_auth::valid_project_id(&project) {
        return Err("invalid policy project identity".into());
    }
    let key = emilybase_auth::key_file::read_api_key_file(&key_file)?;
    // Finish bounded stdin validation before acquiring any database owner.
    // Current service authorization is performed by the retained root afterward.
    let prepared = match &command {
        Operation::Install {
            table,
            expected: revision,
        } => {
            if table.is_empty() || table.len() > emilybase_catalog::MAX_NAME_BYTES {
                return Err("invalid policy table name".into());
            }
            Some((expected(revision)?, document(std::io::stdin().lock())?))
        }
        _ => None,
    };
    let mut root = AccountRoot::open(path, PasswordPool::new(1)?)?;
    match command {
        Operation::Enable => {
            operation(root.enable_row_policy_catalog(&project, &key))?;
            let version = operation(root.private_schema_version(&project, &key))?;
            output(&serde_json::json!({"private_version":version}))
        }
        Operation::List => {
            let policies: Vec<Receipt> = operation(root.row_policy_receipts(&project, &key))?
                .into_iter()
                .map(Into::into)
                .collect();
            output(&serde_json::json!({"policies":policies}))
        }
        Operation::Install { table, .. } => {
            let (revision, bytes) = prepared.ok_or("policy input unavailable")?;
            let receipt =
                operation(root.install_row_policy(&project, &key, &table, revision, &bytes))?;
            output(&serde_json::json!({"receipt":Receipt::from(receipt)}))
        }
    }
}

fn operation<T>(value: emilybase_server::Result<T>) -> Result<T> {
    // Preserve typed private diagnostics without printing a nested error chain.
    value.map_err(|error| match error {
        emilybase_server::Error::Accounts(account) => Box::new(account) as _,
        other => Box::new(other) as _,
    })
}

#[cfg(test)]
mod tests;
