use crate::{Database, Error, Result, Transaction};
use emilybase_catalog::{Key, Row};
use emilybase_database::RowLocation;
use emilybase_wal::DatabaseId;

/// A row image scoped to the persistent WAL identity. Requires ordinary data authorization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundRowLocation {
    pub database_id: DatabaseId,
    pub row: RowLocation,
}

impl Database {
    pub fn row_location(&self, table: &str, key: &Key) -> Result<Option<BoundRowLocation>> {
        Ok(self
            .view()?
            .row_location(table, key)?
            .map(|row| BoundRowLocation {
                database_id: self.database_id(),
                row,
            }))
    }
    pub fn resolve_row_location(
        &self,
        table: &str,
        key: &Key,
        location: BoundRowLocation,
    ) -> Result<&Row> {
        self.ready()?;
        if location.database_id != self.database_id() {
            return Err(Error::LocationDatabase);
        }
        Ok(self
            .view()?
            .resolve_row_location(table, key, location.row)?)
    }
}
impl Transaction<'_> {
    pub fn row_location(&self, table: &str, key: &Key) -> Result<Option<BoundRowLocation>> {
        Ok(self
            .view()?
            .row_location(table, key)?
            .map(|row| BoundRowLocation {
                database_id: self.database.database_id(),
                row,
            }))
    }
    pub fn resolve_row_location(
        &self,
        table: &str,
        key: &Key,
        location: BoundRowLocation,
    ) -> Result<&Row> {
        let snapshot = self.view()?;
        if location.database_id != self.database.database_id() {
            return Err(Error::LocationDatabase);
        }
        Ok(snapshot.resolve_row_location(table, key, location.row)?)
    }
}
