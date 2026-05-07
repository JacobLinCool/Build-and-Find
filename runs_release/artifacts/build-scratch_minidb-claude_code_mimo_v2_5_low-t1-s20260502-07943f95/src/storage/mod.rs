pub mod btree;
pub mod heap;
pub mod page;
pub mod wal;

use crate::error::Result;

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowId(pub u64);

pub trait PageManager {
    fn allocate_page(&mut self) -> Result<PageId>;
    fn read_page(&self, id: PageId) -> Result<page::Page>;
    fn write_page(&mut self, id: PageId, page: &page::Page) -> Result<()>;
    fn flush(&mut self) -> Result<()>;
}
