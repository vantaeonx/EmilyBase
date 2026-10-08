//! Typed public-table operations; private account schemas live in separate stores.
use emilybase_catalog::{Schema, encode_schema};
use emilybase_transactions::Database;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum TableError {
    #[error("invalid table request document")]
    Document,
    #[error("table response limit exceeded")]
    Limit,
    #[error(transparent)]
    Catalog(#[from] emilybase_catalog::Error),
    #[error(transparent)]
    Database(#[from] emilybase_database::Error),
    #[error(transparent)]
    Transaction(#[from] emilybase_transactions::Error),
}
pub(crate) type Result<T> = std::result::Result<T, TableError>;
#[derive(Serialize)]
pub(crate) struct TableSummary {
    id: String,
    name: String,
    columns: usize,
    primary_key: String,
}
#[derive(Serialize)]
pub(crate) struct TableList {
    tables: Vec<TableSummary>,
}
#[derive(Serialize)]
pub(crate) struct TableCreated {
    table: TableSummary,
    transaction: String,
}
#[derive(Serialize)]
pub(crate) struct TableDropped {
    transaction: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Name {
    table: String,
}
pub(crate) fn name_request(bytes: &[u8]) -> Result<String> {
    serde_json::from_slice::<Name>(bytes)
        .map(|n| n.table)
        .map_err(|_| TableError::Document)
}
pub(crate) fn schema_request(bytes: &[u8]) -> Result<Schema> {
    let schema: Schema = serde_json::from_slice(bytes).map_err(|_| TableError::Document)?;
    schema.validate()?;
    encode_schema(&schema)?;
    Ok(schema)
}
fn summary(id: u64, schema: &Schema) -> Result<TableSummary> {
    schema.validate()?;
    let primary = schema
        .columns
        .get(usize::from(schema.primary_key))
        .ok_or(emilybase_catalog::Error::PrimaryKey)?;
    Ok(TableSummary {
        id: id.to_string(),
        name: schema.name.clone(),
        columns: schema.columns.len(),
        primary_key: primary.name.clone(),
    })
}
pub(crate) fn list(database: &mut Database) -> Result<TableList> {
    let snapshot = database.view()?;
    let schemas = snapshot.schema_refs();
    if schemas.len() > emilybase_database::MAX_TABLES {
        return Err(TableError::Limit);
    }
    let mut tables = Vec::with_capacity(schemas.len());
    for schema in schemas {
        tables.push(summary(snapshot.table_id(&schema.name)?, schema)?);
    }
    Ok(TableList { tables })
}
pub(crate) fn describe(database: &mut Database, name: &str) -> Result<Schema> {
    Ok(database.view()?.schema(name)?.clone())
}
pub(crate) fn create(database: &mut Database, schema: Schema) -> Result<TableCreated> {
    let mut metadata = summary(0, &schema)?;
    let mut tx = database.begin()?;
    let id = tx.create_table(schema)?;
    let transaction = tx.commit()?.to_string();
    metadata.id = id.to_string();
    Ok(TableCreated {
        table: metadata,
        transaction,
    })
}
pub(crate) fn drop_table(database: &mut Database, name: &str) -> Result<TableDropped> {
    let mut tx = database.begin()?;
    tx.drop_table(name)?;
    Ok(TableDropped {
        transaction: tx.commit()?.to_string(),
    })
}
pub(crate) fn response(value: &impl Serialize) -> Result<axum::response::Response> {
    let bytes = serde_json::to_vec(value).map_err(|_| TableError::Document)?;
    if bytes.len() > crate::http::MAX_BODY {
        return Err(TableError::Limit);
    }
    Ok(crate::http::transfer_response(bytes))
}

#[cfg(test)]
mod tests;
