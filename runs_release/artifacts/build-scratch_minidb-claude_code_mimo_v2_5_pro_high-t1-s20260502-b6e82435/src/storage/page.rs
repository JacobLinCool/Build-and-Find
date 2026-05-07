/// Page size in bytes (4 KB)
pub const PAGE_SIZE: usize = 4096;

/// A page identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageId(pub u64);

impl PageId {
    pub const INVALID: PageId = PageId(u64::MAX);
}

impl std::fmt::Display for PageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Page({})", self.0)
    }
}

/// A fixed-size page of bytes.
#[derive(Clone)]
pub struct Page {
    pub id: PageId,
    pub data: [u8; PAGE_SIZE],
    pub dirty: bool,
}

impl Page {
    pub fn new(id: PageId) -> Self {
        Self {
            id,
            data: [0u8; PAGE_SIZE],
            dirty: false,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.dirty = true;
        &mut self.data
    }

    pub fn write_u64(&mut self, offset: usize, val: u64) {
        self.dirty = true;
        self.data[offset..offset + 8].copy_from_slice(&val.to_le_bytes());
    }

    pub fn read_u64(&self, offset: usize) -> u64 {
        u64::from_le_bytes(self.data[offset..offset + 8].try_into().unwrap())
    }

    pub fn write_u32(&mut self, offset: usize, val: u32) {
        self.dirty = true;
        self.data[offset..offset + 4].copy_from_slice(&val.to_le_bytes());
    }

    pub fn read_u32(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.data[offset..offset + 4].try_into().unwrap())
    }

    pub fn write_u16(&mut self, offset: usize, val: u16) {
        self.dirty = true;
        self.data[offset..offset + 2].copy_from_slice(&val.to_le_bytes());
    }

    pub fn read_u16(&self, offset: usize) -> u16 {
        u16::from_le_bytes(self.data[offset..offset + 2].try_into().unwrap())
    }

    pub fn write_u8(&mut self, offset: usize, val: u8) {
        self.dirty = true;
        self.data[offset] = val;
    }

    pub fn read_u8(&self, offset: usize) -> u8 {
        self.data[offset]
    }

    pub fn write_bytes(&mut self, offset: usize, bytes: &[u8]) {
        self.dirty = true;
        let end = offset + bytes.len();
        self.data[offset..end].copy_from_slice(bytes);
    }

    pub fn read_bytes(&self, offset: usize, len: usize) -> &[u8] {
        &self.data[offset..offset + len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_read_write() {
        let mut page = Page::new(PageId(0));
        page.write_u64(0, 42);
        assert_eq!(page.read_u64(0), 42);

        page.write_u32(8, 100);
        assert_eq!(page.read_u32(8), 100);

        page.write_u16(12, 200);
        assert_eq!(page.read_u16(12), 200);

        assert!(page.dirty);
    }

    #[test]
    fn test_page_size() {
        let page = Page::new(PageId(0));
        assert_eq!(page.data.len(), PAGE_SIZE);
    }
}
