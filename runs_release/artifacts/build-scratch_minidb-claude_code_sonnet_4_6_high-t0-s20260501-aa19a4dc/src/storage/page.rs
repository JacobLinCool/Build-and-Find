/// Page size: 4096 bytes
pub const PAGE_SIZE: usize = 4096;

/// A typed page identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageId(pub u64);

impl PageId {
    pub fn new(id: u64) -> Self {
        PageId(id)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for PageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Page({})", self.0)
    }
}

/// A page is a fixed-size byte array
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

    pub fn from_bytes(id: PageId, bytes: &[u8]) -> Self {
        let mut data = Box::new([0u8; PAGE_SIZE]);
        let len = bytes.len().min(PAGE_SIZE);
        data[..len].copy_from_slice(&bytes[..len]);
        Page {
            id,
            data,
            dirty: false,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn as_slice(&self) -> &[u8] {
        self.data.as_ref()
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.data.as_mut()
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
    fn test_page_roundtrip() {
        let mut p = Page::new(PageId(1));
        p.data[0] = 42;
        p.data[4095] = 99;
        assert_eq!(p.data[0], 42);
        assert_eq!(p.data[4095], 99);
    }
}
