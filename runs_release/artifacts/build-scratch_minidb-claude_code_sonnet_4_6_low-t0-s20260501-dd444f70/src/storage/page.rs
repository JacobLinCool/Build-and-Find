pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageId(pub u64);

impl PageId {
    pub fn as_file_offset(&self) -> u64 {
        self.0 * PAGE_SIZE as u64
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageType {
    Free = 0,
    Catalog = 1,
    Heap = 2,
    BTreeInternal = 3,
    BTreeLeaf = 4,
    Overflow = 5,
}

impl PageType {
    pub fn from_u8(v: u8) -> Option<PageType> {
        match v {
            0 => Some(PageType::Free),
            1 => Some(PageType::Catalog),
            2 => Some(PageType::Heap),
            3 => Some(PageType::BTreeInternal),
            4 => Some(PageType::BTreeLeaf),
            5 => Some(PageType::Overflow),
            _ => None,
        }
    }
}

pub type PageData = [u8; PAGE_SIZE];

#[derive(Clone)]
pub struct Page {
    pub id: PageId,
    pub data: Box<PageData>,
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

    pub fn page_type(&self) -> Option<PageType> {
        PageType::from_u8(self.data[0])
    }

    pub fn set_page_type(&mut self, pt: PageType) {
        self.data[0] = pt as u8;
        self.dirty = true;
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
        u32::from_le_bytes(self.data[offset..offset + 4].try_into().unwrap())
    }

    pub fn write_u32(&mut self, offset: usize, val: u32) {
        let bytes = val.to_le_bytes();
        self.data[offset..offset + 4].copy_from_slice(&bytes);
        self.dirty = true;
    }

    pub fn read_u64(&self, offset: usize) -> u64 {
        u64::from_le_bytes(self.data[offset..offset + 8].try_into().unwrap())
    }

    pub fn write_u64(&mut self, offset: usize, val: u64) {
        let bytes = val.to_le_bytes();
        self.data[offset..offset + 8].copy_from_slice(&bytes);
        self.dirty = true;
    }
}

// Heap page layout:
// [0]: page_type (u8)
// [1..3]: num_slots (u16)
// [3..5]: free_start (u16) - next byte after last row data (grows up from 8)
// [5..7]: next_page_lo (u16) - lower 16 bits of next page id (0 = no next page)
// [7..15]: next_page (u64) - next page in heap chain
// [16..]: row data area then slot array from end
// Slot array from end: each slot = [offset:u16][length:u16], grows downward

pub const HEAP_HEADER_SIZE: usize = 16;
pub const SLOT_SIZE: usize = 4; // offset(u16) + length(u16)

pub struct HeapPage<'a> {
    pub page: &'a mut Page,
}

impl<'a> HeapPage<'a> {
    pub fn new(page: &'a mut Page) -> Self {
        HeapPage { page }
    }

    pub fn init(&mut self) {
        self.page.set_page_type(PageType::Heap);
        self.page.write_u16(1, 0); // num_slots = 0
        self.page.write_u16(3, HEAP_HEADER_SIZE as u16); // free_start = after header
        self.page.write_u64(7, 0); // next_page = 0
    }

    pub fn num_slots(&self) -> u16 {
        self.page.read_u16(1)
    }

    pub fn set_num_slots(&mut self, n: u16) {
        self.page.write_u16(1, n);
    }

    pub fn free_start(&self) -> u16 {
        self.page.read_u16(3)
    }

    pub fn set_free_start(&mut self, v: u16) {
        self.page.write_u16(3, v);
    }

    pub fn next_page(&self) -> u64 {
        self.page.read_u64(7)
    }

    pub fn set_next_page(&mut self, pid: u64) {
        self.page.write_u64(7, pid);
    }

    pub fn slot_array_end(&self) -> usize {
        PAGE_SIZE - self.num_slots() as usize * SLOT_SIZE
    }

    pub fn free_space(&self) -> usize {
        let data_end = self.free_start() as usize;
        let slot_start = self.slot_array_end();
        if slot_start > data_end {
            slot_start - data_end
        } else {
            0
        }
    }

    pub fn can_fit(&self, data_len: usize) -> bool {
        self.free_space() >= data_len + SLOT_SIZE
    }

    pub fn get_slot(&self, slot_idx: u16) -> Option<(u16, u16)> {
        if slot_idx >= self.num_slots() {
            return None;
        }
        let slot_pos = PAGE_SIZE - (slot_idx as usize + 1) * SLOT_SIZE;
        let offset = u16::from_le_bytes([self.page.data[slot_pos], self.page.data[slot_pos + 1]]);
        let length =
            u16::from_le_bytes([self.page.data[slot_pos + 2], self.page.data[slot_pos + 3]]);
        Some((offset, length))
    }

    pub fn get_row_data(&self, slot_idx: u16) -> Option<&[u8]> {
        let (offset, length) = self.get_slot(slot_idx)?;
        if length == 0 {
            return None; // deleted
        }
        Some(&self.page.data[offset as usize..offset as usize + length as usize])
    }

    pub fn insert_row(&mut self, data: &[u8]) -> Option<u16> {
        if !self.can_fit(data.len()) {
            return None;
        }
        let offset = self.free_start();
        let slot_idx = self.num_slots();

        // Write data at offset
        let end = offset as usize + data.len();
        self.page.data[offset as usize..end].copy_from_slice(data);
        self.page.dirty = true;

        // Write slot at end of slot array
        let slot_pos = PAGE_SIZE - (slot_idx as usize + 1) * SLOT_SIZE;
        let offset_bytes = offset.to_le_bytes();
        let len_bytes = (data.len() as u16).to_le_bytes();
        self.page.data[slot_pos] = offset_bytes[0];
        self.page.data[slot_pos + 1] = offset_bytes[1];
        self.page.data[slot_pos + 2] = len_bytes[0];
        self.page.data[slot_pos + 3] = len_bytes[1];

        self.set_free_start(offset + data.len() as u16);
        self.set_num_slots(slot_idx + 1);

        Some(slot_idx)
    }

    pub fn delete_row(&mut self, slot_idx: u16) {
        let n = self.num_slots();
        if slot_idx >= n {
            return;
        }
        let slot_pos = PAGE_SIZE - (slot_idx as usize + 1) * SLOT_SIZE;
        // Mark as deleted by setting length to 0
        self.page.data[slot_pos + 2] = 0;
        self.page.data[slot_pos + 3] = 0;
        self.page.dirty = true;
    }

    pub fn update_row(&mut self, slot_idx: u16, data: &[u8]) -> bool {
        if let Some((offset, length)) = self.get_slot(slot_idx) {
            if data.len() as u16 <= length {
                // Fits in place
                self.page.data[offset as usize..offset as usize + data.len()].copy_from_slice(data);
                // Update length
                let slot_pos = PAGE_SIZE - (slot_idx as usize + 1) * SLOT_SIZE;
                let len_bytes = (data.len() as u16).to_le_bytes();
                self.page.data[slot_pos + 2] = len_bytes[0];
                self.page.data[slot_pos + 3] = len_bytes[1];
                self.page.dirty = true;
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heap_page_basic() {
        let mut page = Page::new(PageId(1));
        let mut hp = HeapPage::new(&mut page);
        hp.init();
        assert_eq!(hp.num_slots(), 0);

        let data = b"hello world";
        let slot = hp.insert_row(data).unwrap();
        assert_eq!(slot, 0);
        assert_eq!(hp.num_slots(), 1);

        let row_data = hp.get_row_data(0).unwrap();
        assert_eq!(row_data, data);
    }

    #[test]
    fn test_heap_page_multiple_rows() {
        let mut page = Page::new(PageId(1));
        let mut hp = HeapPage::new(&mut page);
        hp.init();

        for i in 0..10u8 {
            let data = vec![i; 100];
            hp.insert_row(&data).unwrap();
        }
        assert_eq!(hp.num_slots(), 10);

        for i in 0..10u8 {
            let row = hp.get_row_data(i as u16).unwrap();
            assert_eq!(row[0], i);
        }
    }

    #[test]
    fn test_heap_page_delete() {
        let mut page = Page::new(PageId(1));
        let mut hp = HeapPage::new(&mut page);
        hp.init();

        hp.insert_row(b"row0").unwrap();
        hp.insert_row(b"row1").unwrap();
        hp.delete_row(0);

        assert!(hp.get_row_data(0).is_none());
        assert_eq!(hp.get_row_data(1).unwrap(), b"row1");
    }
}
