//! Strict project-data migration transport; never operate on private account stores.
use axum::response::Response;
use emilybase_migrations::{Receipt, apply, inspect, prepare};
use emilybase_transactions::Database;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("invalid bounded migration request document")]
    Document,
    #[error("migration response requires inspection")]
    Response,
    #[error(transparent)]
    Engine(#[from] emilybase_migrations::Error),
}
type Result<T> = std::result::Result<T, MigrationError>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    version: u32,
    label: String,
    sql: String,
}
fn decode(bytes: &[u8]) -> Result<Input> {
    if bytes.len() > crate::http::MAX_BODY {
        return Err(MigrationError::Document);
    }
    serde_json::from_slice(bytes).map_err(|_| MigrationError::Document)
}

/// Pure grammar/bounds check. Grants no authority and performs no database work.
pub fn validate_migration_request(bytes: &[u8]) -> Result<()> {
    let input = decode(bytes)?;
    prepare(input.version, &input.label, &input.sql)?;
    Ok(())
}

#[derive(Serialize)]
struct WireReceipt {
    version: u32,
    label: String,
    sha256: String,
    transaction: String,
}
impl From<Receipt> for WireReceipt {
    fn from(value: Receipt) -> Self {
        Self {
            version: value.version,
            label: value.label,
            sha256: value.sha256.iter().map(|b| format!("{b:02x}")).collect(),
            transaction: value.transaction.to_string(),
        }
    }
}
fn response(value: &impl Serialize) -> Result<Response> {
    let bytes = serde_json::to_vec(value).map_err(|_| MigrationError::Response)?;
    if bytes.len() > crate::http::MAX_BODY {
        return Err(MigrationError::Response);
    }
    Ok(crate::http::transfer_response(bytes))
}
pub(crate) fn list(database: &mut Database) -> Result<Response> {
    #[derive(Serialize)]
    struct Inventory {
        migrations: Vec<WireReceipt>,
    }
    response(&Inventory {
        migrations: inspect(database)?.into_iter().map(Into::into).collect(),
    })
}
pub(crate) fn run(database: &mut Database, bytes: &[u8]) -> Result<Response> {
    #[derive(Serialize)]
    struct Applied {
        receipt: WireReceipt,
        already_applied: bool,
    }
    let input = decode(bytes)?;
    let migration = prepare(input.version, &input.label, &input.sql)?;
    let applied = apply(database, &migration)?;
    response(&Applied {
        receipt: applied.receipt.into(),
        already_applied: applied.already_applied,
    })
}

#[cfg(test)]
mod tests;
