use crate::error::{MiniDbError, Result};
use crate::storage::page::PageData;
use crate::storage::wal::{Wal, WalRecord};
use crate::storage::{PageId, Pager, PAGE_SIZE};
use crate::types::{Row, Value};
use log::debug;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub fn table_id_from_name(name: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    name.to_lowercase().hash(&mut h);
    h.finish()
}

pub struct HeapFile {
    pub table_id: u64,
    pub pages: Vec<PageId>,
    pager: Arc<Mutex<Pager>>,
    dirty: HashMap<PageId, PageData>,
}

impl HeapFile {
    pub fn new(table_id: u64, root_page: PageId, pager: Arc<Mutex<Pager>>) -> Self {
        Self {
            table_id,
            pages: vec![root_page],
            pager,
            dirty: HashMap::new(),
        }
    }

    pub fn from_pages(table_id: u64, pages: Vec<PageId>, pager: Arc<Mutex<Pager>>) -> Self {
        Self {
            table_id,
            pages,
            pager,
            dirty: HashMap::new(),
        }
    }

    pub fn load_page(&self, pid: PageId) -> Result<PageData> {
        if let Some(p) = self.dirty.get(&pid) {
            return Ok(p.clone());
        }
        let raw = self.pager.lock().unwrap().read_page(pid)?;
        Ok(PageData { page_id: pid, raw })
    }

    fn load_page_mut(&mut self, pid: PageId) -> Result<()> {
        if !self.dirty.contains_key(&pid) {
            let page = self.load_page(pid)?;
            self.dirty.insert(pid, page);
        }
        Ok(())
    }

    fn wal_page(&self, pid: PageId, page: &PageData, wal: &mut Wal, txn_id: u64) -> Result<()> {
        wal.append(&WalRecord::PageWrite {
            table_id: self.table_id,
            page_id: pid.0,
            data: page.raw,
        })
    }

    pub fn insert_row(&mut self, row: &Row, wal: &mut Wal, txn_id: u64) -> Result<()> {
        let encoded = encode_row(row);
        let table_id = self.table_id;
        // Try last page first
        let last_pid = *self.pages.last().unwrap();
        self.load_page_mut(last_pid)?;
        let page = self.dirty.get_mut(&last_pid).unwrap();
        if page.can_insert(encoded.len()) {
            page.insert_row_data(&encoded);
            wal.append(&WalRecord::PageWrite {
                table_id,
                page_id: last_pid.0,
                data: page.raw,
            })?;
            return Ok(());
        }
        // Need new page
        let new_pid = {
            let mut pager = self.pager.lock().unwrap();
            pager.alloc_page()
        };
        let mut new_page = PageData::new_heap(new_pid);
        new_page.insert_row_data(&encoded);
        wal.append(&WalRecord::PageWrite {
            table_id,
            page_id: new_pid.0,
            data: new_page.raw,
        })?;
        self.dirty.insert(new_pid, new_page);
        self.pages.push(new_pid);
        Ok(())
    }

    pub fn scan_rows(&self) -> Result<Vec<Row>> {
        let mut rows = Vec::new();
        for &pid in &self.pages {
            let page = self.load_page(pid)?;
            for (_slot, data) in page.iter_rows() {
                rows.push(decode_row(data)?);
            }
        }
        Ok(rows)
    }

    pub fn scan_rows_with_rids(&self) -> Result<Vec<(u64, Row)>> {
        let mut results = Vec::new();
        for &pid in &self.pages {
            let page = self.load_page(pid)?;
            for (slot, data) in page.iter_rows() {
                let rid = make_rid(pid.0, slot as u16);
                results.push((rid, decode_row(data)?));
            }
        }
        Ok(results)
    }

    pub fn delete_by_rid(&mut self, rid: u64, wal: &mut Wal, txn_id: u64) -> Result<()> {
        let (page_id_raw, slot) = split_rid(rid);
        let pid = PageId(page_id_raw);
        let table_id = self.table_id;
        self.load_page_mut(pid)?;
        let page = self.dirty.get_mut(&pid).unwrap();
        page.delete_slot(slot as usize);
        wal.append(&WalRecord::PageWrite {
            table_id,
            page_id: pid.0,
            data: page.raw,
        })?;
        Ok(())
    }

    pub fn update_by_rid(
        &mut self,
        rid: u64,
        new_row: &Row,
        wal: &mut Wal,
        txn_id: u64,
    ) -> Result<()> {
        let (page_id_raw, slot) = split_rid(rid);
        let pid = PageId(page_id_raw);
        let table_id = self.table_id;
        self.load_page_mut(pid)?;
        let encoded = encode_row(new_row);
        let page = self.dirty.get_mut(&pid).unwrap();
        if !page.update_slot_data(slot as usize, &encoded) {
            return Err(MiniDbError::Storage("failed to update row in place".into()));
        }
        wal.append(&WalRecord::PageWrite {
            table_id,
            page_id: pid.0,
            data: page.raw,
        })?;
        Ok(())
    }

    pub fn flush_dirty(&mut self) -> Result<()> {
        let pager = self.pager.lock().unwrap();
        for (pid, page) in &self.dirty {
            pager.write_page(*pid, &page.raw)?;
        }
        Ok(())
    }

    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    pub fn discard_dirty(&mut self) {
        self.dirty.clear();
    }

    pub fn apply_wal_replay(&mut self, records: &[WalRecord]) -> Result<()> {
        for rec in records {
            if let WalRecord::PageWrite {
                table_id,
                page_id,
                data,
            } = rec
            {
                if *table_id == self.table_id {
                    let pid = PageId(*page_id);
                    let page = PageData {
                        page_id: pid,
                        raw: *data,
                    };
                    if !self.pages.contains(&pid) {
                        self.pages.push(pid);
                    }
                    // Write directly to disk
                    self.pager.lock().unwrap().write_page(pid, &page.raw)?;
                }
            }
        }
        Ok(())
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

pub fn encode_row(row: &Row) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&(row.len() as u32).to_le_bytes());
    for val in row {
        let vb = val.to_bytes();
        buf.extend_from_slice(&(vb.len() as u32).to_le_bytes());
        buf.extend_from_slice(&vb);
    }
    buf
}

pub fn decode_row(data: &[u8]) -> Result<Row> {
    if data.len() < 4 {
        return Err(MiniDbError::Storage("truncated row".into()));
    }
    let ncols = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let mut row = Vec::with_capacity(ncols);
    let mut pos = 4;
    for _ in 0..ncols {
        if pos + 4 > data.len() {
            return Err(MiniDbError::Storage("truncated row value length".into()));
        }
        let vlen = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        if pos + vlen > data.len() {
            return Err(MiniDbError::Storage("truncated row value".into()));
        }
        let (val, _) = Value::from_bytes(&data[pos..pos + vlen])?;
        row.push(val);
        pos += vlen;
    }
    Ok(row)
}

pub fn make_rid(page_id: u64, slot: u16) -> u64 {
    (page_id << 16) | (slot as u64)
}

pub fn split_rid(rid: u64) -> (u64, u16) {
    ((rid >> 16), (rid & 0xFFFF) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Value;

    #[test]
    fn test_encode_decode_row() {
        let row = vec![
            Value::Int(42),
            Value::Text("hello".into()),
            Value::Null,
            Value::Bool(true),
        ];
        let encoded = encode_row(&row);
        let decoded = decode_row(&encoded).unwrap();
        assert_eq!(row.len(), decoded.len());
        assert_eq!(row[0], decoded[0]);
        assert_eq!(row[1], decoded[1]);
        assert!(decoded[2].is_null());
        assert_eq!(row[3], decoded[3]);
    }

    #[test]
    fn test_rid_roundtrip() {
        let rid = make_rid(42, 7);
        let (pid, slot) = split_rid(rid);
        assert_eq!(pid, 42);
        assert_eq!(slot, 7);
    }
}
