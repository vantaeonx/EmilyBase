//! Synchronous building blocks for the EmilyBase file format.
mod error;
pub mod header;
mod page;

pub use error::{Error, Result};
pub use page::{MAX_RECORD_SIZE, Page, SlotId};

pub const PAGE_SIZE: usize = 4096;
pub const FORMAT_VERSION: u16 = 1;
