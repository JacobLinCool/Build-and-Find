//! Storage engine: page manager, heap files, B-tree indexes, WAL.

pub mod btree;
pub mod heap;
pub mod page;
pub mod pager;
pub mod wal;

pub use page::{PageId, PAGE_SIZE};
