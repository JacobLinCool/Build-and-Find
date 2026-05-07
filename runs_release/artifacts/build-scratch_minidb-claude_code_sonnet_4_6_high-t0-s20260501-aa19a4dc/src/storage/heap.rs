/// Heap file: stores variable-length rows in pages.
///
/// Page layout:
///   [2 bytes: num_slots][num_slots * slot_entry][free space][row data growing backwards]
///
/// Slot entry: [2 bytes: offset][2 bytes: length] (0 offset + 0 length = deleted)
///
/// The first page of a table heap has a special 8-byte header:
///   [4 bytes: magic 0xHEAP][4 bytes: next_row_id low 32 bits]
use crate::error::{MiniDbError, Result};
use crate::storage::page::{PageId, PAGE_SIZE};
use crate::storage::pager::Pager;
use crate::types::{Row, Schema};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Cursor;

pub const HEAP_MAGIC: u32 = 0x48454150; // 'HEAP'

/// Identifies a specific row within a table
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RowId(pub u64);

const SLOT_SIZE: usize = 4; // 2 bytes offset + 2 bytes length
const PAGE_HEADER_SIZE: usize = 2; // num_slots u16
const DATA_PAGE_HEADER_SIZE: usize = 6; // num_slots u16 + next_page u32

/// A heap page stores slots (offset, len) pointing into its data area.
/// Pages link together via a 4-byte "next page" pointer in the header.
pub struct HeapPage<'a> {
    data: &'a mut [u8; PAGE_SIZE],
}

impl<'a> HeapPage<'a> {
    pub fn new(data: &'a mut [u8; PAGE_SIZE]) -> Self {
        HeapPage { data }
    }

    fn num_slots(&self) -> u16 {
        u16::from_le_bytes([self.data[0], self.data[1]])
    }

    fn set_num_slots(&mut self, n: u16) {
        let bytes = n.to_le_bytes();
        self.data[0] = bytes[0];
        self.data[1] = bytes[1];
    }

    fn next_page(&self) -> u32 {
        u32::from_le_bytes([self.data[2], self.data[3], self.data[4], self.data[5]])
    }

    fn set_next_page(&mut self, page: u32) {
        let bytes = page.to_le_bytes();
        self.data[2] = bytes[0];
        self.data[3] = bytes[1];
        self.data[4] = bytes[2];
        self.data[5] = bytes[3];
    }

    fn slot_offset(idx: u16) -> usize {
        DATA_PAGE_HEADER_SIZE + idx as usize * SLOT_SIZE
    }

    fn read_slot(&self, idx: u16) -> (u16, u16) {
        let off = Self::slot_offset(idx);
        let offset = u16::from_le_bytes([self.data[off], self.data[off + 1]]);
        let length = u16::from_le_bytes([self.data[off + 2], self.data[off + 3]]);
        (offset, length)
    }

    fn write_slot(&mut self, idx: u16, offset: u16, length: u16) {
        let off = Self::slot_offset(idx);
        let offset_bytes = offset.to_le_bytes();
        let length_bytes = length.to_le_bytes();
        self.data[off] = offset_bytes[0];
        self.data[off + 1] = offset_bytes[1];
        self.data[off + 2] = length_bytes[0];
        self.data[off + 3] = length_bytes[1];
    }

    /// Free space available in this page
    pub fn free_space(&self) -> usize {
        let num_slots = self.num_slots();
        let slots_end = Self::slot_offset(num_slots);
        // Find the lowest used offset (data starts from bottom of page)
        let mut low_offset = PAGE_SIZE;
        for i in 0..num_slots {
            let (off, len) = self.read_slot(i);
            if len > 0 && (off as usize) < low_offset {
                low_offset = off as usize;
            }
        }
        // Account for a new slot header
        if low_offset >= slots_end + SLOT_SIZE {
            low_offset - slots_end - SLOT_SIZE
        } else {
            0
        }
    }

    /// Insert row data, returning the slot index
    pub fn insert(&mut self, row_data: &[u8]) -> Option<u16> {
        let data_len = row_data.len();
        if data_len > 0xFFFF {
            return None;
        }
        let num_slots = self.num_slots();
        let slots_end = Self::slot_offset(num_slots + 1);

        // Find lowest existing data offset
        let mut low_offset = PAGE_SIZE;
        for i in 0..num_slots {
            let (off, len) = self.read_slot(i);
            if len > 0 && (off as usize) < low_offset {
                low_offset = off as usize;
            }
        }

        // Check space
        if low_offset < slots_end + data_len {
            return None;
        }

        let new_offset = low_offset - data_len;
        // Copy row data
        self.data[new_offset..new_offset + data_len].copy_from_slice(row_data);
        // Write slot
        self.write_slot(num_slots, new_offset as u16, data_len as u16);
        self.set_num_slots(num_slots + 1);

        Some(num_slots)
    }

    /// Read row data at slot idx
    pub fn read_row(&self, slot: u16) -> Option<&[u8]> {
        if slot >= self.num_slots() {
            return None;
        }
        let (offset, length) = self.read_slot(slot);
        if length == 0 {
            return None; // deleted
        }
        Some(&self.data[offset as usize..offset as usize + length as usize])
    }

    /// Delete a row (mark slot as empty)
    pub fn delete_row(&mut self, slot: u16) -> bool {
        if slot >= self.num_slots() {
            return false;
        }
        let (_, len) = self.read_slot(slot);
        if len == 0 {
            return false;
        }
        self.write_slot(slot, 0, 0); // mark deleted
        true
    }

    /// Update a row in place (only works if same length or fits)
    pub fn update_row(&mut self, slot: u16, new_data: &[u8]) -> bool {
        if slot >= self.num_slots() {
            return false;
        }
        let (offset, length) = self.read_slot(slot);
        if length == 0 {
            return false;
        }
        if new_data.len() == length as usize {
            self.data[offset as usize..offset as usize + length as usize].copy_from_slice(new_data);
            true
        } else {
            // Mark old slot deleted and insert as new
            self.write_slot(slot, 0, 0);
            // Try to insert new data
            self.insert(new_data).is_some()
        }
    }
}

/// Heap manager: manages the set of pages for a table
pub struct Heap {
    pub root_page: PageId,
}

impl Heap {
    /// Create a new heap with a root page
    pub fn create(pager: &mut Pager) -> Result<Self> {
        let id = pager.allocate_page()?;
        let page = pager.read_page_mut(id)?;
        // Initialize root page
        page.data[0..2].copy_from_slice(&0u16.to_le_bytes()); // num_slots = 0
        page.data[2..6].copy_from_slice(&0u32.to_le_bytes()); // next_page = 0 (none)
        page.mark_dirty();
        Ok(Heap { root_page: id })
    }

    pub fn open(root_page: PageId) -> Self {
        Heap { root_page }
    }

    /// Insert a row, returning its (page_id, slot) location
    pub fn insert_row(&self, pager: &mut Pager, row_data: &[u8]) -> Result<(PageId, u16)> {
        // Walk pages to find one with space
        let mut current_id = self.root_page;
        loop {
            let next_page = {
                let page = pager.read_page(current_id)?;
                let next =
                    u32::from_le_bytes([page.data[2], page.data[3], page.data[4], page.data[5]]);
                next
            };

            // Try inserting on current page
            {
                let page = pager.read_page_mut(current_id)?;
                let mut hp = HeapPage::new(&mut page.data);
                if let Some(slot) = hp.insert(row_data) {
                    page.dirty = true;
                    return Ok((current_id, slot));
                }
            }

            if next_page == 0 {
                // Allocate new page
                let new_id = pager.allocate_page()?;
                // Link current -> new
                {
                    let page = pager.read_page_mut(current_id)?;
                    let new_id_u32 = new_id.as_u64() as u32;
                    page.data[2] = new_id_u32.to_le_bytes()[0];
                    page.data[3] = new_id_u32.to_le_bytes()[1];
                    page.data[4] = new_id_u32.to_le_bytes()[2];
                    page.data[5] = new_id_u32.to_le_bytes()[3];
                    page.mark_dirty();
                }
                // Initialize new page
                {
                    let page = pager.read_page_mut(new_id)?;
                    page.data[0..2].copy_from_slice(&0u16.to_le_bytes());
                    page.data[2..6].copy_from_slice(&0u32.to_le_bytes());
                    page.mark_dirty();
                }
                current_id = new_id;
            } else {
                current_id = PageId(next_page as u64);
            }
        }
    }

    /// Scan all rows, calling callback for each live row: (page_id, slot, row_data)
    pub fn scan<F>(&self, pager: &mut Pager, mut callback: F) -> Result<()>
    where
        F: FnMut(PageId, u16, &[u8]) -> Result<bool>, // return false to stop
    {
        let mut current_id = self.root_page;
        loop {
            let (next_page, num_slots) = {
                let page = pager.read_page(current_id)?;
                let next =
                    u32::from_le_bytes([page.data[2], page.data[3], page.data[4], page.data[5]]);
                let ns = u16::from_le_bytes([page.data[0], page.data[1]]);
                (next, ns)
            };

            for slot in 0..num_slots {
                let row_data: Vec<u8> = {
                    let page = pager.read_page(current_id)?;
                    let hp = HeapPage::new(unsafe {
                        &mut *(page.data.as_ptr() as *mut [u8; PAGE_SIZE])
                    });
                    match hp.read_row(slot) {
                        Some(d) => d.to_vec(),
                        None => continue,
                    }
                };
                let cont = callback(current_id, slot, &row_data)?;
                if !cont {
                    return Ok(());
                }
            }

            if next_page == 0 {
                break;
            }
            current_id = PageId(next_page as u64);
        }
        Ok(())
    }

    /// Delete a row at the given location
    pub fn delete_row(&self, pager: &mut Pager, page_id: PageId, slot: u16) -> Result<()> {
        let page = pager.read_page_mut(page_id)?;
        let mut hp = HeapPage::new(&mut page.data);
        if !hp.delete_row(slot) {
            return Err(MiniDbError::NotFound(format!(
                "row at ({}, {}) not found",
                page_id, slot
            )));
        }
        page.dirty = true;
        Ok(())
    }

    /// Update a row at the given location
    pub fn update_row(
        &self,
        pager: &mut Pager,
        page_id: PageId,
        slot: u16,
        new_data: &[u8],
    ) -> Result<Option<(PageId, u16)>> {
        let updated = {
            let page = pager.read_page_mut(page_id)?;
            let mut hp = HeapPage::new(&mut page.data);
            let res = hp.update_row(slot, new_data);
            page.dirty = true;
            res
        };

        if updated {
            Ok(None) // updated in place
        } else {
            // Must delete and re-insert elsewhere
            self.delete_row(pager, page_id, slot)?;
            let new_loc = self.insert_row(pager, new_data)?;
            Ok(Some(new_loc))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::pager::Pager;
    use tempfile::NamedTempFile;

    #[test]
    fn test_heap_insert_scan() {
        let tmp = NamedTempFile::new().unwrap();
        let mut pager = Pager::open(tmp.path()).unwrap();
        let heap = Heap::create(&mut pager).unwrap();

        heap.insert_row(&mut pager, b"row1").unwrap();
        heap.insert_row(&mut pager, b"row2").unwrap();
        heap.insert_row(&mut pager, b"row3").unwrap();

        let mut rows = Vec::new();
        heap.scan(&mut pager, |_, _, data| {
            rows.push(data.to_vec());
            Ok(true)
        })
        .unwrap();

        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn test_heap_delete() {
        let tmp = NamedTempFile::new().unwrap();
        let mut pager = Pager::open(tmp.path()).unwrap();
        let heap = Heap::create(&mut pager).unwrap();

        let (pid, slot) = heap.insert_row(&mut pager, b"hello").unwrap();
        heap.insert_row(&mut pager, b"world").unwrap();

        heap.delete_row(&mut pager, pid, slot).unwrap();

        let mut rows = Vec::new();
        heap.scan(&mut pager, |_, _, data| {
            rows.push(data.to_vec());
            Ok(true)
        })
        .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], b"world");
    }

    #[test]
    fn test_heap_page_overflow() {
        let tmp = NamedTempFile::new().unwrap();
        let mut pager = Pager::open(tmp.path()).unwrap();
        let heap = Heap::create(&mut pager).unwrap();

        // Insert enough rows to overflow onto a second page
        let big_row = vec![0u8; 500];
        for _ in 0..20 {
            heap.insert_row(&mut pager, &big_row).unwrap();
        }

        let mut count = 0;
        heap.scan(&mut pager, |_, _, _| {
            count += 1;
            Ok(true)
        })
        .unwrap();

        assert_eq!(count, 20);
    }
}
