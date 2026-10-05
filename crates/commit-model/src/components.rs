use emilybase_commit_format::{MAX_HISTORY_PAGES, MAX_PRIMARY_PAGES, ROOT_BYTES};
use emilybase_index::PAGE_SIZE;

use crate::{Error, MAX_SELECTED_INDEX_PAGES, Result};

/// Exact lengths of existing standalone components, not a WAL or heap budget.
/// History counts page images only; physical file headers are excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodedComponents {
    history_pages: u64,
    roots: u64,
    index_pages: u64,
    history_bytes: u64,
    index_bytes: u64,
    root_bytes: u64,
    total_bytes: u64,
}

impl EncodedComponents {
    /// Counts are bounded before arithmetic. Index pages count live images,
    /// including one empty leaf per selected root, never sparse ID holes.
    pub fn from_counts(history_pages: u64, roots: u64, index_pages: u64) -> Result<Self> {
        if !(1..=MAX_HISTORY_PAGES).contains(&history_pages)
            || roots > emilybase_database::MAX_TABLES as u64
            || index_pages < roots
            || (roots == 0 && index_pages != 0)
            || index_pages > MAX_SELECTED_INDEX_PAGES as u64
        {
            return Err(Error::Limit);
        }
        let max_index_pages = roots.checked_mul(MAX_PRIMARY_PAGES).ok_or(Error::Limit)?;
        if index_pages > max_index_pages {
            return Err(Error::Limit);
        }
        let history_bytes = history_pages
            .checked_mul(PAGE_SIZE as u64)
            .ok_or(Error::Limit)?;
        let index_bytes = index_pages
            .checked_add(roots)
            .and_then(|pages| pages.checked_mul(PAGE_SIZE as u64))
            .ok_or(Error::Limit)?;
        let root_bytes = roots.checked_mul(ROOT_BYTES as u64).ok_or(Error::Limit)?;
        let total_bytes = history_bytes
            .checked_add(index_bytes)
            .and_then(|bytes| bytes.checked_add(root_bytes))
            .ok_or(Error::Limit)?;
        Ok(Self {
            history_pages,
            roots,
            index_pages,
            history_bytes,
            index_bytes,
            root_bytes,
            total_bytes,
        })
    }

    pub fn history_pages(self) -> u64 {
        self.history_pages
    }
    pub fn roots(self) -> u64 {
        self.roots
    }
    pub fn index_pages(self) -> u64 {
        self.index_pages
    }
    pub fn history_bytes(self) -> u64 {
        self.history_bytes
    }
    /// Includes one EBIF header per selected root.
    pub fn index_bytes(self) -> u64 {
        self.index_bytes
    }
    pub fn root_bytes(self) -> u64 {
        self.root_bytes
    }
    pub fn total_bytes(self) -> u64 {
        self.total_bytes
    }
}
