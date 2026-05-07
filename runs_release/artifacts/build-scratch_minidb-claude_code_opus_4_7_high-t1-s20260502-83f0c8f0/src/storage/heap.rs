//! Slotted heap pages chained into a per-table heap file.
//!
//! Page format (offsets in bytes inside a 4096-byte page):
//!
//! ```text
//!   0..2    u16  num_slots
//!   2..4    u16  free_high       (lowest data offset; new record goes below this)
//!   4..12   u64  next_page       (0 if last page)
//!   12..16  u32  reserved
//!   16..16+4*num_slots           slot directory (u16 offset, u16 length); length==0 means deleted
//!   ... free space ...
//!   [free_high .. PAGE_SIZE]      record storage, growing downward
//! ```
//!
//! Records use the variable-length encoding from `codec`.

use crate::error::{MiniDbError, Result};
use crate::storage::codec;
use crate::storage::page::{PageId, RowId, PAGE_SIZE};
use crate::storage::pager::Pager;
use crate::types::Value;

const HEADER_LEN: usize = 16;
const SLOT_LEN: usize = 4;

fn read_u16(buf: &[u8], off: usize) -> u16 {
    let mut a = [0u8; 2];
    a.copy_from_slice(&buf[off..off + 2]);
    u16::from_le_bytes(a)
}

fn read_u64(buf: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&buf[off..off + 8]);
    u64::from_le_bytes(a)
}

fn write_u16(buf: &mut [u8], off: usize, v: u16) {
    buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

fn write_u32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn write_u64(buf: &mut [u8], off: usize, v: u64) {
    buf[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

#[derive(Debug, Clone, Copy)]
pub struct PageHeader {
    pub num_slots: u16,
    pub free_high: u16,
    pub next_page: u64,
}

impl PageHeader {
    pub fn read(buf: &[u8]) -> Self {
        PageHeader {
            num_slots: read_u16(buf, 0),
            free_high: read_u16(buf, 2),
            next_page: read_u64(buf, 4),
        }
    }

    pub fn write(&self, buf: &mut [u8]) {
        write_u16(buf, 0, self.num_slots);
        write_u16(buf, 2, self.free_high);
        write_u64(buf, 4, self.next_page);
        write_u32(buf, 12, 0);
    }

    /// Free space available for a *new* slot+record pair.
    pub fn free_bytes(&self) -> usize {
        let used_dir = HEADER_LEN + (self.num_slots as usize) * SLOT_LEN;
        if (self.free_high as usize) <= used_dir {
            0
        } else {
            self.free_high as usize - used_dir
        }
    }
}

pub fn init_page(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        *b = 0;
    }
    let h = PageHeader {
        num_slots: 0,
        free_high: PAGE_SIZE as u16,
        next_page: 0,
    };
    h.write(buf);
}

fn slot_offset(idx: u16) -> usize {
    HEADER_LEN + (idx as usize) * SLOT_LEN
}

/// Read a slot as `(offset, length)`. `length == 0` means deleted/empty.
pub fn read_slot(buf: &[u8], idx: u16) -> (u16, u16) {
    let off = slot_offset(idx);
    (read_u16(buf, off), read_u16(buf, off + 2))
}

fn write_slot(buf: &mut [u8], idx: u16, off: u16, len: u16) {
    let pos = slot_offset(idx);
    write_u16(buf, pos, off);
    write_u16(buf, pos + 2, len);
}

/// Try to append a record into a page buffer. Returns the slot index on success.
pub fn try_insert_into_page(buf: &mut [u8], record: &[u8]) -> Option<u16> {
    let mut h = PageHeader::read(buf);
    let needed = SLOT_LEN + record.len();
    if h.free_bytes() < needed {
        return None;
    }
    let new_off = h.free_high as usize - record.len();
    buf[new_off..new_off + record.len()].copy_from_slice(record);
    let slot = h.num_slots;
    h.num_slots += 1;
    h.free_high = new_off as u16;
    h.write(buf);
    write_slot(buf, slot, new_off as u16, record.len() as u16);
    Some(slot)
}

/// Mark a slot as deleted by setting length=0.
pub fn delete_slot(buf: &mut [u8], slot: u16) {
    let h = PageHeader::read(buf);
    if slot >= h.num_slots {
        return;
    }
    write_slot(buf, slot, 0, 0);
}

pub fn read_record<'a>(buf: &'a [u8], slot: u16) -> Option<&'a [u8]> {
    let h = PageHeader::read(buf);
    if slot >= h.num_slots {
        return None;
    }
    let (off, len) = read_slot(buf, slot);
    if len == 0 {
        return None;
    }
    Some(&buf[off as usize..off as usize + len as usize])
}

/// A heap is an append-friendly chain of slotted pages associated with a single table.
pub struct Heap;

impl Heap {
    /// Create a new empty heap and return its first page id.
    pub fn create(pager: &mut Pager) -> Result<PageId> {
        let id = pager.allocate_page()?;
        let mut buf = vec![0u8; PAGE_SIZE];
        init_page(&mut buf);
        pager.write_page(id, &buf)?;
        Ok(id)
    }

    /// Insert a row into the heap rooted at `root`. Returns the assigned RowId.
    pub fn insert(pager: &mut Pager, root: PageId, values: &[Value]) -> Result<RowId> {
        let mut record = Vec::with_capacity(64);
        codec::encode_row(values, &mut record)?;
        if record.len() + SLOT_LEN > PAGE_SIZE - HEADER_LEN {
            return Err(MiniDbError::storage("row too large for a single page"));
        }
        // Walk the chain until we find a page with room or reach the tail.
        let mut current = root;
        loop {
            let mut buf = pager.read_page(current)?;
            if let Some(slot) = try_insert_into_page(&mut buf, &record) {
                pager.write_page(current, &buf)?;
                return Ok(RowId::new(current, slot));
            }
            let h = PageHeader::read(&buf);
            if h.next_page == 0 {
                let new_page = pager.allocate_page()?;
                let mut nbuf = vec![0u8; PAGE_SIZE];
                init_page(&mut nbuf);
                let slot = try_insert_into_page(&mut nbuf, &record)
                    .ok_or_else(|| MiniDbError::storage("row will not fit in fresh page"))?;
                pager.write_page(new_page, &nbuf)?;
                let mut h2 = PageHeader::read(&buf);
                h2.next_page = new_page.0;
                h2.write(&mut buf);
                pager.write_page(current, &buf)?;
                return Ok(RowId::new(new_page, slot));
            }
            current = PageId(h.next_page);
        }
    }

    /// Delete a row by row id. No-ops on missing rows.
    pub fn delete(pager: &mut Pager, rid: RowId) -> Result<bool> {
        let mut buf = pager.read_page(rid.page)?;
        let h = PageHeader::read(&buf);
        if rid.slot >= h.num_slots {
            return Ok(false);
        }
        let (_off, len) = read_slot(&buf, rid.slot);
        if len == 0 {
            return Ok(false);
        }
        delete_slot(&mut buf, rid.slot);
        pager.write_page(rid.page, &buf)?;
        Ok(true)
    }

    /// Read a row by row id. Returns None if the row was deleted or out of range.
    pub fn read(pager: &mut Pager, rid: RowId) -> Result<Option<Vec<Value>>> {
        let buf = pager.read_page(rid.page)?;
        let Some(rec) = read_record(&buf, rid.slot) else {
            return Ok(None);
        };
        let (row, _) = codec::decode_row(rec)?;
        Ok(Some(row))
    }

    /// Update a row in place when possible, falling back to delete+insert.
    pub fn update(
        pager: &mut Pager,
        root: PageId,
        rid: RowId,
        new_values: &[Value],
    ) -> Result<RowId> {
        let mut record = Vec::with_capacity(64);
        codec::encode_row(new_values, &mut record)?;
        let mut buf = pager.read_page(rid.page)?;
        let (off, len) = read_slot(&buf, rid.slot);
        if len as usize == record.len() && len != 0 {
            buf[off as usize..off as usize + len as usize].copy_from_slice(&record);
            pager.write_page(rid.page, &buf)?;
            return Ok(rid);
        }
        // Otherwise: delete + reinsert (may move to a different page).
        delete_slot(&mut buf, rid.slot);
        pager.write_page(rid.page, &buf)?;
        Heap::insert(pager, root, new_values)
    }

    /// Iterate every (RowId, Row) in the heap.
    pub fn scan_collect(pager: &mut Pager, root: PageId) -> Result<Vec<(RowId, Vec<Value>)>> {
        let mut out = Vec::new();
        let mut current = root;
        loop {
            let buf = pager.read_page(current)?;
            let h = PageHeader::read(&buf);
            for slot in 0..h.num_slots {
                if let Some(rec) = read_record(&buf, slot) {
                    let (row, _) = codec::decode_row(rec)?;
                    out.push((RowId::new(current, slot), row));
                }
            }
            if h.next_page == 0 {
                break;
            }
            current = PageId(h.next_page);
        }
        Ok(out)
    }

    /// Free every page in the heap chain rooted at `root`.
    pub fn drop_heap(pager: &mut Pager, root: PageId) -> Result<()> {
        let mut chain = Vec::new();
        let mut current = root;
        loop {
            let buf = pager.read_page(current)?;
            let h = PageHeader::read(&buf);
            chain.push(current);
            if h.next_page == 0 {
                break;
            }
            current = PageId(h.next_page);
        }
        for p in chain {
            pager.free_page(p)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = temp_dir();
        p.push(format!("minidb_heap_{}_{}.db", std::process::id(), n));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn insert_scan() {
        let path = tmp();
        let mut pager = Pager::open(&path).unwrap();
        let root = Heap::create(&mut pager).unwrap();
        let r1 = Heap::insert(&mut pager, root, &[Value::Int(1), Value::Text("a".into())]).unwrap();
        let r2 =
            Heap::insert(&mut pager, root, &[Value::Int(2), Value::Text("bb".into())]).unwrap();
        assert_ne!(r1, r2);
        let rows = Heap::scan_collect(&mut pager, root).unwrap();
        assert_eq!(rows.len(), 2);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn delete_then_scan() {
        let path = tmp();
        let mut pager = Pager::open(&path).unwrap();
        let root = Heap::create(&mut pager).unwrap();
        let r1 = Heap::insert(&mut pager, root, &[Value::Int(1)]).unwrap();
        Heap::insert(&mut pager, root, &[Value::Int(2)]).unwrap();
        Heap::delete(&mut pager, r1).unwrap();
        let rows = Heap::scan_collect(&mut pager, root).unwrap();
        assert_eq!(rows.len(), 1);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn many_rows_chain_pages() {
        let path = tmp();
        let mut pager = Pager::open(&path).unwrap();
        let root = Heap::create(&mut pager).unwrap();
        for i in 0..1000 {
            Heap::insert(
                &mut pager,
                root,
                &[Value::Int(i), Value::Text("x".repeat(40))],
            )
            .unwrap();
        }
        let rows = Heap::scan_collect(&mut pager, root).unwrap();
        assert_eq!(rows.len(), 1000);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn update_in_place() {
        let path = tmp();
        let mut pager = Pager::open(&path).unwrap();
        let root = Heap::create(&mut pager).unwrap();
        let r = Heap::insert(&mut pager, root, &[Value::Int(1), Value::Int(2)]).unwrap();
        let r2 = Heap::update(&mut pager, root, r, &[Value::Int(10), Value::Int(20)]).unwrap();
        assert_eq!(r, r2);
        let row = Heap::read(&mut pager, r2).unwrap().unwrap();
        assert_eq!(row, vec![Value::Int(10), Value::Int(20)]);
        std::fs::remove_file(&path).ok();
    }
}
