//! Storage subsystem: pages, slotted heaps, B-tree indexes, and the WAL.
//!
//! Higher layers (catalog, executor) interact with storage through this
//! module's public API. Storage knows nothing about SQL; it manipulates
//! pages, rows, and ordered key sets.

pub mod btree;
pub mod codec;
pub mod heap;
pub mod page;
pub mod pager;
pub mod wal;

pub use page::{PageId, RowId, PAGE_SIZE};
pub use pager::Pager;
