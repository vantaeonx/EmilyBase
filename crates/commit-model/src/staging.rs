use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use emilybase_catalog::DataType;
use emilybase_commit_format::{
    IndexKeyType, MAX_TRANSACTION, PageAddress, Predecessor, RootBinding,
};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_index::IndexSnapshot;

use crate::state::State;
use crate::{EncodedComponents, Error, MAX_EVENTS, MAX_SELECTED_INDEX_PAGES, Result, Selection};

pub struct Staged {
    base: Arc<State>,
    relational: Snapshot,
    transaction: u64,
    events: usize,
    touched: BTreeSet<u64>,
    candidates: BTreeMap<u64, Selection>,
    candidate_pages: usize,
    aborted: bool,
}

pub struct Prepared {
    pub(crate) base: [u8; 32],
    pub(crate) next: Arc<State>,
    retired_tables: Vec<u64>,
    pub(crate) previous: Arc<State>,
}

impl Staged {
    pub(crate) fn new(base: Arc<State>) -> Result<Self> {
        let transaction = base
            .transaction
            .checked_add(1)
            .filter(|tx| *tx <= MAX_TRANSACTION)
            .ok_or(Error::Limit)?;
        Ok(Self {
            relational: base.relational.clone(),
            base,
            transaction,
            events: 0,
            touched: BTreeSet::new(),
            candidates: BTreeMap::new(),
            candidate_pages: 0,
            aborted: false,
        })
    }

    fn ready(&self) -> Result<()> {
        if self.aborted {
            Err(Error::Aborted)
        } else {
            Ok(())
        }
    }

    pub fn transaction(&self) -> u64 {
        self.transaction
    }
    pub fn view(&self) -> Result<&Snapshot> {
        self.ready()?;
        Ok(&self.relational)
    }

    pub fn apply(&mut self, event: Event) -> Result<()> {
        self.ready()?;
        let result = (|| {
            if self.events >= MAX_EVENTS {
                return Err(Error::Limit);
            }
            if matches!(event.kind, EventKind::Root) {
                return Err(Error::Selection("initial root cannot be restaged"));
            }
            let table = event.table_id;
            self.relational.apply(event)?;
            self.touched.insert(table);
            self.events += 1;
            Ok(())
        })();
        if result.is_err() {
            self.aborted = true;
        }
        result
    }

    /// The full candidate is owned by this stage and cannot mutate the base.
    pub fn index(&mut self, binding: RootBinding, index: IndexSnapshot) -> Result<()> {
        self.ready()?;
        let result = (|| {
            let table = binding.address().table();
            if self.candidates.len() >= emilybase_database::MAX_TABLES {
                return Err(Error::Limit);
            }
            if self.candidates.contains_key(&table) {
                return Err(Error::Selection("duplicate staged index"));
            }
            let candidate_pages = self
                .candidate_pages
                .checked_add(index.tree.page_count())
                .filter(|pages| *pages <= MAX_SELECTED_INDEX_PAGES)
                .ok_or(Error::Limit)?;
            self.candidates
                .insert(table, Selection::new(binding, index)?);
            self.candidate_pages = candidate_pages;
            Ok(())
        })();
        if result.is_err() {
            self.aborted = true;
        }
        result
    }

    /// Rebuild an original primary tree and bind its exact predecessor without
    /// exposing a clonable relational snapshot to an admitted stage's caller.
    pub fn rebuild_index(&mut self, name: &str) -> Result<()> {
        self.ready()?;
        let result = (|| {
            let table = self.relational.table_id(name)?;
            let previous = self.base.selected.get(&table);
            let revision = previous
                .map_or(Some(1), |value| value.binding.revision().checked_add(1))
                .ok_or(Error::Limit)?;
            let predecessor = previous
                .map(|value| {
                    Predecessor::new(
                        value.binding.revision(),
                        value.binding.transaction(),
                        value.index_fingerprint,
                    )
                })
                .transpose()?;
            let schema = self.relational.schema(name)?;
            let key_type = match schema.columns.get(usize::from(schema.primary_key)) {
                Some(column) if column.data_type == DataType::Integer => IndexKeyType::Integer,
                Some(column) if column.data_type == DataType::Text => IndexKeyType::Text,
                _ => return Err(Error::Selection("primary key type")),
            };
            let tree = self.relational.export_primary_tree(name)?;
            let info = self.relational.verify_primary_tree(name, &tree)?;
            let binding = RootBinding::new(
                PageAddress::primary(self.base.database, table, tree.root_id())?,
                key_type,
                revision,
                self.transaction,
                info.entries as u64,
                info.excluded_long_keys as u64,
                info.pages as u32,
                predecessor,
            )?;
            self.index(binding, IndexSnapshot { revision, tree })
        })();
        if result.is_err() {
            self.aborted = true;
        }
        result
    }

    pub fn prepare(mut self) -> Result<Prepared> {
        self.ready()?;
        if self.events == 0 && self.candidates.is_empty() {
            return Err(Error::Empty);
        }
        let mut selected = BTreeMap::new();
        for schema in self.relational.schemas() {
            let table = self.relational.table_id(&schema.name)?;
            let previous = self.base.selected.get(&table);
            let candidate = self.candidates.remove(&table);
            let selection = match candidate {
                Some(candidate) => {
                    candidate
                        .binding
                        .verify_owner(self.base.database, table, self.transaction)?;
                    match previous {
                        Some(previous) => candidate
                            .binding
                            .verify_predecessor(previous.binding, previous.index_fingerprint)?,
                        None if candidate.binding.revision() == 1
                            && candidate.binding.predecessor().is_none() => {}
                        None => return Err(Error::Selection("new table has a predecessor")),
                    }
                    Arc::new(candidate)
                }
                None if !self.touched.contains(&table) => {
                    Arc::clone(previous.ok_or(Error::Selection("new table needs an index"))?)
                }
                None => return Err(Error::Selection("changed table needs an index")),
            };
            selected.insert(table, selection);
        }
        if !self.candidates.is_empty() {
            return Err(Error::Selection("extra staged index"));
        }
        let retired_tables = self
            .base
            .selected
            .keys()
            .filter(|id| !selected.contains_key(id))
            .copied()
            .collect();
        let next = State::validated(
            self.base.database,
            self.transaction,
            self.relational,
            selected,
        )?;
        Ok(Prepared {
            base: self.base.fingerprint,
            next,
            retired_tables,
            previous: self.base,
        })
    }
}

impl Prepared {
    /// Complete selected-state component lengths, excluding WAL and heap.
    pub fn encoded_components(&self) -> Result<EncodedComponents> {
        self.next.encoded_components()
    }

    pub fn view(&self) -> &Snapshot {
        &self.next.relational
    }
    pub fn transaction(&self) -> u64 {
        self.next.transaction
    }
    pub fn selection(&self, table: u64) -> Option<&Selection> {
        self.next.selected.get(&table).map(Arc::as_ref)
    }
    pub fn retired_tables(&self) -> &[u64] {
        &self.retired_tables
    }
}
