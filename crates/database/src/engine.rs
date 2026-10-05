use std::path::Path;

use emilybase_catalog::{Key, Row, Schema};
use emilybase_storage::{Error as StorageError, Page, Pager};

use crate::state::State;
use crate::{DATABASE_MARKER, Error, Event, EventKind, MAX_ROWS, Result};

/// Bounded single-owner table engine. Changes are page events, not transactions.
pub struct Database {
    pager: Pager,
    state: State,
    last_page: Page,
    poisoned: bool,
}

impl Database {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let mut page = Page::new(1)?;
        page.insert(&DATABASE_MARKER)?;
        let pager = Pager::create_with_pages(path, std::slice::from_ref(&page))?;
        Ok(Self {
            pager,
            state: State::new(),
            last_page: page,
            poisoned: false,
        })
    }

    /// Replay strictly validated history without changing any input bytes.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut pager = Pager::open(path)?;
        if pager.page_count() == 0 {
            return Err(Error::NotTableFile);
        }
        let mut state = State::new();
        let mut last_page = None;
        for id in 1..=pager.page_count() {
            let page = pager.read_page(id)?;
            if page.record_count() == 0 || page.record_count() != page.slot_count() {
                return Err(Error::Event("empty or deleted history slots"));
            }
            for slot in 0..page.slot_count() {
                let bytes = page.get(slot as u16)?;
                if id == 1 && slot == 0 {
                    if bytes != DATABASE_MARKER {
                        return Err(Error::NotTableFile);
                    }
                } else {
                    state.apply(Event::decode(bytes)?)?;
                }
            }
            last_page = Some(page);
        }
        Ok(Self {
            pager,
            state,
            last_page: last_page.ok_or(Error::NotTableFile)?,
            poisoned: false,
        })
    }

    pub fn create_table(&mut self, schema: Schema) -> Result<u64> {
        self.ready()?;
        let table_id = self.state.next_id;
        self.write_event(Event {
            table_id,
            kind: EventKind::Create(schema),
        })?;
        Ok(table_id)
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        self.ready()?;
        let table_id = self.state.table_id(name)?;
        self.write_event(Event {
            table_id,
            kind: EventKind::Drop,
        })
    }

    pub fn schemas(&self) -> Result<Vec<Schema>> {
        self.ready()?;
        Ok(self
            .state
            .tables
            .values()
            .map(|table| table.schema.clone())
            .collect())
    }

    pub fn insert(&mut self, name: &str, row: Row) -> Result<Key> {
        self.ready()?;
        let key = self.state.table(name)?.schema.key(&row)?;
        let table_id = self.state.table_id(name)?;
        self.write_event(Event {
            table_id,
            kind: EventKind::Insert(row),
        })?;
        Ok(key)
    }

    pub fn get(&self, name: &str, key: &Key) -> Result<Option<&Row>> {
        self.ready()?;
        let table = self.state.table(name)?;
        table.schema.validate_key(key)?;
        Ok(table.rows.get(key).map(std::sync::Arc::as_ref))
    }

    /// Replace all values while retaining the original primary key.
    pub fn update(&mut self, name: &str, key: &Key, row: Row) -> Result<()> {
        self.ready()?;
        let table = self.state.table(name)?;
        table.schema.validate_key(key)?;
        if table.schema.key(&row)? != *key {
            return Err(Error::PrimaryKeyChange);
        }
        let table_id = self.state.table_id(name)?;
        self.write_event(Event {
            table_id,
            kind: EventKind::Replace(row),
        })
    }

    pub fn delete(&mut self, name: &str, key: &Key) -> Result<()> {
        self.ready()?;
        let table_id = self.state.table_id(name)?;
        self.write_event(Event {
            table_id,
            kind: EventKind::Delete(key.clone()),
        })
    }

    /// Primary-key order only; SQL filtering and an on-disk B+ tree are future work.
    pub fn scan(&self, name: &str, limit: usize) -> Result<Vec<Row>> {
        self.ready()?;
        if limit > MAX_ROWS {
            return Err(Error::Limit("scan rows"));
        }
        Ok(self
            .state
            .table(name)?
            .rows
            .values()
            .take(limit)
            .map(std::sync::Arc::as_ref)
            .cloned()
            .collect())
    }

    pub fn row_count(&self) -> usize {
        self.state.row_count
    }

    pub fn event_count(&self) -> usize {
        self.state.event_count
    }

    pub fn page_count(&self) -> u64 {
        self.pager.page_count()
    }

    fn ready(&self) -> Result<()> {
        if self.poisoned {
            Err(Error::Poisoned)
        } else {
            Ok(())
        }
    }

    fn write_event(&mut self, event: Event) -> Result<()> {
        self.state.validate(&event)?;
        let bytes = event.encode()?;
        let mut page = self.last_page.clone();
        match page.insert(&bytes) {
            Ok(_) => (),
            Err(StorageError::PageFull) => {
                page = Page::new(self.pager.page_count() + 1)?;
                page.insert(&bytes)?;
            }
            Err(error) => return Err(error.into()),
        }
        if let Err(error) = self.pager.write_page(&page) {
            self.poisoned = true;
            return Err(error.into());
        }
        // Exclusive ownership leaves state unchanged between validation and apply.
        self.state.apply(event)?;
        self.last_page = page;
        Ok(())
    }
}
