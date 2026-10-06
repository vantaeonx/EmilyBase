//! Immutable physical component plan for a validated memory transaction.
//! There is no outer wire envelope, WAL append, fsync or durable acknowledgment.
use std::collections::BTreeSet;
use std::sync::Arc;

use emilybase_commit_format::{DatabaseId, Domain, PageAddress, RootBinding};
use emilybase_database::Snapshot;
use emilybase_index::{BPlusTree, IndexPage, IndexSnapshot, PAGE_SIZE, SnapshotDelta};
use emilybase_storage::Page;

use crate::state::State;
use crate::{Error, MAX_EVENTS, MAX_SELECTED_INDEX_PAGES, Model, Prepared, Result, Selection};

#[path = "plan_codec.rs"]
mod codec;
pub use codec::{IMAGE_PLAN_HEADER_BYTES, IMAGE_PLAN_MAX_BYTES, IMAGE_PLAN_VERSION};
#[path = "envelopes.rs"]
mod envelopes;
pub use envelopes::{
    AdmittedEnvelope, EnvelopeLimit, EnvelopeLimits, EnvelopePool, EnvelopeUsage,
    MAX_ENVELOPE_BUFFERS, MAX_ENVELOPE_BYTES,
};
#[path = "decoded_plans.rs"]
mod decoded;
pub use decoded::{
    AdmittedPlan, DecodedPlanLimit, DecodedPlanLimits, DecodedPlanPool, DecodedPlanUsage,
    MAX_DECODED_PLAN_BYTES, MAX_DECODED_PLAN_VECTOR_BYTES, MAX_DECODED_PLANS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanCounts {
    history: usize,
    primary: usize,
    retired_pages: usize,
    changed_roots: usize,
    retired_tables: usize,
    image_bytes: u64,
}

impl PlanCounts {
    /// Bounds arithmetic only. Row coverage, topology and history admission are
    /// additionally required by ImagePlan::replay. Bytes exclude all framing.
    pub fn from_counts(
        history: usize,
        primary: usize,
        retired_pages: usize,
        changed_roots: usize,
        retired_tables: usize,
    ) -> Result<Self> {
        if history > MAX_EVENTS
            || primary > MAX_SELECTED_INDEX_PAGES
            || retired_pages > MAX_SELECTED_INDEX_PAGES
            || changed_roots > emilybase_database::MAX_TABLES
            || retired_tables > emilybase_database::MAX_TABLES
            || (changed_roots == 0 && (primary != 0 || retired_pages != 0))
        {
            return Err(Error::Limit);
        }
        let image_bytes = (history as u64)
            .checked_add(primary as u64)
            .and_then(|count| count.checked_mul(PAGE_SIZE as u64))
            .ok_or(Error::Limit)?;
        Ok(Self {
            history,
            primary,
            retired_pages,
            changed_roots,
            retired_tables,
            image_bytes,
        })
    }
    pub fn history_pages(self) -> usize {
        self.history
    }
    pub fn primary_pages(self) -> usize {
        self.primary
    }
    pub fn retired_pages(self) -> usize {
        self.retired_pages
    }
    pub fn changed_roots(self) -> usize {
        self.changed_roots
    }
    pub fn retired_tables(self) -> usize {
        self.retired_tables
    }
    /// Original 4096-byte EBPG/EBIX bodies only, not a journal/heap budget.
    pub fn image_body_bytes(self) -> u64 {
        self.image_bytes
    }
}

pub struct PageWrite {
    address: PageAddress,
    image: [u8; PAGE_SIZE],
}
impl PageWrite {
    pub fn address(&self) -> PageAddress {
        self.address
    }
    pub fn image(&self) -> &[u8; PAGE_SIZE] {
        &self.image
    }
}

pub struct RootChange {
    binding: RootBinding,
    upserts: Vec<PageWrite>,
    retired: Vec<PageAddress>,
}
impl RootChange {
    pub fn binding(&self) -> RootBinding {
        self.binding
    }
    pub fn upserts(&self) -> &[PageWrite] {
        &self.upserts
    }
    pub fn retired(&self) -> &[PageAddress] {
        &self.retired
    }
}

pub struct RetiredRoot {
    binding: RootBinding,
    fingerprint: [u8; 32],
}
impl RetiredRoot {
    pub fn binding(&self) -> RootBinding {
        self.binding
    }
    pub fn index_fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }
}

/// Owned component images; not an admitted ModelPool generation. Output copies
/// and replay temporaries need their own future byte/transient reservation.
pub struct ImagePlan {
    database: DatabaseId,
    base_transaction: u64,
    transaction: u64,
    base: [u8; 32],
    next: [u8; 32],
    history: Vec<PageWrite>,
    roots: Vec<RootChange>,
    retired: Vec<RetiredRoot>,
}

impl Prepared {
    /// Materialize only changed last/appended history pages and changed index
    /// images. Unchanged roots retain their old selection and are omitted.
    pub fn image_plan(&self) -> Result<ImagePlan> {
        let database = self.next.database;
        let previous = &self.previous;
        let old_count = previous.relational.page_count();
        let old_last = previous
            .relational
            .pages()
            .last()
            .ok_or(Error::Plan("empty base history"))?
            .encode();
        let mut history = Vec::new();
        for page in self.next.relational.pages().skip(old_count - 1) {
            let image = page.encode();
            if page.id() as usize == old_count && image == old_last {
                continue;
            }
            if history.len() == MAX_EVENTS {
                return Err(Error::Limit);
            }
            history.push(PageWrite {
                address: PageAddress::history(database, page.id())?,
                image,
            });
        }
        let mut roots = Vec::new();
        for (table, selected) in &self.next.selected {
            let base = previous.selected.get(table);
            if base.is_some_and(|base| Arc::ptr_eq(base, selected)) {
                continue;
            }
            let (images, retired) = match base {
                Some(base) => {
                    let delta = base.index.delta_to(&selected.index.tree)?;
                    if delta.revision != selected.index.revision {
                        return Err(Error::Plan("nonadjacent index revision"));
                    }
                    (delta.upserts, delta.retired)
                }
                None => (selected.index.tree.page_images()?, Vec::new()),
            };
            let upserts = images
                .into_iter()
                .map(|image| {
                    let id = u64::from_le_bytes(
                        image[8..16]
                            .try_into()
                            .map_err(|_| Error::Plan("page ID width"))?,
                    );
                    Ok(PageWrite {
                        address: PageAddress::primary(database, *table, id)?,
                        image,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let retired = retired
                .into_iter()
                .map(|id| Ok(PageAddress::primary(database, *table, id)?))
                .collect::<Result<Vec<_>>>()?;
            roots.push(RootChange {
                binding: selected.binding,
                upserts,
                retired,
            });
        }
        let retired = previous
            .selected
            .iter()
            .filter(|(table, _)| !self.next.selected.contains_key(table))
            .map(|(_, selected)| RetiredRoot {
                binding: selected.binding,
                fingerprint: selected.index_fingerprint,
            })
            .collect();
        let plan = ImagePlan {
            database,
            base_transaction: previous.transaction,
            transaction: self.next.transaction,
            base: self.base,
            next: self.next.fingerprint,
            history,
            roots,
            retired,
        };
        plan.counts()?;
        Ok(plan)
    }
}

impl ImagePlan {
    pub fn database_id(&self) -> DatabaseId {
        self.database
    }
    pub fn base_transaction(&self) -> u64 {
        self.base_transaction
    }
    pub fn transaction(&self) -> u64 {
        self.transaction
    }
    pub fn base_fingerprint(&self) -> [u8; 32] {
        self.base
    }
    pub fn next_fingerprint(&self) -> [u8; 32] {
        self.next
    }
    pub fn history(&self) -> &[PageWrite] {
        &self.history
    }
    pub fn roots(&self) -> &[RootChange] {
        &self.roots
    }
    pub fn retired_tables(&self) -> &[RetiredRoot] {
        &self.retired
    }

    pub fn counts(&self) -> Result<PlanCounts> {
        if self.roots.len() > emilybase_database::MAX_TABLES
            || self.retired.len() > emilybase_database::MAX_TABLES
        {
            return Err(Error::Limit);
        }
        let primary = self.roots.iter().try_fold(0usize, |count, root| {
            count.checked_add(root.upserts.len()).ok_or(Error::Limit)
        })?;
        let retired_pages = self.roots.iter().try_fold(0usize, |count, root| {
            count.checked_add(root.retired.len()).ok_or(Error::Limit)
        })?;
        PlanCounts::from_counts(
            self.history.len(),
            primary,
            retired_pages,
            self.roots.len(),
            self.retired.len(),
        )
    }

    /// Reconstruct both projections independently from physical components. The
    /// supplied base is immutable and never published; this writes no files.
    pub fn replay(&self, base: &Model) -> Result<Model> {
        self.counts()?;
        if base.database_id() != self.database
            || base.transaction() != self.base_transaction
            || base.fingerprint() != self.base
            || self.base_transaction.checked_add(1) != Some(self.transaction)
            || self.transaction > emilybase_commit_format::MAX_TRANSACTION
        {
            return Err(Error::Conflict);
        }
        let relational = self.replay_history(&base.state.relational)?;
        let mut selected = base.state.selected.clone();
        let mut retired_tables = BTreeSet::new();
        let mut previous_table = 0;
        for retired in &self.retired {
            let table = retired.binding.address().table();
            if table <= previous_table {
                return Err(Error::Plan("retired table order"));
            }
            previous_table = table;
            retired_tables.insert(table);
            let previous = selected
                .remove(&table)
                .ok_or(Error::Plan("unknown retired table"))?;
            if previous.binding != retired.binding
                || previous.index_fingerprint != retired.fingerprint
            {
                return Err(Error::Plan("retired root predecessor"));
            }
        }
        previous_table = 0;
        for change in &self.roots {
            let table = change.binding.address().table();
            if table <= previous_table || retired_tables.contains(&table) {
                return Err(Error::Plan("changed root order/scope"));
            }
            previous_table = table;
            change
                .binding
                .verify_owner(self.database, table, self.transaction)?;
            let images = change.checked_images(self.database, table)?;
            let retired = change.checked_retirements(self.database, table)?;
            let index = match selected.get(&table) {
                Some(previous) => {
                    change
                        .binding
                        .verify_predecessor(previous.binding, previous.index_fingerprint)?;
                    SnapshotDelta {
                        base_revision: previous.index.revision,
                        base_fingerprint: previous.index_fingerprint,
                        revision: change.binding.revision(),
                        root: change.binding.address().page(),
                        entries: change.binding.covered() as usize,
                        upserts: images,
                        retired,
                    }
                    .apply(&previous.index)?
                }
                None => {
                    if change.binding.revision() != 1
                        || change.binding.predecessor().is_some()
                        || !retired.is_empty()
                    {
                        return Err(Error::Plan("new root predecessor"));
                    }
                    IndexSnapshot {
                        revision: 1,
                        tree: BPlusTree::from_stable_pages(
                            change.binding.address().page(),
                            &images,
                        )?,
                    }
                }
            };
            selected.insert(table, Arc::new(Selection::new(change.binding, index)?));
        }
        let state = State::validated(self.database, self.transaction, relational, selected)?;
        if state.fingerprint != self.next {
            return Err(Error::Plan("next state fingerprint"));
        }
        Ok(Model { state })
    }

    fn replay_history(&self, base: &Snapshot) -> Result<Snapshot> {
        if self.history.is_empty() {
            return Ok(base.clone());
        }
        let mut pages = Vec::with_capacity(self.history.len());
        let mut previous_id = 0;
        for write in &self.history {
            let address = write.address;
            if address.database() != self.database
                || address.domain() != Domain::RelationalHistory
                || address.table() != 0
                || address.page() <= previous_id
            {
                return Err(Error::Plan("history address/order"));
            }
            previous_id = address.page();
            let page = Page::decode(&write.image, address.page())
                .map_err(emilybase_database::Error::from)?;
            pages.push(page);
        }
        base.replay_append_pages(&pages)
            .map_err(|error| match error {
                emilybase_database::Error::Event("committed history rewrite") => {
                    Error::Plan("committed history rewrite")
                }
                emilybase_database::Error::Event("history append starts outside tail")
                | emilybase_database::Error::Event("noncontiguous or deleted append slots") => {
                    Error::Plan("history page gap or earlier rewrite")
                }
                emilybase_database::Error::Event("history append does not extend tail") => {
                    Error::Plan("history does not append records")
                }
                other => other.into(),
            })
    }
}

impl RootChange {
    fn checked_images(&self, database: DatabaseId, table: u64) -> Result<Vec<[u8; PAGE_SIZE]>> {
        let mut previous_id = 0;
        let mut images = Vec::new();
        for write in &self.upserts {
            let address = write.address;
            if address.database() != database
                || address.domain() != Domain::PrimaryIndex
                || address.table() != table
                || address.page() <= previous_id
            {
                return Err(Error::Plan("index address/order"));
            }
            previous_id = address.page();
            IndexPage::decode(&write.image, address.page())?;
            images.push(write.image);
        }
        Ok(images)
    }
    fn checked_retirements(&self, database: DatabaseId, table: u64) -> Result<Vec<u64>> {
        let mut previous_id = 0;
        let mut ids = Vec::new();
        for address in &self.retired {
            if address.database() != database
                || address.domain() != Domain::PrimaryIndex
                || address.table() != table
                || address.page() <= previous_id
            {
                return Err(Error::Plan("index retirement address/order"));
            }
            previous_id = address.page();
            ids.push(address.page());
        }
        Ok(ids)
    }
}

#[cfg(test)]
#[path = "images_tests.rs"]
mod tests;
