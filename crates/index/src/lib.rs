//! Original bounded B+ tree and standalone storage. Managed snapshots derive point caches;
//! independently durable table-index pages do not yet participate in WAL.
mod bulk;
mod cursor;
#[cfg(test)]
mod cursor_tests;
mod delta;
mod mutations;
mod page;
mod snapshot;
mod store;
#[cfg(test)]
mod store_tests;
mod tree;

pub use cursor::RangeCursor;
pub use delta::SnapshotDelta;
pub use emilybase_catalog::Key;
pub use emilybase_storage::PAGE_SIZE;
pub use page::{IndexPage, RecordPointer};
pub use snapshot::{IndexSnapshot, MAX_SNAPSHOT_BYTES, SNAPSHOT_VERSION};
pub use store::{IndexStore, StoreError};
pub use tree::BPlusTree;

pub const INDEX_VERSION: u16 = 1;
pub const MAX_KEYS: usize = 14;
pub const MIN_KEYS: usize = MAX_KEYS / 2;
pub const MAX_KEY_BYTES: usize = 256;
pub const MAX_INDEX_PAGES: usize = 1024;
pub const MAX_INDEX_ENTRIES: usize = 10_000;
pub const MAX_TREE_HEIGHT: usize = 8;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("unsupported index version {0}")]
    Version(u16),
    #[error("invalid index magic")]
    Magic,
    #[error("invalid index page identity")]
    PageId,
    #[error("index page checksum mismatch")]
    Checksum,
    #[error("invalid index layout: {0}")]
    Layout(&'static str),
    #[error("index text key exceeds 256 UTF-8 bytes")]
    KeySize,
    #[error("duplicate index key")]
    Duplicate,
    #[error("index key does not exist")]
    NoKey,
    #[error("index capacity exceeded")]
    Limit,
    #[error("index output allocation refused")]
    Allocation,
}

pub(crate) fn validate_key(key: &Key) -> Result<()> {
    if matches!(key, Key::Text(text) if text.len() > MAX_KEY_BYTES) {
        return Err(Error::KeySize);
    }
    Ok(())
}

#[cfg(test)]
mod snapshot_tests;

#[cfg(test)]
mod shared_pages_tests;

#[cfg(test)]
mod stable_view_tests;
