//! Bounded service row operations on the original synchronous catalog and WAL.
mod wire;
use crate::table_api::{Result, TableError};
use emilybase_catalog::{Key, MAX_COLUMNS, Row, encode_row};
use emilybase_transactions::Database;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::Write;
use wire::{Input, InputKey, OutputKey, OutputRow};

#[derive(Clone, Copy)]
pub enum Operation {
    Get,
    Page,
    Insert,
    Update,
    Delete,
    Batch,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Point {
    table: String,
    key: InputKey,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    table: String,
    after: Option<InputKey>,
    limit: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Insert {
    table: String,
    row: Vec<Input>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Update {
    table: String,
    key: InputKey,
    row: Vec<Input>,
}
fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    if bytes.len() > crate::http::MAX_BODY {
        return Err(TableError::Limit);
    }
    serde_json::from_slice(bytes).map_err(|_| TableError::Document)
}
fn row(input: Vec<Input>) -> Result<Row> {
    if input.len() > MAX_COLUMNS {
        return Err(TableError::Limit);
    }
    let row = input
        .into_iter()
        .map(Input::value)
        .collect::<Result<Row>>()?;
    encode_row(&row)?;
    Ok(row)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    table: String,
    operations: Vec<InputWrite>,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum InputWrite {
    Insert { row: Vec<Input> },
    Update { key: InputKey, row: Vec<Input> },
    Delete { key: InputKey },
}
enum WriteOperation {
    Insert(Row),
    Update(Key, Row),
    Delete(Key),
}
fn writes(input: Vec<InputWrite>) -> Result<Vec<WriteOperation>> {
    if input.is_empty() || input.len() > emilybase_transactions::MAX_TRANSACTION_EVENTS {
        return Err(TableError::Limit);
    }
    input
        .into_iter()
        .map(|op| {
            Ok(match op {
                InputWrite::Insert { row: values } => WriteOperation::Insert(row(values)?),
                InputWrite::Update { key, row: values } => {
                    WriteOperation::Update(key.key()?, row(values)?)
                }
                InputWrite::Delete { key } => WriteOperation::Delete(key.key()?),
            })
        })
        .collect()
}
#[derive(Serialize)]
struct BatchChanged {
    changed: usize,
    transaction: String,
}
fn batch_after_stage(
    db: &mut Database,
    table: &str,
    operations: Vec<WriteOperation>,
    staged: impl FnOnce(),
) -> Result<axum::response::Response> {
    let changed = operations.len();
    let mut tx = db.begin()?;
    for op in operations {
        match op {
            WriteOperation::Insert(row) => {
                tx.insert(table, row)?;
            }
            WriteOperation::Update(key, row) => tx.update(table, &key, row)?,
            WriteOperation::Delete(key) => tx.delete(table, &key)?,
        }
    }
    staged();
    let transaction = tx.commit()?.to_string();
    response(&BatchChanged {
        changed,
        transaction,
    })
}
struct Output {
    bytes: Vec<u8>,
}
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > crate::http::MAX_BODY.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("row response limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn response(value: &impl Serialize) -> Result<axum::response::Response> {
    let mut output = Output { bytes: Vec::new() };
    serde_json::to_writer(&mut output, value).map_err(|_| TableError::Limit)?;
    Ok(crate::http::transfer_response(output.bytes))
}
#[derive(Serialize)]
struct Found<'a> {
    row: Option<OutputRow<'a>>,
}
#[derive(Serialize)]
struct Rows<'a> {
    rows: Vec<OutputRow<'a>>,
    next: Option<OutputKey<'a>>,
}
#[derive(Serialize)]
struct Changed<'a> {
    key: OutputKey<'a>,
    transaction: String,
}
fn changed(key: &Key, transaction: u64) -> Result<axum::response::Response> {
    response(&Changed {
        key: OutputKey::from(key),
        transaction: transaction.to_string(),
    })
}
enum Prepared {
    Get(String, Key),
    Page(String, Option<Key>, usize),
    Insert(String, Row),
    Update(String, Key, Row),
    Delete(String, Key),
    Batch(String, Vec<WriteOperation>),
}
fn decode(op: Operation, bytes: &[u8]) -> Result<Prepared> {
    Ok(match op {
        Operation::Batch => {
            let input: Batch = parse(bytes)?;
            Prepared::Batch(input.table, writes(input.operations)?)
        }
        Operation::Get | Operation::Delete => {
            let input: Point = parse(bytes)?;
            let key = input.key.key()?;
            match op {
                Operation::Get => Prepared::Get(input.table, key),
                _ => Prepared::Delete(input.table, key),
            }
        }
        Operation::Page => {
            let input: Page = parse(bytes)?;
            if !(1..=128).contains(&input.limit) {
                return Err(TableError::Limit);
            }
            Prepared::Page(
                input.table,
                input.after.map(InputKey::key).transpose()?,
                input.limit,
            )
        }
        Operation::Insert => {
            let input: Insert = parse(bytes)?;
            Prepared::Insert(input.table, row(input.row)?)
        }
        Operation::Update => {
            let input: Update = parse(bytes)?;
            Prepared::Update(input.table, input.key.key()?, row(input.row)?)
        }
    })
}
/// Validate bounded row transport grammar/values without opening a database.
/// Schema-specific types, existence and authority require the actual scoped operation.
pub fn validate_row_request(op: Operation, bytes: &[u8]) -> Result<()> {
    decode(op, bytes).map(drop)
}
pub(crate) fn run(
    db: &mut Database,
    op: Operation,
    bytes: &[u8],
) -> Result<axum::response::Response> {
    match decode(op, bytes)? {
        Prepared::Batch(table, operations) => batch_after_stage(db, &table, operations, || {}),
        Prepared::Get(table, key) => {
            let snapshot = db.view()?;
            let schema = snapshot.schema(&table)?;
            let row = snapshot
                .primary_rows(&table, Some(&key), None)?
                .next()
                .transpose()?;
            let found = match row {
                Some(r) if schema.key(r)? == key => Some(r),
                _ => None,
            };
            response(&Found {
                row: found.map(OutputRow),
            })
        }
        Prepared::Page(table, after, limit) => {
            let snapshot = db.view()?;
            let schema = snapshot.schema(&table)?;
            let mut rows = Vec::with_capacity(limit);
            let mut last = None;
            let mut more = false;
            for item in snapshot.primary_rows(&table, after.as_ref(), None)? {
                let row = item?;
                let key = schema.key(row)?;
                if after.as_ref().is_some_and(|a| key <= *a) {
                    continue;
                }
                if rows.len() == limit {
                    more = true;
                    break;
                }
                rows.push(OutputRow(row));
                last = Some(key);
            }
            response(&Rows {
                rows,
                next: if more {
                    last.as_ref().map(OutputKey::from)
                } else {
                    None
                },
            })
        }
        Prepared::Insert(table, row) => {
            let mut tx = db.begin()?;
            let key = tx.insert(&table, row)?;
            let transaction = tx.commit()?;
            changed(&key, transaction)
        }
        Prepared::Update(table, key, row) => {
            let mut tx = db.begin()?;
            tx.update(&table, &key, row)?;
            let transaction = tx.commit()?;
            changed(&key, transaction)
        }
        Prepared::Delete(table, key) => {
            let mut tx = db.begin()?;
            tx.delete(&table, &key)?;
            let transaction = tx.commit()?;
            changed(&key, transaction)
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod batch_tests;

#[cfg(test)]
mod page_tests;
