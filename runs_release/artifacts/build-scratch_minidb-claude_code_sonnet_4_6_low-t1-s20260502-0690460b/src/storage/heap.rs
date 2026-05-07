/// Heap file storage: stores variable-length rows
/// Format: rows stored as (row_id: u64, len: u32, data: [u8; len], deleted: u8)
/// We keep a simple in-memory store backed by serialized append-only file.
use crate::error::{MiniDbError, Result};
use crate::types::{Row, Value};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

pub type RowId = u64;

/// A slot in the heap
#[derive(Debug, Clone)]
pub struct HeapSlot {
    pub row_id: RowId,
    pub data: Vec<u8>,
    pub deleted: bool,
}

/// Heap: manages rows for a single table
pub struct Heap {
    slots: BTreeMap<RowId, HeapSlot>,
    next_row_id: RowId,
}

impl Heap {
    pub fn new() -> Self {
        Heap {
            slots: BTreeMap::new(),
            next_row_id: 0,
        }
    }

    pub fn insert_row(&mut self, row: &Row) -> Result<RowId> {
        let row_id = self.next_row_id;
        self.next_row_id += 1;
        let data = serialize_row(row)?;
        self.slots.insert(
            row_id,
            HeapSlot {
                row_id,
                data,
                deleted: false,
            },
        );
        Ok(row_id)
    }

    pub fn insert_row_with_id(&mut self, row_id: RowId, data: Vec<u8>) -> Result<()> {
        if row_id >= self.next_row_id {
            self.next_row_id = row_id + 1;
        }
        self.slots.insert(
            row_id,
            HeapSlot {
                row_id,
                data,
                deleted: false,
            },
        );
        Ok(())
    }

    pub fn get_row(&self, row_id: RowId) -> Result<Option<Row>> {
        match self.slots.get(&row_id) {
            None => Ok(None),
            Some(slot) if slot.deleted => Ok(None),
            Some(slot) => Ok(Some(deserialize_row(&slot.data)?)),
        }
    }

    pub fn get_row_data(&self, row_id: RowId) -> Option<&[u8]> {
        self.slots
            .get(&row_id)
            .filter(|s| !s.deleted)
            .map(|s| s.data.as_slice())
    }

    pub fn update_row(&mut self, row_id: RowId, row: &Row) -> Result<()> {
        let data = serialize_row(row)?;
        let slot = self
            .slots
            .get_mut(&row_id)
            .ok_or_else(|| MiniDbError::Runtime(format!("row {} not found", row_id)))?;
        if slot.deleted {
            return Err(MiniDbError::Runtime(format!("row {} is deleted", row_id)));
        }
        slot.data = data;
        Ok(())
    }

    pub fn delete_row(&mut self, row_id: RowId) -> Result<()> {
        let slot = self
            .slots
            .get_mut(&row_id)
            .ok_or_else(|| MiniDbError::Runtime(format!("row {} not found", row_id)))?;
        slot.deleted = true;
        Ok(())
    }

    pub fn undo_insert(&mut self, row_id: RowId) {
        self.slots.remove(&row_id);
    }

    pub fn undo_delete(&mut self, row_id: RowId, data: Vec<u8>) {
        if let Some(slot) = self.slots.get_mut(&row_id) {
            slot.deleted = false;
            slot.data = data;
        }
    }

    pub fn undo_update(&mut self, row_id: RowId, old_data: Vec<u8>) {
        if let Some(slot) = self.slots.get_mut(&row_id) {
            slot.data = old_data;
        }
    }

    pub fn scan(&self) -> impl Iterator<Item = (RowId, Row)> + '_ {
        self.slots
            .values()
            .filter(|s| !s.deleted)
            .filter_map(|s| deserialize_row(&s.data).ok().map(|r| (s.row_id, r)))
    }

    pub fn scan_raw(&self) -> impl Iterator<Item = &HeapSlot> + '_ {
        self.slots.values().filter(|s| !s.deleted)
    }

    pub fn row_count(&self) -> usize {
        self.slots.values().filter(|s| !s.deleted).count()
    }

    /// Serialize heap to bytes for persistence
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.next_row_id.to_le_bytes());
        for slot in self.slots.values() {
            let deleted = if slot.deleted { 1u8 } else { 0u8 };
            out.extend_from_slice(&slot.row_id.to_le_bytes());
            out.extend_from_slice(&(slot.data.len() as u32).to_le_bytes());
            out.extend_from_slice(&slot.data);
            out.push(deleted);
        }
        out
    }

    /// Deserialize heap from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 8 {
            return Ok(Heap::new());
        }
        let next_row_id = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let mut pos = 8;
        let mut slots = BTreeMap::new();
        while pos + 13 <= data.len() {
            let row_id = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
            pos += 8;
            let len = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            if pos + len + 1 > data.len() {
                break;
            }
            let row_data = data[pos..pos + len].to_vec();
            pos += len;
            let deleted = data[pos] != 0;
            pos += 1;
            slots.insert(
                row_id,
                HeapSlot {
                    row_id,
                    data: row_data,
                    deleted,
                },
            );
        }
        Ok(Heap { slots, next_row_id })
    }
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

pub fn serialize_row(row: &Row) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(&(row.len() as u32).to_le_bytes());
    for val in row {
        let vbytes = val.to_bytes();
        out.extend_from_slice(&vbytes);
    }
    Ok(out)
}

pub fn deserialize_row(data: &[u8]) -> Result<Row> {
    if data.len() < 4 {
        return Err(MiniDbError::Storage("truncated row".into()));
    }
    let col_count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let mut pos = 4;
    let mut row = Vec::with_capacity(col_count);
    for _ in 0..col_count {
        let (val, consumed) = Value::from_bytes(&data[pos..])?;
        pos += consumed;
        row.push(val);
    }
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Value;

    #[test]
    fn test_heap_insert_get() {
        let mut heap = Heap::new();
        let row = vec![Value::Int(1), Value::Text("hello".into())];
        let id = heap.insert_row(&row).unwrap();
        let got = heap.get_row(id).unwrap().unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], Value::Int(1));
    }

    #[test]
    fn test_heap_delete() {
        let mut heap = Heap::new();
        let id = heap.insert_row(&vec![Value::Int(42)]).unwrap();
        heap.delete_row(id).unwrap();
        assert!(heap.get_row(id).unwrap().is_none());
    }

    #[test]
    fn test_heap_scan() {
        let mut heap = Heap::new();
        for i in 0..5 {
            heap.insert_row(&vec![Value::Int(i)]).unwrap();
        }
        let rows: Vec<_> = heap.scan().collect();
        assert_eq!(rows.len(), 5);
    }

    #[test]
    fn test_heap_serialization() {
        let mut heap = Heap::new();
        heap.insert_row(&vec![Value::Int(1), Value::Text("a".into())])
            .unwrap();
        heap.insert_row(&vec![Value::Int(2), Value::Text("b".into())])
            .unwrap();
        let bytes = heap.to_bytes();
        let heap2 = Heap::from_bytes(&bytes).unwrap();
        assert_eq!(heap2.row_count(), 2);
    }
}
