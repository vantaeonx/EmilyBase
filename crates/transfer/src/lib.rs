//! Bounded logical table exchange; imports use the original synchronous WAL engine.
mod bounded;
mod wire;
use emilybase_catalog::{Key, Row, Schema, encode_row, encode_schema};
use emilybase_database::Snapshot;
use emilybase_transactions::Database;
use std::io::{Read, Write};

pub const TRANSFER_VERSION: u16 = 1;
pub const MAX_TRANSFER_ROWS: usize = emilybase_transactions::MAX_TRANSACTION_EVENTS - 1;
pub const MAX_TRANSFER_BYTES: usize = 8 * 1024 * 1024;
const MAGIC: &str = "emilybase-table";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("table transfer byte or element limit exceeded")]
    Limit,
    #[error("invalid table transfer document")]
    Document,
    #[error("unsupported table transfer version")]
    Version,
    #[error("table transfer primary keys must be strictly ascending")]
    Order,
    #[error("table transfer target already exists")]
    Existing,
    #[error(transparent)]
    Catalog(#[from] emilybase_catalog::Error),
    #[error(transparent)]
    Database(#[from] emilybase_database::Error),
    #[error(transparent)]
    Transactions(#[from] emilybase_transactions::Error),
    #[error("table transfer stream error")]
    Io(#[source] std::io::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

/// Counts only. Transfer documents deliberately contain caller-selected plaintext.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct Report {
    pub version: u16,
    pub rows: usize,
    pub columns: usize,
    pub bytes: usize,
}
/// Fully validated owned input. No public fields or unchecked constructor.
pub struct VerifiedTable {
    schema: Schema,
    rows: Vec<Row>,
    report: Report,
}
impl std::fmt::Debug for VerifiedTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedTable")
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}
impl VerifiedTable {
    pub fn report(&self) -> &Report {
        &self.report
    }
}

/// Read one bounded stream before any destination database is opened.
pub fn read_table(reader: impl Read) -> Result<VerifiedTable> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_TRANSFER_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(Error::Io)?;
    decode_table(&bytes)
}
pub fn decode_table(bytes: &[u8]) -> Result<VerifiedTable> {
    if bytes.len() > MAX_TRANSFER_BYTES {
        return Err(Error::Limit);
    }
    let document: wire::Input = serde_json::from_slice(bytes).map_err(|_| Error::Document)?;
    if document.format.0 != MAGIC {
        return Err(Error::Document);
    }
    if document.version != TRANSFER_VERSION {
        return Err(Error::Version);
    }
    let schema = document.schema.into_schema();
    schema.validate()?;
    encode_schema(&schema)?;
    let mut previous: Option<Key> = None;
    let mut rows = Vec::with_capacity(document.rows.0.len());
    for input in document.rows.0 {
        let row = input
            .0
            .into_iter()
            .map(wire::InputValue::into_value)
            .collect::<Result<Row>>()?;
        let key = schema.key(&row)?;
        encode_row(&row)?;
        if previous.as_ref().is_some_and(|old| old >= &key) {
            return Err(Error::Order);
        }
        previous = Some(key);
        rows.push(row);
    }
    let report = Report {
        version: TRANSFER_VERSION,
        rows: rows.len(),
        columns: schema.columns.len(),
        bytes: bytes.len(),
    };
    Ok(VerifiedTable {
        schema,
        rows,
        report,
    })
}

struct BoundedOutput(Vec<u8>);
impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_TRANSFER_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("table transfer output limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
/// Complete selected table in checked primary order. Never silently truncate.
pub fn export_table(snapshot: &Snapshot, table: &str) -> Result<Vec<u8>> {
    let schema = snapshot.schema(table)?;
    encode_schema(schema)?;
    let mut rows = Vec::new();
    for row in snapshot.primary_rows(table, None, None)? {
        let row = row?;
        if rows.len() == MAX_TRANSFER_ROWS {
            return Err(Error::Limit);
        }
        schema.validate_row(row)?;
        encode_row(row)?;
        rows.push(row);
    }
    let mut output = BoundedOutput(Vec::new());
    serde_json::to_writer(
        &mut output,
        &wire::Output {
            format: MAGIC,
            version: TRANSFER_VERSION,
            schema,
            rows: &rows,
        },
    )
    .map_err(|_| Error::Limit)?;
    Ok(output.0)
}
/// Create and populate a new table in one commit. Existing tables are never merged.
pub fn import_table(database: &mut Database, table: VerifiedTable) -> Result<u64> {
    import_after_stage(database, table, || {})
}
fn import_after_stage(
    database: &mut Database,
    table: VerifiedTable,
    staged: impl FnOnce(),
) -> Result<u64> {
    if database
        .view()?
        .schema_refs()
        .any(|schema| schema.name == table.schema.name)
    {
        return Err(Error::Existing);
    }
    let name = table.schema.name.clone();
    let mut tx = database.begin()?;
    tx.create_table(table.schema)?;
    for row in table.rows {
        tx.insert(&name, row)?;
    }
    staged();
    Ok(tx.commit()?)
}

#[cfg(test)]
mod tests;
