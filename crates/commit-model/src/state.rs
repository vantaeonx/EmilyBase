use std::collections::BTreeMap;
use std::sync::Arc;

use emilybase_catalog::DataType;
use emilybase_commit_format::{DatabaseId, IndexKeyType, PageAddress};
use emilybase_database::Snapshot;
use sha2::{Digest, Sha256};

use crate::{Error, Prepared, Result, Selection, Staged};

pub(crate) struct State {
    pub database: DatabaseId,
    pub transaction: u64,
    pub relational: Snapshot,
    pub selected: BTreeMap<u64, Arc<Selection>>,
    pub fingerprint: [u8; 32],
}

/// Immutable historical views share a complete state. Publication swaps one Arc.
#[derive(Clone)]
pub struct Model {
    pub(crate) state: Arc<State>,
}

impl Model {
    /// Synthetic initialized state at logical transaction one; no file is created.
    pub fn new(database: DatabaseId) -> Result<Self> {
        PageAddress::history(database, 1)?;
        Ok(Self {
            state: State::validated(database, 1, Snapshot::empty()?, BTreeMap::new())?,
        })
    }

    pub fn database_id(&self) -> DatabaseId {
        self.state.database
    }
    pub fn transaction(&self) -> u64 {
        self.state.transaction
    }
    pub fn view(&self) -> &Snapshot {
        &self.state.relational
    }
    pub fn selection(&self, table: u64) -> Option<&Selection> {
        self.state.selected.get(&table).map(Arc::as_ref)
    }
    pub fn fingerprint(&self) -> [u8; 32] {
        self.state.fingerprint
    }
    pub fn begin(&self) -> Result<Staged> {
        Staged::new(Arc::clone(&self.state))
    }

    /// Memory publication only. A future durable writer must sync its single
    /// commit fence before exposing such a state; this method is not that writer.
    pub fn publish(&mut self, prepared: Prepared) -> Result<()> {
        if self.state.fingerprint != prepared.base {
            return Err(Error::Conflict);
        }
        self.state = prepared.next;
        Ok(())
    }
}

impl State {
    pub fn validated(
        database: DatabaseId,
        transaction: u64,
        relational: Snapshot,
        selected: BTreeMap<u64, Arc<Selection>>,
    ) -> Result<Arc<Self>> {
        let schemas = relational.schemas();
        if schemas.len() != selected.len() || selected.len() > emilybase_database::MAX_TABLES {
            return Err(Error::Selection("incomplete or extra table roots"));
        }
        for schema in schemas {
            let table = relational.table_id(&schema.name)?;
            let selection = selected
                .get(&table)
                .ok_or(Error::Selection("missing table root"))?;
            let binding = selection.binding;
            binding.verify_owner(database, table, binding.transaction())?;
            if binding.transaction() > transaction {
                return Err(Error::Selection("root belongs to a later transaction"));
            }
            let key_type = match schema.columns.get(usize::from(schema.primary_key)) {
                Some(column) if column.data_type == DataType::Integer => IndexKeyType::Integer,
                Some(column) if column.data_type == DataType::Text => IndexKeyType::Text,
                _ => return Err(Error::Selection("primary key type")),
            };
            let info = relational.verify_primary_tree(&schema.name, &selection.index.tree)?;
            if binding.key_type() != key_type
                || binding.covered() != info.entries as u64
                || binding.excluded() != info.excluded_long_keys as u64
            {
                return Err(Error::Selection("root/live row coverage mismatch"));
            }
        }
        // Fixed-width parts, sorted roots and an explicit count make composition
        // unambiguous. This public state digest is not a credential or MAC.
        let mut digest = Sha256::new();
        digest.update(b"EBMODEL\0");
        digest.update(database);
        digest.update(transaction.to_le_bytes());
        digest.update(relational.page_fingerprint());
        digest.update((selected.len() as u32).to_le_bytes());
        for selection in selected.values() {
            digest.update(selection.binding.encode()?);
            digest.update(selection.index.fingerprint()?);
        }
        Ok(Arc::new(Self {
            database,
            transaction,
            relational,
            selected,
            fingerprint: digest.finalize().into(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_transaction_refuses_staging_without_wrapping_or_changing_state() {
        let model = Model {
            state: State::validated(
                [7; 16],
                emilybase_commit_format::MAX_TRANSACTION,
                Snapshot::empty().unwrap(),
                BTreeMap::new(),
            )
            .unwrap(),
        };
        let before = model.fingerprint();
        assert!(matches!(model.begin(), Err(Error::Limit)));
        assert_eq!(model.fingerprint(), before);
    }
}
