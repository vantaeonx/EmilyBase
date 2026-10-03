use std::sync::{Arc, OnceLock};

use emilybase_catalog::{Key, Row, Schema};
use emilybase_storage::{Error as StorageError, MAX_PAGES, Page};

use crate::location::{Change, Locations};
use crate::primary::{PrimaryIndexes, eligible};
use crate::state::State;
use crate::{DATABASE_MARKER, Error, Event, MAX_ROWS, Result};
use crate::{EventKind, PrimaryIndexInfo, RowLocation};

/// Validated in-memory relational history. Clones share immutable page images.
/// Mutating a snapshot alone provides no persistence or commit acknowledgment.
#[derive(Clone)]
pub struct Snapshot {
    pub(crate) state: State,
    pages: Vec<Arc<Page>>,
    pub(crate) locations: Locations,
    pub(crate) primary_indexes: PrimaryIndexes,
    pub(crate) page_digest: Arc<OnceLock<[u8; 32]>>,
}

impl Snapshot {
    pub fn empty() -> Result<Self> {
        let mut root = Page::new(1)?;
        root.insert(&DATABASE_MARKER)?;
        Ok(Self {
            state: State::new(),
            pages: vec![Arc::new(root)],
            locations: Locations::default(),
            primary_indexes: PrimaryIndexes::default(),
            page_digest: Arc::new(OnceLock::new()),
        })
    }

    pub fn from_pages(pages: Vec<Page>) -> Result<Self> {
        if pages.is_empty() || pages.len() as u64 > MAX_PAGES {
            return Err(Error::NotTableFile);
        }
        let mut state = State::new();
        let mut locations = Locations::default();
        for (index, page) in pages.iter().enumerate() {
            if page.id() != index as u64 + 1 {
                return Err(Error::Event("nonsequential snapshot page IDs"));
            }
            if page.record_count() == 0 || page.record_count() != page.slot_count() {
                return Err(Error::Event("empty or deleted history slots"));
            }
            for slot in 0..page.slot_count() {
                let bytes = page.get(slot as u16)?;
                if index == 0 && slot == 0 {
                    if bytes != DATABASE_MARKER {
                        return Err(Error::NotTableFile);
                    }
                } else {
                    let event = Event::decode(bytes)?;
                    state.validate(&event)?;
                    let location = RowLocation::from_record(&event, page.id(), slot as u16, bytes);
                    let change = Change::prepare(&state, &event, location)?;
                    state.apply(event)?;
                    locations.apply(change);
                }
            }
        }
        let primary_indexes = PrimaryIndexes::from_tables(state.tables.keys().copied());
        Ok(Self {
            state,
            pages: pages.into_iter().map(Arc::new).collect(),
            locations,
            primary_indexes,
            page_digest: Arc::new(OnceLock::new()),
        })
    }

    /// Validate and stage a single event without filesystem operations.
    pub fn apply(&mut self, event: Event) -> Result<()> {
        self.state.validate(&event)?;
        let bytes = event.encode()?;
        let last = self.pages.last().ok_or(Error::NotTableFile)?;
        let mut page = last.as_ref().clone();
        let (append, slot) = match page.insert(&bytes) {
            Ok(slot) => (false, slot),
            Err(StorageError::PageFull) => {
                if self.pages.len() as u64 >= MAX_PAGES {
                    return Err(StorageError::PageLimit.into());
                }
                page = Page::new(self.pages.len() as u64 + 1)?;
                (true, page.insert(&bytes)?)
            }
            Err(error) => return Err(error.into()),
        };
        let location = RowLocation::from_record(&event, page.id(), slot, &bytes);
        let change = Change::prepare(&self.state, &event, location)?;
        let index_change = self
            .primary_indexes
            .prepare(&event, &change, &self.locations)?;
        self.state.apply(event)?;
        if append {
            self.pages.push(Arc::new(page));
        } else {
            let last = self.pages.last_mut().ok_or(Error::NotTableFile)?;
            *last = Arc::new(page);
        }
        self.locations.apply(change);
        self.primary_indexes.apply(index_change);
        self.page_digest = Arc::new(OnceLock::new());
        Ok(())
    }

    pub fn next_table_id(&self) -> u64 {
        self.state.next_id
    }

    pub fn table_id(&self, name: &str) -> Result<u64> {
        self.state.table_id(name)
    }

    pub fn schema(&self, name: &str) -> Result<&Schema> {
        Ok(&self.state.table(name)?.schema)
    }

    pub fn schemas(&self) -> Vec<Schema> {
        self.state
            .tables
            .values()
            .map(|table| table.schema.clone())
            .collect()
    }

    pub fn get(&self, name: &str, key: &Key) -> Result<Option<&Row>> {
        let table = self.state.table(name)?;
        table.schema.validate_key(key)?;
        if !eligible(key) {
            return Ok(table.rows.get(key));
        }
        let table_id = self.table_id(name)?;
        let tree = self
            .primary_indexes
            .tree(table_id, &table.rows, &self.locations)?;
        let Some(pointer) = tree
            .get(key)
            .map_err(|_| Error::PrimaryIndex("point lookup failed"))?
        else {
            if table.rows.contains_key(key) {
                return Err(Error::PrimaryIndex("missing live key"));
            }
            return Ok(None);
        };
        let location = self
            .locations
            .get(table_id, key)
            .ok_or(Error::PrimaryIndex("unknown indexed key"))?;
        if pointer.page_id != location.page_id || pointer.slot_id != location.slot_id {
            return Err(Error::PrimaryIndex("obsolete row pointer"));
        }
        Ok(Some(self.resolve_row_location(name, key, location)?))
    }

    /// Build/read the bounded derived point-lookup index without writing files.
    pub fn primary_index_info(&self, name: &str) -> Result<PrimaryIndexInfo> {
        let table = self.state.table(name)?;
        self.primary_indexes
            .info(self.table_id(name)?, &table.rows, &self.locations)
    }

    /// Return the last live insert/replace image, never an obsolete historical row.
    pub fn row_location(&self, name: &str, key: &Key) -> Result<Option<RowLocation>> {
        let table = self.state.table(name)?;
        table.schema.validate_key(key)?;
        Ok(self.locations.get(self.table_id(name)?, key))
    }

    /// Resolve only a current matching table/key/position/image in this snapshot.
    /// Locations are scoped to the caller-selected database; bind its ID separately.
    pub fn resolve_row_location(
        &self,
        name: &str,
        key: &Key,
        location: RowLocation,
    ) -> Result<&Row> {
        if self.row_location(name, key)? != Some(location) {
            return Err(Error::StaleLocation);
        }
        let index = location
            .page_id
            .checked_sub(1)
            .and_then(|id| usize::try_from(id).ok())
            .ok_or(Error::StaleLocation)?;
        let page = self.pages.get(index).ok_or(Error::StaleLocation)?;
        let bytes = page
            .get(location.slot_id)
            .map_err(|_| Error::StaleLocation)?;
        if !location.matches_record(bytes) {
            return Err(Error::StaleLocation);
        }
        let event = Event::decode(bytes)?;
        let row = match event.kind {
            EventKind::Insert(row) | EventKind::Replace(row)
                if event.table_id == location.table_id =>
            {
                row
            }
            _ => return Err(Error::StaleLocation),
        };
        let table = self.state.table(name)?;
        if table.schema.key(&row)? != *key {
            return Err(Error::StaleLocation);
        }
        let current = table.rows.get(key).ok_or(Error::StaleLocation)?;
        if current != &row {
            return Err(Error::StaleLocation);
        }
        Ok(current)
    }

    pub fn scan(&self, name: &str, limit: usize) -> Result<Vec<Row>> {
        if limit > MAX_ROWS {
            return Err(Error::Limit("scan rows"));
        }
        Ok(self
            .state
            .table(name)?
            .rows
            .values()
            .take(limit)
            .cloned()
            .collect())
    }

    pub fn row_count(&self) -> usize {
        self.state.row_count
    }

    pub fn event_count(&self) -> usize {
        self.state.event_count
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn pages(&self) -> impl Iterator<Item = &Page> {
        self.pages.iter().map(AsRef::as_ref)
    }
}
