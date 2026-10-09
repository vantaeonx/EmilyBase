//! Synchronous building blocks for the EmilyBase file format.
mod byte_file;
mod creation;
mod error;
pub mod header;
mod page;
mod pager;
#[cfg(all(test, target_os = "linux"))]
mod publication_tests;

pub use byte_file::{publish_private_file, publish_private_file_at};
pub use error::{Error, Result};
pub use page::{MAX_RECORD_SIZE, Page, SlotId};
pub use pager::{MAX_PAGES, Pager};

pub const PAGE_SIZE: usize = 4096;
pub const FORMAT_VERSION: u16 = 1;
