use crate::state::State;
use crate::{Error, Event, EventKind, Result};
use emilybase_catalog::Key;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Identifies a current row image within the caller's already validated snapshot.
/// This is not a globally unique database identity or transaction/version token.
#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowLocation {
    pub table_id: u64,
    pub page_id: u64,
    pub slot_id: u16,
    pub fingerprint: [u8; 32],
}
impl std::fmt::Debug for RowLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RowLocation")
            .field("table_id", &self.table_id)
            .field("page_id", &self.page_id)
            .field("slot_id", &self.slot_id)
            .field("fingerprint", &"[redacted]")
            .finish()
    }
}
impl RowLocation {
    pub(crate) fn from_record(event: &Event, page_id: u64, slot_id: u16, bytes: &[u8]) -> Self {
        Self {
            table_id: event.table_id,
            page_id,
            slot_id,
            fingerprint: Sha256::digest(bytes).into(),
        }
    }
    pub(crate) fn matches_record(&self, bytes: &[u8]) -> bool {
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        self.fingerprint == digest
    }
}

#[derive(Clone, Default)]
pub(crate) struct Locations(BTreeMap<u64, BTreeMap<Key, RowLocation>>);
pub(crate) enum Change {
    Put(u64, Key, RowLocation),
    Delete(u64, Key),
    Drop(u64),
    None,
}
impl Change {
    /// Compute every fallible field before state or pages mutate.
    pub(crate) fn prepare(state: &State, event: &Event, location: RowLocation) -> Result<Self> {
        let table_id = event.table_id;
        match &event.kind {
            EventKind::Insert(row) | EventKind::Replace(row) => {
                let table = state.tables.get(&table_id).ok_or(Error::NoTable)?;
                Ok(Self::Put(table_id, table.schema.key(row)?, location))
            }
            EventKind::Delete(key) => Ok(Self::Delete(table_id, key.clone())),
            EventKind::Drop => Ok(Self::Drop(table_id)),
            _ => Ok(Self::None),
        }
    }
}
impl Locations {
    pub(crate) fn get(&self, table_id: u64, key: &Key) -> Option<RowLocation> {
        self.0.get(&table_id)?.get(key).copied()
    }
    pub(crate) fn apply(&mut self, change: Change) {
        match change {
            Change::Put(table_id, key, location) => {
                self.0.entry(table_id).or_default().insert(key, location);
            }
            Change::Delete(table_id, key) => {
                if let Some(rows) = self.0.get_mut(&table_id) {
                    rows.remove(&key);
                    if rows.is_empty() {
                        self.0.remove(&table_id);
                    }
                }
            }
            Change::Drop(table_id) => {
                self.0.remove(&table_id);
            }
            Change::None => (),
        }
    }
}
