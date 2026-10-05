//! Experimental standalone namespace/root codecs. No WAL version selects these
//! records and no managed database writes them. CRC is integrity, not authorization.
mod address;
mod codec;
mod root;

pub use address::{ADDRESS_BYTES, Domain, PageAddress};
pub use root::{IndexKeyType, Predecessor, ROOT_BYTES, RootBinding};

pub type DatabaseId = [u8; 16];
pub const VERSION: u16 = 1;
pub const MAX_HISTORY_PAGES: u64 = 65_536;
pub const MAX_PRIMARY_PAGES: u64 = 1024;
pub const MAX_LIVE_KEYS: u64 = 10_000;
pub const MAX_TRANSACTION: u64 = u64::MAX - 1;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("invalid experimental record length")]
    Length,
    #[error("invalid experimental record magic")]
    Magic,
    #[error("unsupported experimental record version {0}")]
    Version(u16),
    #[error("experimental record checksum mismatch")]
    Checksum,
    #[error("unknown page domain {0}")]
    Domain(u8),
    #[error("unknown index key type {0}")]
    KeyType(u8),
    #[error("nonzero experimental reserved fields")]
    Reserved,
    #[error("invalid experimental metadata: {0}")]
    Invalid(&'static str),
    #[error("experimental root does not match its expected owner")]
    Identity,
    #[error("experimental root does not match its exact predecessor")]
    Predecessor,
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn database_id(id: DatabaseId) -> Result<()> {
    if id == [0; 16] {
        return Err(Error::Invalid("database identity"));
    }
    Ok(())
}

pub(crate) fn transaction(value: u64) -> Result<()> {
    if !(1..=MAX_TRANSACTION).contains(&value) {
        return Err(Error::Invalid("transaction"));
    }
    Ok(())
}
