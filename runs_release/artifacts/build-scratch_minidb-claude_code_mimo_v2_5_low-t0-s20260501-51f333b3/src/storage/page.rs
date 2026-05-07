pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageId(pub u64);

impl PageId {
    pub fn offset(&self) -> u64 {
        self.0 * PAGE_SIZE as u64
    }
}

impl std::fmt::Display for PageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PageId({})", self.0)
    }
}

#[derive(Debug, Clone)]
pub struct Page {
    pub id: PageId,
    pub data: [u8; PAGE_SIZE],
    pub dirty: bool,
}

impl Page {
    pub fn new(id: PageId) -> Self {
        Page {
            id,
            data: [0; PAGE_SIZE],
            dirty: false,
        }
    }

    pub fn read_u16(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.data[offset], self.data[offset + 1]])
    }

    pub fn write_u16(&mut self, offset: usize, val: u16) {
        let bytes = val.to_le_bytes();
        self.data[offset] = bytes[0];
        self.data[offset + 1] = bytes[1];
        self.dirty = true;
    }

    pub fn read_u32(&self, offset: usize) -> u32 {
        u32::from_le_bytes([
            self.data[offset],
            self.data[offset + 1],
            self.data[offset + 2],
            self.data[offset + 3],
        ])
    }

    pub fn write_u32(&mut self, offset: usize, val: u32) {
        let bytes = val.to_le_bytes();
        for i in 0..4 {
            self.data[offset + i] = bytes[i];
        }
        self.dirty = true;
    }

    pub fn read_u64(&self, offset: usize) -> u64 {
        u64::from_le_bytes([
            self.data[offset],
            self.data[offset + 1],
            self.data[offset + 2],
            self.data[offset + 3],
            self.data[offset + 4],
            self.data[offset + 5],
            self.data[offset + 6],
            self.data[offset + 7],
        ])
    }

    pub fn write_u64(&mut self, offset: usize, val: u64) {
        let bytes = val.to_le_bytes();
        for i in 0..8 {
            self.data[offset + i] = bytes[i];
        }
        self.dirty = true;
    }

    pub fn read_bytes(&self, offset: usize, len: usize) -> &[u8] {
        &self.data[offset..offset + len]
    }

    pub fn write_bytes(&mut self, offset: usize, bytes: &[u8]) {
        self.data[offset..offset + bytes.len()].copy_from_slice(bytes);
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_read_write() {
        let mut page = Page::new(PageId(0));
        page.write_u16(0, 42);
        assert_eq!(page.read_u16(0), 42);

        page.write_u32(10, 123456);
        assert_eq!(page.read_u32(10), 123456);

        page.write_u64(20, 9999999999);
        assert_eq!(page.read_u64(20), 9999999999);
    }

    #[test]
    fn test_page_bytes() {
        let mut page = Page::new(PageId(0));
        let data = b"hello world";
        page.write_bytes(100, data);
        assert_eq!(page.read_bytes(100, data.len()), data);
    }
}
