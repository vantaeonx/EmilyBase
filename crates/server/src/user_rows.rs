//! Synchronous typed user-row enforcement; no SQL, DDL or public HTTP authority.
mod page;
/// Maximum number of policy-visible rows retained by one native page.
pub const MAX_USER_PAGE_ROWS: usize = 128;
use emilybase_auth::{
    accounts::PolicyPrincipal,
    row_policy::{Change, PolicyError, TableContext},
};
use emilybase_catalog::{Key, Row};
use emilybase_transactions::{Database, MAX_TRANSACTION_EVENTS};

#[derive(Debug, thiserror::Error)]
pub enum UserRowsError {
    #[error("invalid bounded user row operation")]
    Input,
    #[error("user row write rejected")]
    Rejected,
    #[error("user row policy decision failed")]
    Policy(#[source] PolicyError),
    #[error("user row storage operation failed; inspect before retrying")]
    Storage(#[source] emilybase_transactions::Error),
    #[error("user row lookup failed")]
    Database(#[from] emilybase_database::Error),
}
pub enum UserWrite {
    Insert(Row),
    Update { key: Key, row: Row },
    Delete(Key),
}
pub enum UserTableOperation {
    Get(Key),
    Page { after: Option<Key>, limit: usize },
    Write(Vec<UserWrite>),
}
#[derive(PartialEq)]
pub enum UserTableResult {
    Row(Option<Row>),
    Page { rows: Vec<Row>, next: Option<Key> },
    Committed { transaction: u64, operations: usize },
}
macro_rules! redacted {
    ($($name:ty),*) => {$ (
        impl std::fmt::Debug for $name {
            fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str(concat!(stringify!($name),"(redacted)"))}
        }
    )*};
}
redacted!(UserWrite, UserTableOperation, UserTableResult);

pub(crate) fn validate(table: &str, operation: &UserTableOperation) -> Result<(), UserRowsError> {
    // This conventional ledger remains service-writable, never user-writable.
    if table.eq_ignore_ascii_case(emilybase_migrations::LEDGER_TABLE) {
        return Err(UserRowsError::Rejected);
    }
    if let UserTableOperation::Write(writes) = operation
        && (writes.is_empty() || writes.len() > MAX_TRANSACTION_EVENTS)
    {
        return Err(UserRowsError::Input);
    }
    if let UserTableOperation::Page { limit, .. } = operation
        && !(1..=MAX_USER_PAGE_ROWS).contains(limit)
    {
        return Err(UserRowsError::Input);
    }
    Ok(())
}
fn transaction(error: emilybase_transactions::Error) -> UserRowsError {
    use emilybase_database::Error as D;
    use emilybase_transactions::Error as T;
    match error {
        T::Database(D::DuplicateKey | D::NoRow | D::PrimaryKeyChange) => UserRowsError::Rejected,
        T::Catalog(_) | T::Database(D::Catalog(_)) => UserRowsError::Input,
        other => UserRowsError::Storage(other),
    }
}
fn decision(result: Result<(), PolicyError>) -> Result<(), UserRowsError> {
    result.map_err(UserRowsError::Policy)
}
pub(crate) fn run(
    database: &mut Database,
    proof: &PolicyPrincipal<'_>,
    context: TableContext<'_>,
    operation: UserTableOperation,
) -> Result<UserTableResult, UserRowsError> {
    validate(&context.schema.name, &operation)?;
    proof
        .check_context(context)
        .map_err(UserRowsError::Policy)?;
    match operation {
        UserTableOperation::Page { after, limit } => {
            page::run(database, proof, context, after.as_ref(), limit)
        }
        UserTableOperation::Get(key) => {
            let Some(row) = database
                .view()
                .map_err(transaction)?
                .get(&context.schema.name, &key)?
            else {
                return Ok(UserTableResult::Row(None));
            };
            match proof.authorize(context, Change::Select(row)) {
                Ok(()) => Ok(UserTableResult::Row(Some(row.clone()))),
                Err(PolicyError::Denied) => Ok(UserTableResult::Row(None)),
                Err(error) => Err(UserRowsError::Policy(error)),
            }
        }
        UserTableOperation::Write(writes) => {
            let count = writes.len();
            let mut tx = database.begin().map_err(transaction)?;
            for write in writes {
                match write {
                    UserWrite::Insert(row) => {
                        decision(proof.authorize(context, Change::Insert(&row)))?;
                        tx.insert(&context.schema.name, row).map_err(transaction)?;
                    }
                    UserWrite::Update { key, row } => {
                        let old = tx
                            .view()
                            .map_err(transaction)?
                            .get(&context.schema.name, &key)?
                            .ok_or(UserRowsError::Rejected)?;
                        decision(proof.authorize(context, Change::Update { old, new: &row }))?;
                        tx.update(&context.schema.name, &key, row)
                            .map_err(transaction)?;
                    }
                    UserWrite::Delete(key) => {
                        let old = tx
                            .view()
                            .map_err(transaction)?
                            .get(&context.schema.name, &key)?
                            .ok_or(UserRowsError::Rejected)?;
                        decision(proof.authorize(context, Change::Delete(old)))?;
                        tx.delete(&context.schema.name, &key).map_err(transaction)?;
                    }
                }
            }
            #[cfg(test)]
            crate::durability::checkpoint("user_table_staged");
            let transaction = tx.commit().map_err(transaction)?;
            Ok(UserTableResult::Committed {
                transaction,
                operations: count,
            })
        }
    }
}
