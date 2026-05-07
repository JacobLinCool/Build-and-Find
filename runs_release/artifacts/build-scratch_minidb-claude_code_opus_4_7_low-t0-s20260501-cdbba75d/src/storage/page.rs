use serde::{Deserialize, Serialize};

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageId(pub u64);

impl PageId {
    pub fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct Page {
    pub id: PageId,
    pub data: Vec<u8>, // length == PAGE_SIZE
}

impl Page {
    pub fn empty(id: PageId) -> Self {
        Page {
            id,
            data: vec![0u8; PAGE_SIZE],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_size_is_4096() {
        assert_eq!(PAGE_SIZE, 4096);
        let p = Page::empty(PageId(0));
        assert_eq!(p.data.len(), PAGE_SIZE);
    }
}
