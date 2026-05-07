/// Page size: 4096 bytes
pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageId(pub u64);

impl PageId {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// A raw page of PAGE_SIZE bytes
#[derive(Clone)]
pub struct Page {
    pub id: PageId,
    pub data: Box<[u8; PAGE_SIZE]>,
    pub dirty: bool,
}

impl Page {
    pub fn new(id: PageId) -> Self {
        Page {
            id,
            data: Box::new([0u8; PAGE_SIZE]),
            dirty: false,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

impl std::fmt::Debug for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Page(id={}, dirty={})", self.id.0, self.dirty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_size() {
        let p = Page::new(PageId(0));
        assert_eq!(p.data.len(), PAGE_SIZE);
        assert_eq!(PAGE_SIZE, 4096);
    }

    #[test]
    fn test_page_id_ordering() {
        assert!(PageId(0) < PageId(1));
        assert_eq!(PageId(5).as_u64(), 5);
    }
}
