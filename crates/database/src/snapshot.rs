use std::sync::Arc;

use emilybase_catalog::{Key, Row, Schema};
use emilybase_storage::{Error as StorageError, MAX_PAGES, Page};

use crate::state::State;
use crate::{DATABASE_MARKER, Error, Event, MAX_ROWS, Result};

/// Validated in-memory relational history. Clones share immutable page images.
/// Mutating a snapshot alone provides no persistence or commit acknowledgment.
#[derive(Clone)]
pub struct Snapshot {
    state: State,
    pages: Vec<Arc<Page>>,
}

impl Snapshot {
    pub fn empty() -> Result<Self> {
        let mut root = Page::new(1)?;
        root.insert(&DATABASE_MARKER)?;
        Ok(Self {
            state: State::new(),
            pages: vec![Arc::new(root)],
        })
    }

    pub fn from_pages(pages: Vec<Page>) -> Result<Self> {
        if pages.is_empty() || pages.len() as u64 > MAX_PAGES {
            return Err(Error::NotTableFile);
        }
        let mut state = State::new();
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
                    state.apply(Event::decode(bytes)?)?;
                }
            }
        }
        Ok(Self {
            state,
            pages: pages.into_iter().map(Arc::new).collect(),
        })
    }

    /// Validate and stage a single event without filesystem operations.
    pub fn apply(&mut self, event: Event) -> Result<()> {
        self.state.validate(&event)?;
        let bytes = event.encode()?;
        let last = self.pages.last().ok_or(Error::NotTableFile)?;
        let mut page = last.as_ref().clone();
        let append = match page.insert(&bytes) {
            Ok(_) => false,
            Err(StorageError::PageFull) => {
                if self.pages.len() as u64 >= MAX_PAGES {
                    return Err(StorageError::PageLimit.into());
                }
                page = Page::new(self.pages.len() as u64 + 1)?;
                page.insert(&bytes)?;
                true
            }
            Err(error) => return Err(error.into()),
        };
        self.state.apply(event)?;
        if append {
            self.pages.push(Arc::new(page));
        } else {
            let last = self.pages.last_mut().ok_or(Error::NotTableFile)?;
            *last = Arc::new(page);
        }
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
        Ok(table.rows.get(key))
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
