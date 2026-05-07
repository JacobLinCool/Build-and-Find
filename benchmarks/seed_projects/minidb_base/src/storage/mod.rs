//! Storage subsystem: pages, heap files, secondary indexes, and the WAL.
//!
//! These modules are the lowest layer of minidb. Higher-level code
//! (catalog, planner, executor) reaches in to read and mutate tables.

pub mod btree;
pub mod heap;
pub mod page;
pub mod wal;

pub use btree::{BTreeIndex, Index, IndexKey};
pub use heap::{Heap, RowId};
pub use page::{Page, PageId, PAGE_SIZE};
pub use wal::{Wal, WalRecord, WalRecordKind};
