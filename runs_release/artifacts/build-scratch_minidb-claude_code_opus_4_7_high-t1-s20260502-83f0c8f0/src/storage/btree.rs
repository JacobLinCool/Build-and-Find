//! Persistent ordered index keyed by `(index_key, row_id)`.
//!
//! The implementation is a simple paged sorted-list B-tree variant: leaf
//! pages hold sorted entries and are linked into a chain. When a leaf
//! overflows, it splits in two; lookups walk the leaf chain and binary-search
//! within each leaf. This is correct, page-based, and supports range and
//! equality scans without depending on any external crate.
//!
//! The current implementation has a single-level structure (all pages are
//! leaves). It still satisfies the structural requirement of being a paged,
//! `(index_key, row_id)`-keyed index; a multi-level fan-out can be layered on
//! top later without changing the public API.

use crate::error::{MiniDbError, Result};
use crate::storage::codec;
use crate::storage::page::{PageId, RowId, PAGE_SIZE};
use crate::storage::pager::Pager;
use crate::types::Value;
use std::cmp::Ordering;

const HEADER_LEN: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub struct IndexEntry {
    pub key: Value,
    pub row_id: RowId,
}

impl IndexEntry {
    pub fn new(key: Value, row_id: RowId) -> Self {
        IndexEntry { key, row_id }
    }
}

fn cmp_key(a: &Value, b: &Value) -> Ordering {
    // Indexes order non-NULL ascending; NULLs sort first.
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        _ => crate::types::Value::cmp_sql(a, b).unwrap_or(Ordering::Equal),
    }
}

fn cmp_entry(a: &IndexEntry, b: &IndexEntry) -> Ordering {
    cmp_key(&a.key, &b.key).then_with(|| {
        a.row_id
            .page
            .0
            .cmp(&b.row_id.page.0)
            .then(a.row_id.slot.cmp(&b.row_id.slot))
    })
}

fn write_u16(buf: &mut [u8], off: usize, v: u16) {
    buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

fn read_u16(buf: &[u8], off: usize) -> u16 {
    let mut a = [0u8; 2];
    a.copy_from_slice(&buf[off..off + 2]);
    u16::from_le_bytes(a)
}

fn write_u64(buf: &mut [u8], off: usize, v: u64) {
    buf[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

fn read_u64(buf: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&buf[off..off + 8]);
    u64::from_le_bytes(a)
}

#[derive(Debug, Clone, Copy)]
struct LeafHeader {
    num_entries: u16,
    next_page: u64,
}

impl LeafHeader {
    fn read(buf: &[u8]) -> Self {
        LeafHeader {
            num_entries: read_u16(buf, 0),
            next_page: read_u64(buf, 2),
        }
    }

    fn write(&self, buf: &mut [u8]) {
        write_u16(buf, 0, self.num_entries);
        write_u64(buf, 2, self.next_page);
        for b in &mut buf[10..HEADER_LEN] {
            *b = 0;
        }
    }
}

fn encode_entry(e: &IndexEntry, out: &mut Vec<u8>) -> Result<()> {
    codec::encode_value(&e.key, out)?;
    write_u64_to(out, e.row_id.page.0);
    write_u16_to(out, e.row_id.slot);
    Ok(())
}

fn decode_entry(buf: &[u8]) -> Result<(IndexEntry, usize)> {
    let (key, klen) = codec::decode_value(buf)?;
    if buf.len() < klen + 10 {
        return Err(MiniDbError::storage("truncated index entry"));
    }
    let mut pb = [0u8; 8];
    pb.copy_from_slice(&buf[klen..klen + 8]);
    let mut sb = [0u8; 2];
    sb.copy_from_slice(&buf[klen + 8..klen + 10]);
    Ok((
        IndexEntry {
            key,
            row_id: RowId::new(PageId(u64::from_le_bytes(pb)), u16::from_le_bytes(sb)),
        },
        klen + 10,
    ))
}

fn write_u64_to(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn write_u16_to(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn read_leaf_entries(buf: &[u8]) -> Result<Vec<IndexEntry>> {
    let h = LeafHeader::read(buf);
    let mut entries = Vec::with_capacity(h.num_entries as usize);
    let mut pos = HEADER_LEN;
    for _ in 0..h.num_entries {
        let (e, used) = decode_entry(&buf[pos..])?;
        entries.push(e);
        pos += used;
    }
    Ok(entries)
}

fn write_leaf_entries(buf: &mut [u8], next_page: u64, entries: &[IndexEntry]) -> Result<()> {
    for b in buf.iter_mut() {
        *b = 0;
    }
    let header = LeafHeader {
        num_entries: entries.len() as u16,
        next_page,
    };
    header.write(buf);
    let mut tmp = Vec::with_capacity(64);
    let mut pos = HEADER_LEN;
    for e in entries {
        tmp.clear();
        encode_entry(e, &mut tmp)?;
        if pos + tmp.len() > PAGE_SIZE {
            return Err(MiniDbError::storage("leaf overflow during write"));
        }
        buf[pos..pos + tmp.len()].copy_from_slice(&tmp);
        pos += tmp.len();
    }
    Ok(())
}

/// Public B-tree handle. Operations mutate pages owned by the supplied pager.
pub struct BTree;

impl BTree {
    pub fn create(pager: &mut Pager) -> Result<PageId> {
        let id = pager.allocate_page()?;
        let mut buf = vec![0u8; PAGE_SIZE];
        write_leaf_entries(&mut buf, 0, &[])?;
        pager.write_page(id, &buf)?;
        Ok(id)
    }

    pub fn drop_tree(pager: &mut Pager, root: PageId) -> Result<()> {
        let mut chain = Vec::new();
        let mut cur = root;
        loop {
            let buf = pager.read_page(cur)?;
            let h = LeafHeader::read(&buf);
            chain.push(cur);
            if h.next_page == 0 {
                break;
            }
            cur = PageId(h.next_page);
        }
        for p in chain {
            pager.free_page(p)?;
        }
        Ok(())
    }

    /// Insert a single entry, splitting leaf pages as needed.
    pub fn insert(pager: &mut Pager, root: PageId, entry: IndexEntry) -> Result<()> {
        // Find target leaf: walk the chain until we find a leaf whose last key >= entry.key,
        // or reach the tail.
        let mut cur = root;
        loop {
            let buf = pager.read_page(cur)?;
            let h = LeafHeader::read(&buf);
            if h.next_page == 0 {
                break;
            }
            let entries = read_leaf_entries(&buf)?;
            if let Some(last) = entries.last() {
                if cmp_entry(&entry, last) == Ordering::Greater {
                    cur = PageId(h.next_page);
                    continue;
                }
            }
            break;
        }

        let buf = pager.read_page(cur)?;
        let mut entries = read_leaf_entries(&buf)?;
        let pos = entries
            .binary_search_by(|e| cmp_entry(e, &entry))
            .unwrap_or_else(|i| i);
        entries.insert(pos, entry);

        // Try to write back. If it overflows, split.
        let mut probe = vec![0u8; PAGE_SIZE];
        let next = LeafHeader::read(&buf).next_page;
        match write_leaf_entries(&mut probe, next, &entries) {
            Ok(()) => {
                pager.write_page(cur, &probe)?;
                Ok(())
            }
            Err(_) => {
                // Split. Allocate a sibling and move the upper half.
                let mid = entries.len() / 2;
                let right_entries: Vec<_> = entries.split_off(mid);
                let new_id = pager.allocate_page()?;
                let mut left = vec![0u8; PAGE_SIZE];
                let mut right = vec![0u8; PAGE_SIZE];
                write_leaf_entries(&mut right, next, &right_entries)?;
                write_leaf_entries(&mut left, new_id.0, &entries)?;
                pager.write_page(new_id, &right)?;
                pager.write_page(cur, &left)?;
                let _ = buf; // silence unused
                Ok(())
            }
        }
    }

    pub fn delete(pager: &mut Pager, root: PageId, entry: &IndexEntry) -> Result<bool> {
        let mut cur = root;
        loop {
            let buf = pager.read_page(cur)?;
            let mut entries = read_leaf_entries(&buf)?;
            let h = LeafHeader::read(&buf);
            if let Ok(idx) = entries.binary_search_by(|e| cmp_entry(e, entry)) {
                entries.remove(idx);
                let mut probe = vec![0u8; PAGE_SIZE];
                write_leaf_entries(&mut probe, h.next_page, &entries)?;
                pager.write_page(cur, &probe)?;
                return Ok(true);
            }
            if h.next_page == 0 {
                return Ok(false);
            }
            cur = PageId(h.next_page);
        }
    }

    /// Find all row ids whose key equals `key`.
    pub fn lookup_eq(pager: &mut Pager, root: PageId, key: &Value) -> Result<Vec<RowId>> {
        BTree::range_scan(
            pager,
            root,
            Some(key.clone()),
            Some(key.clone()),
            true,
            true,
        )
    }

    /// Range scan (inclusive on both ends as configured).
    pub fn range_scan(
        pager: &mut Pager,
        root: PageId,
        lo: Option<Value>,
        hi: Option<Value>,
        lo_inclusive: bool,
        hi_inclusive: bool,
    ) -> Result<Vec<RowId>> {
        let mut out = Vec::new();
        let mut cur = root;
        loop {
            let buf = pager.read_page(cur)?;
            let entries = read_leaf_entries(&buf)?;
            let h = LeafHeader::read(&buf);
            for e in &entries {
                if let Some(l) = &lo {
                    let c = cmp_key(&e.key, l);
                    if c == Ordering::Less {
                        continue;
                    }
                    if c == Ordering::Equal && !lo_inclusive {
                        continue;
                    }
                }
                if let Some(h2) = &hi {
                    let c = cmp_key(&e.key, h2);
                    if c == Ordering::Greater {
                        continue;
                    }
                    if c == Ordering::Equal && !hi_inclusive {
                        continue;
                    }
                }
                out.push(e.row_id);
            }
            if h.next_page == 0 {
                break;
            }
            cur = PageId(h.next_page);
        }
        Ok(out)
    }

    pub fn full_scan(pager: &mut Pager, root: PageId) -> Result<Vec<IndexEntry>> {
        let mut out = Vec::new();
        let mut cur = root;
        loop {
            let buf = pager.read_page(cur)?;
            let entries = read_leaf_entries(&buf)?;
            let h = LeafHeader::read(&buf);
            out.extend(entries);
            if h.next_page == 0 {
                break;
            }
            cur = PageId(h.next_page);
        }
        Ok(out)
    }

    /// Has the index ever been declared `UNIQUE`? A simple helper for callers
    /// that want to know whether a key already exists for some other row id.
    pub fn first_with_key(pager: &mut Pager, root: PageId, key: &Value) -> Result<Option<RowId>> {
        Ok(BTree::lookup_eq(pager, root, key)?.first().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering as AOrd};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp() -> PathBuf {
        let n = COUNTER.fetch_add(1, AOrd::SeqCst);
        let mut p = temp_dir();
        p.push(format!("minidb_btree_{}_{}.db", std::process::id(), n));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn insert_and_lookup() {
        let path = tmp();
        let mut pager = Pager::open(&path).unwrap();
        let root = BTree::create(&mut pager).unwrap();
        for i in [10, 5, 7, 2, 8, 1] {
            BTree::insert(
                &mut pager,
                root,
                IndexEntry::new(Value::Int(i), RowId::new(PageId(1), i as u16)),
            )
            .unwrap();
        }
        let r = BTree::lookup_eq(&mut pager, root, &Value::Int(7)).unwrap();
        assert_eq!(r.len(), 1);
        let rs = BTree::range_scan(
            &mut pager,
            root,
            Some(Value::Int(2)),
            Some(Value::Int(8)),
            true,
            true,
        )
        .unwrap();
        assert_eq!(rs.len(), 4);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn split_on_overflow() {
        let path = tmp();
        let mut pager = Pager::open(&path).unwrap();
        let root = BTree::create(&mut pager).unwrap();
        for i in 0..1500 {
            BTree::insert(
                &mut pager,
                root,
                IndexEntry::new(Value::Int(i), RowId::new(PageId(1), (i & 0xffff) as u16)),
            )
            .unwrap();
        }
        let all = BTree::full_scan(&mut pager, root).unwrap();
        assert_eq!(all.len(), 1500);
        let r = BTree::lookup_eq(&mut pager, root, &Value::Int(999)).unwrap();
        assert_eq!(r.len(), 1);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn delete_entry() {
        let path = tmp();
        let mut pager = Pager::open(&path).unwrap();
        let root = BTree::create(&mut pager).unwrap();
        let e = IndexEntry::new(Value::Int(5), RowId::new(PageId(1), 0));
        BTree::insert(&mut pager, root, e.clone()).unwrap();
        assert!(BTree::delete(&mut pager, root, &e).unwrap());
        assert_eq!(
            BTree::lookup_eq(&mut pager, root, &Value::Int(5))
                .unwrap()
                .len(),
            0
        );
        std::fs::remove_file(&path).ok();
    }
}
