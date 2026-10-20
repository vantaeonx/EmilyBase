//! Pure bounded original typed file-policy records, without installation authority.
use super::{BoundPolicy, Change, MAX_DOCUMENT_BYTES, PolicyError, Result, StoreContext, decode};
use crate::accounts::SessionPrincipal;
use emilybase_catalog::{Column, DataType, Row, Schema, Value, encode_row};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub const RECORD_VERSION: i64 = 1;
const DOMAIN: &[u8] = b"emilybase-file-policy-record-v1\0";

/// Sensitive original typed data. Raw row access is explicit and must not be logged.
pub struct PolicyRecord {
    row: Row,
}
impl std::fmt::Debug for PolicyRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EncodedFilePolicyRecord(redacted)")
    }
}
impl PolicyRecord {
    pub fn row(&self) -> &[Value] {
        &self.row
    }
    /// Move the complete row into the trusted original catalog transaction.
    pub fn into_row(self) -> Row {
        self.row
    }
}

/// Integrity-checked observations. Neither a current installed policy nor a grant.
pub struct DecodedPolicy {
    database: [u8; 16],
    revision: u64,
    previous: u64,
    sha256: [u8; 32],
    document: Vec<u8>,
    policy: BoundPolicy,
}
impl std::fmt::Debug for DecodedPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DecodedFilePolicyRecords(redacted)")
    }
}
impl DecodedPolicy {
    pub fn database(&self) -> &[u8; 16] {
        &self.database
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn previous(&self) -> u64 {
        self.previous
    }
    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }
    /// Exact original bytes for trusted explicit retry comparison, never logging.
    pub fn document(&self) -> &[u8] {
        &self.document
    }
    pub fn check_context(
        &self,
        context: StoreContext<'_>,
        principal: &SessionPrincipal<'_>,
    ) -> Result<()> {
        self.policy.check_context(context, principal)
    }
    /// Pure model evaluation only; the caller still needs current installed authority.
    pub fn authorize(
        &self,
        context: StoreContext<'_>,
        principal: &SessionPrincipal<'_>,
        change: Change,
    ) -> Result<()> {
        self.policy.authorize(context, principal, change)
    }
}
fn invalid<T>() -> Result<T> {
    Err(PolicyError::Document)
}
fn key(database: &[u8; 16]) -> String {
    database.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn revision(revision: u64, previous: u64) -> Result<()> {
    if revision < 2 || previous == 1 || previous >= revision {
        return invalid();
    }
    Ok(())
}
fn decimal(value: &str) -> Result<u64> {
    if value.len() > 20 {
        return invalid();
    }
    let number = value.parse::<u64>().map_err(|_| PolicyError::Document)?;
    if number.to_string() != value {
        return invalid();
    }
    Ok(number)
}
fn digest(context: StoreContext<'_>, revision: u64, previous: u64, document: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    hash.update((RECORD_VERSION as u16).to_be_bytes());
    hash.update(context.project.as_bytes());
    hash.update(context.database);
    hash.update(revision.to_be_bytes());
    hash.update(previous.to_be_bytes());
    hash.update((document.len() as u32).to_be_bytes());
    hash.update(document);
    hash.finalize().into()
}
/// Encode one complete bounded row. Trusted catalog code must allocate its actual
/// revision and commit this row atomically; encoding performs no transaction work.
pub fn encode(
    context: StoreContext<'_>,
    revision_id: u64,
    previous: u64,
    document: &[u8],
) -> Result<PolicyRecord> {
    revision(revision_id, previous)?;
    let definition = decode(document)?;
    let _ = BoundPolicy::compile(context, &definition)?;
    let checksum = digest(context, revision_id, previous, document);
    let row = vec![
        Value::Text(key(&context.database)),
        Value::Integer(RECORD_VERSION),
        Value::Text(context.project.into()),
        Value::Text(revision_id.to_string()),
        Value::Text(previous.to_string()),
        Value::Bytes(context.database.to_vec()),
        Value::Bytes(document.to_vec()),
        Value::Bytes(checksum.to_vec()),
    ];
    encode_row(&row).map_err(|_| PolicyError::Document)?;
    Ok(PolicyRecord { row })
}
/// Trusted context comes from the actual selected native component, never from
/// row fields. Checksums detect damage; they do not certify current installation.
pub fn inspect(context: StoreContext<'_>, row: &[Value]) -> Result<DecodedPolicy> {
    if !crate::valid_project_id(context.project) {
        return Err(PolicyError::Scope);
    }
    let [
        Value::Text(stored_key),
        Value::Integer(version),
        Value::Text(project),
        Value::Text(revision_id),
        Value::Text(previous_id),
        Value::Bytes(database),
        Value::Bytes(document),
        Value::Bytes(checksum),
    ] = row
    else {
        return invalid();
    };
    if *version != RECORD_VERSION {
        return Err(PolicyError::Version);
    }
    if project != context.project {
        return Err(PolicyError::Scope);
    }
    if database.len() != 16 || checksum.len() != 32 {
        return invalid();
    }
    if !bool::from(database.as_slice().ct_eq(&context.database)) {
        return Err(PolicyError::Scope);
    }
    if stored_key != &key(&context.database) {
        return invalid();
    }
    if document.len() > MAX_DOCUMENT_BYTES {
        return Err(PolicyError::Limit);
    }
    let revision_id = decimal(revision_id)?;
    let previous = decimal(previous_id)?;
    revision(revision_id, previous)?;
    let expected = digest(context, revision_id, previous, document);
    if !bool::from(checksum.as_slice().ct_eq(&expected)) {
        return invalid();
    }
    let definition = decode(document)?;
    let policy = BoundPolicy::compile(context, &definition)?;
    Ok(DecodedPolicy {
        database: context.database,
        revision: revision_id,
        previous,
        sha256: expected,
        document: document.clone(),
        policy,
    })
}
/// Exact future private catalog schema. This does not migrate or create a store.
pub fn schema() -> Schema {
    use DataType::{Bytes, Integer, Text};
    Schema {
        name: "auth_file_policies".into(),
        primary_key: 0,
        columns: [
            ("database_key", Text),
            ("version", Integer),
            ("project", Text),
            ("revision", Text),
            ("previous", Text),
            ("database", Bytes),
            ("document", Bytes),
            ("sha256", Bytes),
        ]
        .into_iter()
        .map(|(name, data_type)| Column {
            name: name.into(),
            data_type,
            nullable: false,
        })
        .collect(),
    }
}
#[cfg(test)]
mod tests;
