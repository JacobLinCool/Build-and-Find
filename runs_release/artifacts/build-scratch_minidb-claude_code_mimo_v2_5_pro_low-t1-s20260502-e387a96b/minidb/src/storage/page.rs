use crate::storage::{PageId, PAGE_SIZE};

pub const HEAP_MAGIC: u16 = 0x4850; // "HP"
pub const BTREE_LEAF_MAGIC: u16 = 0x424C; // "BL"
pub const BTREE_INTERNAL_MAGIC: u16 = 0x4249; // "BI"

/// Slotted page layout (4096 bytes):
/// [0..2]   magic: u16
/// [2..4]   num_slots: u16
/// [4..8]   data_start: u32 (byte offset where data area begins, grows backward from PAGE_SIZE)
/// [8..8+N*4]  slot directory: each entry = (offset: u16, length: u16)
/// [...free space...]
/// [data_start..PAGE_SIZE]  row data (packed, grows backward from PAGE_SIZE)
#[derive(Debug, Clone)]
pub struct PageData {
    pub page_id: PageId,
    pub raw: [u8; PAGE_SIZE],
}

impl PageData {
    pub fn new_heap(page_id: PageId) -> Self {
        let mut raw = [0u8; PAGE_SIZE];
        raw[0..2].copy_from_slice(&HEAP_MAGIC.to_le_bytes());
        // num_slots = 0
        // data_start = PAGE_SIZE (data grows backward, so starts at the end)
        raw[4..8].copy_from_slice(&(PAGE_SIZE as u32).to_le_bytes());
        Self { page_id, raw }
    }

    pub fn magic(&self) -> u16 {
        u16::from_le_bytes(self.raw[0..2].try_into().unwrap())
    }

    pub fn num_slots(&self) -> usize {
        u16::from_le_bytes(self.raw[2..4].try_into().unwrap()) as usize
    }

    fn set_num_slots(&mut self, n: u16) {
        self.raw[2..4].copy_from_slice(&n.to_le_bytes());
    }

    /// Data area starts here (grows backward from PAGE_SIZE)
    fn data_start(&self) -> usize {
        u32::from_le_bytes(self.raw[4..8].try_into().unwrap()) as usize
    }

    fn set_data_start(&mut self, pos: u32) {
        self.raw[4..8].copy_from_slice(&pos.to_le_bytes());
    }

    /// Slot directory ends at this offset
    fn slot_dir_end(&self) -> usize {
        8 + self.num_slots() * 4
    }

    /// Can we insert data of given length?
    pub fn can_insert(&self, data_len: usize) -> bool {
        let new_slot_dir_end = 8 + (self.num_slots() + 1) * 4;
        let ds = self.data_start();
        if data_len > ds {
            return false;
        }
        let new_data_start = ds - data_len;
        new_slot_dir_end <= new_data_start
    }

    /// Insert a row. Returns the slot index, or None if no space.
    pub fn insert_row_data(&mut self, data: &[u8]) -> Option<usize> {
        let data_len = data.len();
        if !self.can_insert(data_len) {
            return None;
        }

        let slot_idx = self.num_slots();
        let slot_offset = 8 + slot_idx * 4;

        // Data grows backward from PAGE_SIZE
        let new_data_start = self.data_start() - data_len;
        let data_offset = new_data_start;

        // Write slot entry
        self.raw[slot_offset..slot_offset + 2].copy_from_slice(&(data_offset as u16).to_le_bytes());
        self.raw[slot_offset + 2..slot_offset + 4]
            .copy_from_slice(&(data_len as u16).to_le_bytes());

        // Write data
        self.raw[data_offset..data_offset + data_len].copy_from_slice(data);

        // Update header
        self.set_num_slots((slot_idx + 1) as u16);
        self.set_data_start(new_data_start as u32);

        Some(slot_idx)
    }

    /// Get data for a slot
    pub fn get_slot(&self, idx: usize) -> Option<&[u8]> {
        if idx >= self.num_slots() {
            return None;
        }
        let slot_offset = 8 + idx * 4;
        let data_offset =
            u16::from_le_bytes(self.raw[slot_offset..slot_offset + 2].try_into().unwrap()) as usize;
        let data_len = u16::from_le_bytes(
            self.raw[slot_offset + 2..slot_offset + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        if data_offset == 0 && data_len == 0 {
            return None; // Deleted slot
        }
        Some(&self.raw[data_offset..data_offset + data_len])
    }

    /// Mark a slot as deleted
    pub fn delete_slot(&mut self, idx: usize) {
        if idx < self.num_slots() {
            let slot_offset = 8 + idx * 4;
            self.raw[slot_offset..slot_offset + 4].copy_from_slice(&[0u8; 4]);
        }
    }

    /// Update a slot with new data (in-place if same size or smaller, else fails)
    pub fn update_slot_data(&mut self, idx: usize, data: &[u8]) -> bool {
        if idx >= self.num_slots() {
            return false;
        }
        let slot_offset = 8 + idx * 4;
        let old_offset =
            u16::from_le_bytes(self.raw[slot_offset..slot_offset + 2].try_into().unwrap()) as usize;
        let old_len = u16::from_le_bytes(
            self.raw[slot_offset + 2..slot_offset + 4]
                .try_into()
                .unwrap(),
        ) as usize;

        if old_offset == 0 && old_len == 0 {
            return false; // deleted slot
        }

        if data.len() <= old_len {
            // Fits in place
            self.raw[old_offset..old_offset + data.len()].copy_from_slice(data);
            self.raw[slot_offset + 2..slot_offset + 4]
                .copy_from_slice(&(data.len() as u16).to_le_bytes());
            true
        } else {
            // Try to allocate from the data area
            let new_data_start = self.data_start() - data.len();
            let slot_dir_end = self.slot_dir_end();
            if new_data_start < slot_dir_end {
                return false; // No space
            }
            let data_offset = new_data_start;
            self.raw[data_offset..data_offset + data.len()].copy_from_slice(data);
            self.raw[slot_offset..slot_offset + 2]
                .copy_from_slice(&(data_offset as u16).to_le_bytes());
            self.raw[slot_offset + 2..slot_offset + 4]
                .copy_from_slice(&(data.len() as u16).to_le_bytes());
            self.set_data_start(new_data_start as u32);
            true
        }
    }

    /// Iterate over all non-deleted slots
    pub fn iter_rows(&self) -> impl Iterator<Item = (usize, &[u8])> {
        let n = self.num_slots();
        let raw = &self.raw;
        (0..n).filter_map(move |i| {
            let slot_offset = 8 + i * 4;
            let off =
                u16::from_le_bytes(raw[slot_offset..slot_offset + 2].try_into().unwrap()) as usize;
            let len = u16::from_le_bytes(raw[slot_offset + 2..slot_offset + 4].try_into().unwrap())
                as usize;
            if off == 0 && len == 0 {
                None
            } else {
                Some((i, &raw[off..off + len]))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_insert_and_read() {
        let mut page = PageData::new_heap(PageId(0));
        assert_eq!(page.num_slots(), 0);

        let data1 = b"hello";
        let slot0 = page.insert_row_data(data1).unwrap();
        assert_eq!(slot0, 0);
        assert_eq!(page.num_slots(), 1);

        let data2 = b"world!!";
        let slot1 = page.insert_row_data(data2).unwrap();
        assert_eq!(slot1, 1);

        assert_eq!(page.get_slot(0), Some(data1.as_slice()));
        assert_eq!(page.get_slot(1), Some(data2.as_slice()));
    }

    #[test]
    fn test_page_delete() {
        let mut page = PageData::new_heap(PageId(0));
        page.insert_row_data(b"aaa").unwrap();
        page.insert_row_data(b"bbb").unwrap();
        page.delete_slot(0);
        assert_eq!(page.get_slot(0), None);
        assert_eq!(page.get_slot(1), Some(b"bbb".as_slice()));
    }

    #[test]
    fn test_page_iter() {
        let mut page = PageData::new_heap(PageId(0));
        page.insert_row_data(b"a").unwrap();
        page.insert_row_data(b"bb").unwrap();
        page.insert_row_data(b"ccc").unwrap();
        page.delete_slot(1);
        let rows: Vec<_> = page.iter_rows().collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, b"a");
        assert_eq!(rows[1].1, b"ccc");
    }

    #[test]
    fn test_page_fill() {
        let mut page = PageData::new_heap(PageId(0));
        let big = vec![0xAAu8; 2000];
        assert!(page.insert_row_data(&big).is_some());
        assert!(page.insert_row_data(&big).is_some());
        // Third should fail - not enough space
        assert!(page.insert_row_data(&big).is_none());
    }
}
