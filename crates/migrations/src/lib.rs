//! Bounded ordered SQL migrations and receipts in one original WAL transaction.
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::Snapshot;
use emilybase_query::ast::Statement;
use emilybase_transactions::Database;
use sha2::{Digest, Sha256};

/// Private by convention, not an authorization boundary against a database owner.
pub const LEDGER_TABLE: &str = "_emilybase_migrations_v1";
pub const MAX_MIGRATIONS: u32 = 128;
pub const MAX_LABEL_BYTES: usize = 63;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("migration version or label is outside the supported bounds")]
    Identity,
    #[error("migration SQL must contain only DDL/data writes and cannot address its ledger")]
    Script,
    #[error("migration must be the next consecutive version")]
    Order,
    #[error("applied migration identity or exact SQL digest differs")]
    Conflict,
    #[error("invalid migration ledger; inspect it without resetting history")]
    History,
    #[error("migration commit transaction number is exhausted")]
    Exhausted,
    #[error(transparent)]
    Syntax(#[from] emilybase_query::Error),
    #[error(transparent)]
    Execution(#[from] emilybase_query::ExecutionError),
    #[error(transparent)]
    Transaction(#[from] emilybase_transactions::Error),
    #[error(transparent)]
    Database(#[from] emilybase_database::Error),
}

/// Validated immutable input. Deliberately has no Debug implementation or SQL echo.
pub struct Migration<'a> {
    version: u32,
    label: &'a str,
    sql: &'a str,
    digest: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Receipt {
    pub version: u32,
    pub label: String,
    pub sha256: [u8; 32],
    /// Original durable commit, including when a later call is a no-op.
    pub transaction: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Applied {
    pub receipt: Receipt,
    pub already_applied: bool,
}

fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL_BYTES
        && label.as_bytes()[0].is_ascii_alphanumeric()
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Validate bounded SQL before opening a destination or taking a transaction.
/// Text bytes (including whitespace/comments), version and label bind the digest.
pub fn prepare<'a>(version: u32, label: &'a str, sql: &'a str) -> Result<Migration<'a>> {
    if !(1..=MAX_MIGRATIONS).contains(&version) || !valid_label(label) {
        return Err(Error::Identity);
    }
    for statement in emilybase_query::parse(sql)? {
        let table = match &statement {
            Statement::Create(schema) => &schema.name,
            Statement::InsertSelect { table, select, .. } => {
                if select.from.name.eq_ignore_ascii_case(LEDGER_TABLE)
                    || select
                        .join
                        .as_ref()
                        .is_some_and(|(source, _)| source.name.eq_ignore_ascii_case(LEDGER_TABLE))
                {
                    return Err(Error::Script);
                }
                table
            }
            Statement::Drop(table)
            | Statement::Insert { table, .. }
            | Statement::Update { table, .. }
            | Statement::Delete { table, .. } => table,
            _ => return Err(Error::Script),
        };
        if table.eq_ignore_ascii_case(LEDGER_TABLE) {
            return Err(Error::Script);
        }
    }
    let mut hash = Sha256::new();
    hash.update(b"emilybase-migration-v1\0");
    hash.update(version.to_be_bytes());
    // Parser/label limits above make these length conversions exact.
    hash.update((label.len() as u32).to_be_bytes());
    hash.update(label.as_bytes());
    hash.update((sql.len() as u32).to_be_bytes());
    hash.update(sql.as_bytes());
    Ok(Migration {
        version,
        label,
        sql,
        digest: hash.finalize().into(),
    })
}

fn schema() -> Schema {
    Schema {
        name: LEDGER_TABLE.into(),
        primary_key: 0,
        columns: [
            ("version", DataType::Integer),
            ("label", DataType::Text),
            ("sha256", DataType::Bytes),
            ("transaction", DataType::Text),
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

fn history(snapshot: &Snapshot, last_transaction: u64) -> Result<Vec<Receipt>> {
    let actual = match snapshot.schema(LEDGER_TABLE) {
        Ok(schema) => schema,
        Err(emilybase_database::Error::NoTable) => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if actual != &schema() {
        return Err(Error::History);
    }
    let mut receipts = Vec::new();
    // Transaction 1 initializes the engine root and cannot contain a migration.
    let mut previous_transaction = 1;
    for row in snapshot.primary_rows(LEDGER_TABLE, None, None)? {
        if receipts.len() >= MAX_MIGRATIONS as usize {
            return Err(Error::History);
        }
        let row = row?;
        let [
            Value::Integer(version),
            Value::Text(label),
            Value::Bytes(digest),
            Value::Text(transaction),
        ] = row.as_slice()
        else {
            return Err(Error::History);
        };
        let expected = receipts.len() as u32 + 1;
        if *version != i64::from(expected) || !valid_label(label) {
            return Err(Error::History);
        }
        let sha256 = digest.as_slice().try_into().map_err(|_| Error::History)?;
        if transaction.is_empty()
            || transaction.len() > 20
            || transaction.starts_with('0')
            || !transaction.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(Error::History);
        }
        let transaction = transaction.parse::<u64>().map_err(|_| Error::History)?;
        if transaction <= previous_transaction || transaction > last_transaction {
            return Err(Error::History);
        }
        previous_transaction = transaction;
        receipts.push(Receipt {
            version: expected,
            label: label.clone(),
            sha256,
            transaction,
        });
    }
    // The first ledger and its first receipt are always committed together.
    if receipts.is_empty() {
        return Err(Error::History);
    }
    Ok(receipts)
}

/// Read bounded validated receipt metadata without modifying WAL or repairing it.
pub fn inspect(database: &Database) -> Result<Vec<Receipt>> {
    history(database.view()?, database.last_transaction())
}

/// Apply exactly the next migration, or verify an identical historical no-op.
/// ACK follows the one original-engine WAL commit containing SQL and receipt.
/// Ambiguous engine write errors are propagated; reopen/inspect before retrying.
pub fn apply(database: &mut Database, migration: &Migration<'_>) -> Result<Applied> {
    apply_inner(database, migration, || {})
}

fn apply_inner(
    database: &mut Database,
    migration: &Migration<'_>,
    before_commit: impl FnOnce(),
) -> Result<Applied> {
    let receipts = inspect(database)?;
    if let Some(receipt) = receipts.get(migration.version as usize - 1) {
        if receipt.label != migration.label || receipt.sha256 != migration.digest {
            return Err(Error::Conflict);
        }
        return Ok(Applied {
            receipt: receipt.clone(),
            already_applied: true,
        });
    }
    if migration.version != receipts.len() as u32 + 1 {
        return Err(Error::Order);
    }
    let next_transaction = database
        .last_transaction()
        .checked_add(1)
        .ok_or(Error::Exhausted)?;
    let mut transaction = database.begin()?;
    if receipts.is_empty() {
        transaction.create_table(schema())?;
    }
    let (mut transaction, _) =
        emilybase_query::stage(transaction, migration.sql, &[])?.into_parts();
    transaction.insert(
        LEDGER_TABLE,
        vec![
            Value::Integer(i64::from(migration.version)),
            Value::Text(migration.label.into()),
            Value::Bytes(migration.digest.to_vec()),
            Value::Text(next_transaction.to_string()),
        ],
    )?;
    before_commit();
    let transaction = transaction.commit()?;
    Ok(Applied {
        receipt: Receipt {
            version: migration.version,
            label: migration.label.into(),
            sha256: migration.digest,
            transaction,
        },
        already_applied: false,
    })
}

#[cfg(test)]
#[path = "crash_tests.rs"]
mod crash_tests;
