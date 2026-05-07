//! Slotted-page heap for storing tuples.
//!
//! Each heap page has a small fixed header followed by a forward-growing
//! tuple area and a backward-growing slot directory.
//!
//! Layout (offsets in bytes):
//!   0..8   next_page_id (u64)  -- next page in this table's chain, 0 = none
//!   8..10  num_slots    (u16)
//!   10..12 free_start   (u16)  -- next byte to write a tuple to
//!   12..14 free_end     (u16)  -- byte just past the last slot entry
//!   14..16 reserved
//!   slot directory grows downward from PAGE_SIZE; each slot = (offset:u16, len:u16)

use crate::error::{MiniDbError, Result};
use crate::storage::page::{Page, PageId, PAGE_SIZE};
use crate::storage::pager::Pager;

const HDR_LEN: usize = 16;
const SLOT_LEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowId {
    pub page: PageId,
    pub slot: u16,
}

pub fn init_heap_page(page: &mut Page) {
    page.data[0..8].copy_from_slice(&0u64.to_le_bytes());
    page.data[8..10].copy_from_slice(&0u16.to_le_bytes());
    page.data[10..12].copy_from_slice(&(HDR_LEN as u16).to_le_bytes());
    page.data[12..14].copy_from_slice(&(PAGE_SIZE as u16).to_le_bytes());
    for b in &mut page.data[14..16] {
        *b = 0;
    }
}

fn read_u16(p: &Page, off: usize) -> u16 {
    u16::from_le_bytes([p.data[off], p.data[off + 1]])
}
fn read_u64(p: &Page, off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&p.data[off..off + 8]);
    u64::from_le_bytes(a)
}
fn write_u16(p: &mut Page, off: usize, v: u16) {
    p.data[off..off + 2].copy_from_slice(&v.to_le_bytes());
}
fn write_u64(p: &mut Page, off: usize, v: u64) {
    p.data[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

pub fn page_next(p: &Page) -> PageId {
    PageId(read_u64(p, 0))
}
pub fn set_page_next(p: &mut Page, next: PageId) {
    write_u64(p, 0, next.raw());
}
pub fn num_slots(p: &Page) -> u16 {
    read_u16(p, 8)
}
fn free_start(p: &Page) -> u16 {
    read_u16(p, 10)
}
fn free_end(p: &Page) -> u16 {
    read_u16(p, 12)
}

pub fn slot_info(p: &Page, slot: u16) -> Option<(u16, u16)> {
    if slot >= num_slots(p) {
        return None;
    }
    let off = PAGE_SIZE - (slot as usize + 1) * SLOT_LEN;
    let toff = read_u16(p, off);
    let tlen = read_u16(p, off + 2);
    if tlen == 0 && toff == 0 {
        return None;
    }
    Some((toff, tlen))
}

pub fn read_tuple<'a>(p: &'a Page, slot: u16) -> Option<&'a [u8]> {
    let (off, len) = slot_info(p, slot)?;
    if len == 0 {
        return None;
    }
    Some(&p.data[off as usize..off as usize + len as usize])
}

pub fn try_insert_tuple(p: &mut Page, tuple: &[u8]) -> Option<u16> {
    let needed = tuple.len() + SLOT_LEN;
    let fs = free_start(p) as usize;
    let fe = free_end(p) as usize;
    if fs + needed > fe {
        return None;
    }
    let new_slot = num_slots(p);
    let toff = fs;
    let tlen = tuple.len();
    p.data[toff..toff + tlen].copy_from_slice(tuple);
    let new_fs = fs + tlen;
    let new_fe = fe - SLOT_LEN;
    write_u16(p, new_fe, toff as u16);
    write_u16(p, new_fe + 2, tlen as u16);
    write_u16(p, 8, new_slot + 1);
    write_u16(p, 10, new_fs as u16);
    write_u16(p, 12, new_fe as u16);
    Some(new_slot)
}

/// Tombstone: mark slot as empty. We do not compact here.
pub fn delete_slot(p: &mut Page, slot: u16) {
    if slot >= num_slots(p) {
        return;
    }
    let off = PAGE_SIZE - (slot as usize + 1) * SLOT_LEN;
    write_u16(p, off, 0);
    write_u16(p, off + 2, 0);
}

/// Iterate all live (page, slot) pairs in a chain starting at first_page.
pub struct HeapIter<'a> {
    pager: &'a mut Pager,
    cur: Option<PageId>,
    next_slot: u16,
    cur_slots: u16,
}

impl<'a> HeapIter<'a> {
    pub fn new(pager: &'a mut Pager, first: Option<PageId>) -> Self {
        let cur_slots = match first {
            Some(p) => match pager.read_page(p) {
                Ok(pg) => num_slots(pg),
                Err(_) => 0,
            },
            None => 0,
        };
        HeapIter {
            pager,
            cur: first,
            next_slot: 0,
            cur_slots,
        }
    }
}

impl<'a> Iterator for HeapIter<'a> {
    type Item = (RowId, Vec<u8>);
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let pid = self.cur?;
            while self.next_slot < self.cur_slots {
                let slot = self.next_slot;
                self.next_slot += 1;
                let pg = self.pager.read_page(pid).ok()?;
                if let Some(buf) = read_tuple(pg, slot) {
                    let row = RowId { page: pid, slot };
                    return Some((row, buf.to_vec()));
                }
            }
            // advance
            let pg = self.pager.read_page(pid).ok()?;
            let nxt = page_next(pg);
            if nxt.raw() == 0 {
                self.cur = None;
                return None;
            }
            self.cur = Some(nxt);
            let pg2 = self.pager.read_page(nxt).ok()?;
            self.cur_slots = num_slots(pg2);
            self.next_slot = 0;
        }
    }
}

/// Insert a tuple into the chain starting at `first_page`. If first_page is None, allocate
/// a new page. Returns RowId, plus possibly the (new) first page id of the chain.
pub fn heap_insert(
    pager: &mut Pager,
    first_page: Option<PageId>,
    tuple: &[u8],
) -> Result<(RowId, PageId)> {
    if tuple.len() + SLOT_LEN + HDR_LEN > PAGE_SIZE {
        return Err(MiniDbError::Storage(format!(
            "tuple too large for a page: {} bytes",
            tuple.len()
        )));
    }
    // Walk chain to find page with room
    let mut cur = first_page;
    let mut last = None;
    while let Some(pid) = cur {
        last = Some(pid);
        let pg = pager.read_page(pid)?.clone();
        let needed = tuple.len() + SLOT_LEN;
        if (free_start(&pg) as usize) + needed <= (free_end(&pg) as usize) {
            let pg_mut = pager.page_mut(pid)?;
            let s = try_insert_tuple(pg_mut, tuple).expect("space pre-checked");
            return Ok((RowId { page: pid, slot: s }, first_page.unwrap()));
        }
        cur = if page_next(&pg).raw() == 0 {
            None
        } else {
            Some(page_next(&pg))
        };
    }
    // Allocate new page, link from last (or set as new first).
    let new_id = pager.allocate_page()?;
    {
        let pg = pager.page_mut(new_id)?;
        init_heap_page(pg);
        let s = try_insert_tuple(pg, tuple)
            .ok_or_else(|| MiniDbError::Storage("tuple did not fit in fresh page".into()))?;
        let row = RowId {
            page: new_id,
            slot: s,
        };
        // Drop borrow of pg before mutating last
        let new_first = match (first_page, last) {
            (None, _) => new_id,
            (Some(first), Some(last_id)) => {
                let last_pg = pager.page_mut(last_id)?;
                set_page_next(last_pg, new_id);
                first
            }
            (Some(first), None) => first,
        };
        return Ok((row, new_first));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn page_init_and_insert() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("h.db");
        let mut p = Pager::open(&path).unwrap();
        let id = p.allocate_page().unwrap();
        {
            let pg = p.page_mut(id).unwrap();
            init_heap_page(pg);
            assert!(try_insert_tuple(pg, b"hello").is_some());
            assert!(try_insert_tuple(pg, b"world").is_some());
            assert_eq!(num_slots(pg), 2);
        }
        let pg = p.read_page(id).unwrap();
        assert_eq!(read_tuple(pg, 0).unwrap(), b"hello");
        assert_eq!(read_tuple(pg, 1).unwrap(), b"world");
    }

    #[test]
    fn chain_insert_when_full() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("h.db");
        let mut p = Pager::open(&path).unwrap();
        let big = vec![b'x'; 3000];
        let (_r1, first) = heap_insert(&mut p, None, &big).unwrap();
        let (_r2, first2) = heap_insert(&mut p, Some(first), &big).unwrap();
        assert_eq!(first, first2);
        let mut count = 0;
        for _ in HeapIter::new(&mut p, Some(first)) {
            count += 1;
        }
        assert_eq!(count, 2);
    }
}
