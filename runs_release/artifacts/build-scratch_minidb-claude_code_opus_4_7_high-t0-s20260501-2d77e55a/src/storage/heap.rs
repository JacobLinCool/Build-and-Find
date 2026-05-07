//! Heap file: a linked list of slotted pages that store rows for a single table.
//!
//! Page layout (little-endian):
//!   header (16 bytes):
//!       u16 num_slots
//!       u16 free_space_start  (start of slot directory growth)
//!       u16 free_space_end    (end of contiguous free space)
//!       u16 reserved
//!       u64 next_page_id      (0 = end of chain)
//!   slot directory: num_slots × (u16 offset, u16 length) entries.
//!     offset == 0 marks a tombstoned slot.
//!   tuple data grows downwards from the end of the page.
//!
//! Tuples are encoded with bincode.

use serde::{Deserialize, Serialize};

use crate::error::{MiniDbError, Result};
use crate::types::Value;

use super::page::{PageId, RowId, PAGE_SIZE};
use super::pager::{PageBuf, Pager};

const HDR_NUM_SLOTS: usize = 0;
const HDR_FREE_START: usize = 2;
const HDR_FREE_END: usize = 4;
const HDR_RESERVED: usize = 6;
const HDR_NEXT_PAGE: usize = 8;
pub const HEAP_HEADER_SIZE: usize = 16;

const SLOT_ENTRY_SIZE: usize = 4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tuple {
    pub values: Vec<Value>,
}

impl Tuple {
    pub fn new(values: Vec<Value>) -> Self {
        Self { values }
    }
}

/// In-memory view over a heap page. Allows reading and modifying its slots.
pub struct HeapPage<'a> {
    buf: &'a mut PageBuf,
}

impl<'a> HeapPage<'a> {
    pub fn from_buf(buf: &'a mut PageBuf) -> Self {
        Self { buf }
    }

    pub fn init(buf: &'a mut PageBuf) -> Self {
        for b in buf.iter_mut() {
            *b = 0;
        }
        let mut p = Self { buf };
        p.set_num_slots(0);
        p.set_free_start(HEAP_HEADER_SIZE as u16);
        p.set_free_end(PAGE_SIZE as u16);
        p.set_next_page(PageId::NULL);
        p
    }

    fn read_u16(&self, off: usize) -> u16 {
        u16::from_le_bytes(self.buf[off..off + 2].try_into().unwrap())
    }

    fn write_u16(&mut self, off: usize, v: u16) {
        self.buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }

    fn read_u64(&self, off: usize) -> u64 {
        u64::from_le_bytes(self.buf[off..off + 8].try_into().unwrap())
    }

    fn write_u64(&mut self, off: usize, v: u64) {
        self.buf[off..off + 8].copy_from_slice(&v.to_le_bytes());
    }

    pub fn num_slots(&self) -> u16 {
        self.read_u16(HDR_NUM_SLOTS)
    }

    pub fn set_num_slots(&mut self, v: u16) {
        self.write_u16(HDR_NUM_SLOTS, v);
    }

    pub fn free_start(&self) -> u16 {
        self.read_u16(HDR_FREE_START)
    }

    pub fn set_free_start(&mut self, v: u16) {
        self.write_u16(HDR_FREE_START, v);
    }

    pub fn free_end(&self) -> u16 {
        self.read_u16(HDR_FREE_END)
    }

    pub fn set_free_end(&mut self, v: u16) {
        self.write_u16(HDR_FREE_END, v);
    }

    pub fn next_page(&self) -> PageId {
        PageId(self.read_u64(HDR_NEXT_PAGE))
    }

    pub fn set_next_page(&mut self, p: PageId) {
        self.write_u64(HDR_NEXT_PAGE, p.0);
    }

    fn slot_entry_offset(&self, slot: u16) -> usize {
        HEAP_HEADER_SIZE + (slot as usize) * SLOT_ENTRY_SIZE
    }

    pub fn slot(&self, slot: u16) -> Option<(u16, u16)> {
        if slot >= self.num_slots() {
            return None;
        }
        let base = self.slot_entry_offset(slot);
        let off = self.read_u16(base);
        let len = self.read_u16(base + 2);
        if off == 0 {
            None
        } else {
            Some((off, len))
        }
    }

    fn set_slot(&mut self, slot: u16, off: u16, len: u16) {
        let base = self.slot_entry_offset(slot);
        self.write_u16(base, off);
        self.write_u16(base + 2, len);
    }

    pub fn read_tuple(&self, slot: u16) -> Result<Option<Tuple>> {
        match self.slot(slot) {
            None => Ok(None),
            Some((off, len)) => {
                let bytes = &self.buf[off as usize..(off + len) as usize];
                let t: Tuple = bincode::deserialize(bytes)?;
                Ok(Some(t))
            }
        }
    }

    pub fn iter_slots(&self) -> Vec<u16> {
        (0..self.num_slots()).collect()
    }

    /// Available contiguous free space considering the cost of a new slot entry.
    pub fn free_space_for_new(&self) -> usize {
        let used_dir = HEAP_HEADER_SIZE + (self.num_slots() as usize) * SLOT_ENTRY_SIZE;
        let end = self.free_end() as usize;
        let entry = SLOT_ENTRY_SIZE;
        if end > used_dir + entry {
            end - used_dir - entry
        } else {
            0
        }
    }

    pub fn free_space_for_existing(&self) -> usize {
        let used_dir = HEAP_HEADER_SIZE + (self.num_slots() as usize) * SLOT_ENTRY_SIZE;
        let end = self.free_end() as usize;
        end.saturating_sub(used_dir)
    }

    pub fn insert(&mut self, payload: &[u8]) -> Option<u16> {
        if self.free_space_for_new() < payload.len() {
            return None;
        }
        let n = self.num_slots();
        let new_end = self.free_end() as usize - payload.len();
        self.buf[new_end..new_end + payload.len()].copy_from_slice(payload);
        self.set_slot(n, new_end as u16, payload.len() as u16);
        self.set_num_slots(n + 1);
        self.set_free_end(new_end as u16);
        self.set_free_start(self.slot_entry_offset(n + 1) as u16);
        Some(n)
    }

    /// In-place update if the new payload fits in the existing slot's space; otherwise None.
    pub fn update_inplace(&mut self, slot: u16, payload: &[u8]) -> bool {
        if let Some((_, len)) = self.slot(slot) {
            if (len as usize) >= payload.len() {
                let off = self.slot(slot).unwrap().0 as usize;
                self.buf[off..off + payload.len()].copy_from_slice(payload);
                self.set_slot(slot, off as u16, payload.len() as u16);
                return true;
            }
        }
        false
    }

    pub fn delete(&mut self, slot: u16) -> bool {
        if self.slot(slot).is_some() {
            let entry = self.slot_entry_offset(slot);
            self.write_u16(entry, 0);
            self.write_u16(entry + 2, 0);
            true
        } else {
            false
        }
    }
}

/// Heap file abstraction over a linked list of pages.
pub struct Heap<'p> {
    pager: &'p mut Pager,
}

impl<'p> Heap<'p> {
    pub fn new(pager: &'p mut Pager) -> Self {
        Self { pager }
    }

    /// Allocate the first page of a new heap, returning its page id.
    pub fn create(&mut self) -> Result<PageId> {
        let id = self.pager.allocate();
        let mut buf = self.pager.read(id)?;
        HeapPage::init(buf.as_mut());
        self.pager.write(id, buf)?;
        Ok(id)
    }

    pub fn insert(&mut self, head: PageId, tuple: &Tuple) -> Result<RowId> {
        let payload = bincode::serialize(tuple)?;
        if payload.len() + SLOT_ENTRY_SIZE + HEAP_HEADER_SIZE > PAGE_SIZE {
            return Err(MiniDbError::storage("tuple too large for a page"));
        }
        let mut current = head;
        loop {
            let mut buf = self.pager.read(current)?;
            let mut page = HeapPage::from_buf(buf.as_mut());
            if page.free_space_for_new() >= payload.len() {
                let slot = page.insert(&payload).expect("checked");
                self.pager.write(current, buf)?;
                return Ok(RowId::new(current, slot));
            }
            let next = page.next_page();
            if next.is_null() {
                let new_id = self.pager.allocate();
                let mut new_buf = self.pager.read(new_id)?;
                HeapPage::init(new_buf.as_mut());
                let mut new_page = HeapPage::from_buf(new_buf.as_mut());
                let slot = new_page
                    .insert(&payload)
                    .ok_or_else(|| MiniDbError::storage("tuple too large for fresh page"))?;
                self.pager.write(new_id, new_buf)?;
                // Link the previous tail to this new page.
                let mut tail_buf = self.pager.read(current)?;
                let mut tail = HeapPage::from_buf(tail_buf.as_mut());
                tail.set_next_page(new_id);
                self.pager.write(current, tail_buf)?;
                return Ok(RowId::new(new_id, slot));
            }
            current = next;
        }
    }

    pub fn read(&mut self, row: RowId) -> Result<Option<Tuple>> {
        let mut buf = self.pager.read(row.page)?;
        let page = HeapPage::from_buf(buf.as_mut());
        page.read_tuple(row.slot)
    }

    pub fn delete(&mut self, row: RowId) -> Result<bool> {
        let mut buf = self.pager.read(row.page)?;
        let mut page = HeapPage::from_buf(buf.as_mut());
        let ok = page.delete(row.slot);
        if ok {
            self.pager.write(row.page, buf)?;
        }
        Ok(ok)
    }

    /// Update the tuple at `row`. If the new tuple does not fit in-place, the
    /// old slot is tombstoned and a new row id is returned (callers must update
    /// any indexes).
    pub fn update(&mut self, row: RowId, tuple: &Tuple) -> Result<RowId> {
        let payload = bincode::serialize(tuple)?;
        let mut buf = self.pager.read(row.page)?;
        let mut page = HeapPage::from_buf(buf.as_mut());
        if page.update_inplace(row.slot, &payload) {
            self.pager.write(row.page, buf)?;
            return Ok(row);
        }
        // Otherwise: tombstone old, insert anywhere.
        page.delete(row.slot);
        self.pager.write(row.page, buf)?;
        // Walk to head — caller passes head, but we have only `row`. Instead
        // we just append at the tail starting from row.page.
        self.insert(row.page, tuple)
    }

    /// Iterator-style scan: returns all (row_id, tuple) in the heap chain.
    pub fn scan(&mut self, head: PageId) -> Result<Vec<(RowId, Tuple)>> {
        let mut out = Vec::new();
        let mut current = head;
        while !current.is_null() {
            let mut buf = self.pager.read(current)?;
            let page = HeapPage::from_buf(buf.as_mut());
            for slot in page.iter_slots() {
                if let Some(t) = page.read_tuple(slot)? {
                    out.push((RowId::new(current, slot), t));
                }
            }
            current = page.next_page();
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn pager() -> (TempDir, Pager) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data");
        let p = Pager::open(&path).unwrap();
        (dir, p)
    }

    #[test]
    fn insert_and_scan() {
        let (_d, mut pager) = pager();
        let head = Heap::new(&mut pager).create().unwrap();
        let mut h = Heap::new(&mut pager);
        let r1 = h.insert(head, &Tuple::new(vec![Value::Int(1)])).unwrap();
        let r2 = h
            .insert(head, &Tuple::new(vec![Value::Text("hi".into())]))
            .unwrap();
        let rows = h.scan(head).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, r1);
        assert_eq!(rows[1].0, r2);
    }

    #[test]
    fn delete_and_update() {
        let (_d, mut pager) = pager();
        let head = Heap::new(&mut pager).create().unwrap();
        let mut h = Heap::new(&mut pager);
        let r = h.insert(head, &Tuple::new(vec![Value::Int(1)])).unwrap();
        h.update(r, &Tuple::new(vec![Value::Int(2)])).unwrap();
        let rows = h.scan(head).unwrap();
        assert_eq!(rows[0].1.values[0], Value::Int(2));
        h.delete(r).unwrap();
        let rows = h.scan(head).unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn spans_multiple_pages() {
        let (_d, mut pager) = pager();
        let head = Heap::new(&mut pager).create().unwrap();
        let mut h = Heap::new(&mut pager);
        for i in 0..200 {
            h.insert(
                head,
                &Tuple::new(vec![Value::Int(i), Value::Text("x".repeat(50))]),
            )
            .unwrap();
        }
        let rows = h.scan(head).unwrap();
        assert_eq!(rows.len(), 200);
    }
}
