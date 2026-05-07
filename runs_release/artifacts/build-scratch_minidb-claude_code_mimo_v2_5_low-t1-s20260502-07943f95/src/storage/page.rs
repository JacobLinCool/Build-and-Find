use super::PAGE_SIZE;
use crate::error::{MiniDbError, Result};

#[derive(Debug, Clone)]
pub struct Page {
    pub id: u64,
    pub data: Vec<u8>,
    pub page_type: PageType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageType {
    Heap,
    BTreeLeaf,
    BTreeInternal,
    Free,
}

impl Page {
    pub fn new(id: u64, page_type: PageType) -> Self {
        let mut data = vec![0u8; PAGE_SIZE];
        data[0] = match page_type {
            PageType::Heap => 0x01,
            PageType::BTreeLeaf => 0x02,
            PageType::BTreeInternal => 0x03,
            PageType::Free => 0x00,
        };
        Self {
            id,
            data,
            page_type,
        }
    }

    pub fn from_bytes(id: u64, data: Vec<u8>) -> Self {
        let page_type = match data.first() {
            Some(0x01) => PageType::Heap,
            Some(0x02) => PageType::BTreeLeaf,
            Some(0x03) => PageType::BTreeInternal,
            _ => PageType::Free,
        };
        Self {
            id,
            data,
            page_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_creation() {
        let page = Page::new(0, PageType::Heap);
        assert_eq!(page.data[0], 0x01);
        assert_eq!(page.data.len(), PAGE_SIZE);
    }
}
