//! Explicit v5 metadata, closed on migration and verified private restore.
use super::{AccountStore, Error, Result, SCOPE};
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::Snapshot;
use serde::Serialize;

pub(super) const TABLE: &str = "auth_public_admission";

/// Operator metadata only. A copied receipt never authorizes a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicAdmissionReceipt {
    pub enabled: bool,
    pub revision: u64,
    pub previous: u64,
}
pub(super) fn schema() -> Schema {
    Schema {
        name: TABLE.into(),
        primary_key: 0,
        columns: [
            ("id", DataType::Integer),
            ("version", DataType::Integer),
            ("enabled", DataType::Boolean),
            ("revision", DataType::Text),
            ("previous", DataType::Text),
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
fn row(receipt: &PublicAdmissionReceipt) -> Vec<Value> {
    vec![
        Value::Integer(1),
        Value::Integer(1),
        Value::Boolean(receipt.enabled),
        Value::Text(receipt.revision.to_string()),
        Value::Text(receipt.previous.to_string()),
    ]
}
fn canonical(value: &str) -> Result<u64> {
    let parsed = value.parse::<u64>().map_err(|_| Error::Corrupt)?;
    if parsed.to_string() != value {
        return Err(Error::Corrupt);
    }
    Ok(parsed)
}
pub(super) fn validate(snapshot: &Snapshot, last: u64) -> Result<PublicAdmissionReceipt> {
    if snapshot.table_count() != 8
        || snapshot.schema(TABLE).map_err(|_| Error::Corrupt)? != &schema()
    {
        return Err(Error::Corrupt);
    }
    let mut rows = snapshot
        .primary_rows(TABLE, None, None)
        .map_err(|_| Error::Corrupt)?;
    let value = rows
        .next()
        .transpose()
        .map_err(|_| Error::Corrupt)?
        .ok_or(Error::Corrupt)?;
    let [
        Value::Integer(1),
        Value::Integer(1),
        Value::Boolean(enabled),
        Value::Text(revision),
        Value::Text(previous),
    ] = value.as_slice()
    else {
        return Err(Error::Corrupt);
    };
    let revision = canonical(revision)?;
    let previous = canonical(previous)?;
    if rows.next().is_some()
        || revision == 0
        || revision > last
        || previous >= revision
        || (previous == 0 && *enabled)
    {
        return Err(Error::Corrupt);
    }
    Ok(PublicAdmissionReceipt {
        enabled: *enabled,
        revision,
        previous,
    })
}
fn next(last: u64, enabled: bool, previous: u64) -> Result<PublicAdmissionReceipt> {
    Ok(PublicAdmissionReceipt {
        enabled,
        revision: last
            .checked_add(1)
            .filter(|value| *value < u64::MAX)
            .ok_or(Error::AdmissionCapacity)?,
        previous,
    })
}
/// Prepare closure in the same transaction that replaces the session incarnation.
pub(super) fn close_for_reset(snapshot: &Snapshot, last: u64) -> Result<Option<Vec<Value>>> {
    if super::policy_catalog::version(snapshot)? != 5 {
        return Ok(None);
    }
    let current = validate(snapshot, last)?;
    if !current.enabled {
        return Ok(None);
    }
    Ok(Some(row(&next(last, false, current.revision)?)))
}
impl AccountStore {
    /// Current internal schema metadata, not a request grant or migration.
    pub fn private_schema_version(&self) -> Result<u16> {
        let version = super::policy_catalog::version(self.database.view()?)?;
        if !(1..=5).contains(&version) {
            return Err(Error::Corrupt);
        }
        Ok(version as u16)
    }
    /// Explicit v4-to-v5 migration. Does not open admission or change sessions.
    pub fn enable_public_admission_catalog(&mut self) -> Result<PublicAdmissionReceipt> {
        self.enable_public_admission_catalog_with(|| {})
    }
    pub(super) fn enable_public_admission_catalog_with(
        &mut self,
        before_commit: impl FnOnce(),
    ) -> Result<PublicAdmissionReceipt> {
        let state = super::archive::validate_snapshot(
            self.database.view()?,
            &self.project,
            self.database.last_transaction(),
        )?;
        if state.version == 5 {
            return self.public_admission();
        }
        if state.version != 4 {
            return Err(Error::AdmissionSchema);
        }
        let receipt = next(self.database.last_transaction(), false, 0)?;
        let mut scope = self
            .database
            .view()?
            .get(SCOPE, &Key::Integer(1))
            .map_err(|_| Error::Corrupt)?
            .ok_or(Error::Corrupt)?
            .clone();
        scope[1] = Value::Integer(5);
        let mut tx = self.database.begin()?;
        tx.create_table(schema())?;
        tx.insert(TABLE, row(&receipt))?;
        tx.update(SCOPE, &Key::Integer(1), scope)?;
        before_commit();
        tx.commit()?;
        Ok(receipt)
    }
    /// Current metadata from this exclusive private owner, not a request grant.
    pub fn public_admission(&self) -> Result<PublicAdmissionReceipt> {
        let snapshot = self.database.view()?;
        if super::policy_catalog::version(snapshot)? != 5 {
            return Err(Error::AdmissionSchema);
        }
        validate(snapshot, self.database.last_transaction())
    }
    /// Trusted operator CAS. Identical retries accept only the recorded predecessor
    /// or current revision; unrelated private commits do not replace this receipt.
    pub fn set_public_admission(
        &mut self,
        expected: u64,
        enabled: bool,
    ) -> Result<PublicAdmissionReceipt> {
        self.set_public_admission_with(expected, enabled, || {})
    }
    pub(super) fn set_public_admission_with(
        &mut self,
        expected: u64,
        enabled: bool,
        before_commit: impl FnOnce(),
    ) -> Result<PublicAdmissionReceipt> {
        let current = self.public_admission()?;
        if current.enabled == enabled
            && (expected == current.revision || expected == current.previous)
        {
            return Ok(current);
        }
        if expected != current.revision {
            return Err(Error::AdmissionConflict);
        }
        let receipt = next(self.database.last_transaction(), enabled, current.revision)?;
        let mut tx = self.database.begin()?;
        tx.update(TABLE, &Key::Integer(1), row(&receipt))?;
        before_commit();
        tx.commit()?;
        Ok(receipt)
    }
}
