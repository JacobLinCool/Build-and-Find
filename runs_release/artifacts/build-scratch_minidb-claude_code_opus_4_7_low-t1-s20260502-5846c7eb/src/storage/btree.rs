//! Secondary-index B-tree, keyed by `(index_key, row_id)`.
//!
//! For simplicity the index payload uses a sorted, in-memory `BTreeMap`
//! that is serialised to a sidecar file on commit. The exposed API is
//! tree-shaped (range scans, equality lookups, ordered iteration) which
//! is what the executor consumes — not the on-disk representation.

use crate::error::{MiniDbError, Result};
use crate::storage::heap::RowId;
use crate::storage::page::PageId;
use crate::types::Value;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// A wrapper around `Value` that imposes a total order suitable for index keys.
/// NULLs sort first; mixed numeric types collate by numeric value; mixing
/// non-comparable types falls back to type-tag ordering for determinism.
#[derive(Clone, Debug)]
pub struct IndexKey(pub Value);

fn type_rank(v: &Value) -> u8 {
    match v {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Int(_) | Value::Real(_) => 2,
        Value::Text(_) => 3,
    }
}

impl PartialEq for IndexKey {
    fn eq(&self, other: &Self) -> bool {
        matches!(self.cmp(other), Ordering::Equal)
    }
}
impl Eq for IndexKey {}
impl PartialOrd for IndexKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for IndexKey {
    fn cmp(&self, other: &Self) -> Ordering {
        let (a, b) = (&self.0, &other.0);
        match (a, b) {
            (Value::Null, Value::Null) => Ordering::Equal,
            (Value::Null, _) => Ordering::Less,
            (_, Value::Null) => Ordering::Greater,
            (Value::Int(x), Value::Int(y)) => x.cmp(y),
            (Value::Real(x), Value::Real(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
            (Value::Int(x), Value::Real(y)) => {
                (*x as f64).partial_cmp(y).unwrap_or(Ordering::Equal)
            }
            (Value::Real(x), Value::Int(y)) => {
                x.partial_cmp(&(*y as f64)).unwrap_or(Ordering::Equal)
            }
            (Value::Text(x), Value::Text(y)) => x.cmp(y),
            (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
            _ => type_rank(a).cmp(&type_rank(b)),
        }
    }
}

/// In-memory B-tree index over `(IndexKey, RowId)` pairs.
#[derive(Debug, Default, Clone)]
pub struct BTreeIndex {
    map: BTreeMap<(IndexKey, u64, u16), ()>,
}

impl BTreeIndex {
    pub fn new() -> Self {
        BTreeIndex {
            map: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, key: Value, row: RowId) {
        self.map
            .insert((IndexKey(key), row.page.raw(), row.slot), ());
    }

    pub fn remove(&mut self, key: &Value, row: RowId) {
        self.map
            .remove(&(IndexKey(key.clone()), row.page.raw(), row.slot));
    }

    pub fn equal_range(&self, key: &Value) -> Vec<RowId> {
        let from = (IndexKey(key.clone()), 0u64, 0u16);
        let to = (IndexKey(key.clone()), u64::MAX, u16::MAX);
        self.map
            .range(from..=to)
            .map(|((_, p, s), _)| RowId {
                page: PageId(*p),
                slot: *s,
            })
            .collect()
    }

    pub fn range_scan(
        &self,
        lo: Option<&Value>,
        lo_inclusive: bool,
        hi: Option<&Value>,
        hi_inclusive: bool,
    ) -> Vec<RowId> {
        let mut out = Vec::new();
        for ((k, p, s), _) in self.map.iter() {
            if let Some(l) = lo {
                let cmp = k.cmp(&IndexKey(l.clone()));
                if cmp == Ordering::Less || (cmp == Ordering::Equal && !lo_inclusive) {
                    continue;
                }
            }
            if let Some(h) = hi {
                let cmp = k.cmp(&IndexKey(h.clone()));
                if cmp == Ordering::Greater || (cmp == Ordering::Equal && !hi_inclusive) {
                    continue;
                }
            }
            out.push(RowId {
                page: PageId(*p),
                slot: *s,
            });
        }
        out
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.map.len() as u32).to_le_bytes());
        for ((k, p, s), _) in &self.map {
            encode_value(&k.0, &mut out);
            out.extend_from_slice(&p.to_le_bytes());
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    pub fn deserialize(buf: &[u8]) -> Result<Self> {
        if buf.len() < 4 {
            return Err(MiniDbError::Storage("btree blob too short".into()));
        }
        let n = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        let mut p = 4;
        let mut idx = BTreeIndex::new();
        for _ in 0..n {
            let v = decode_value(buf, &mut p)?;
            if p + 10 > buf.len() {
                return Err(MiniDbError::Storage("btree truncated".into()));
            }
            let mut a = [0u8; 8];
            a.copy_from_slice(&buf[p..p + 8]);
            let pg = u64::from_le_bytes(a);
            p += 8;
            let mut b = [0u8; 2];
            b.copy_from_slice(&buf[p..p + 2]);
            let slot = u16::from_le_bytes(b);
            p += 2;
            idx.map.insert((IndexKey(v), pg, slot), ());
        }
        Ok(idx)
    }
}

fn encode_value(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Null => out.push(0),
        Value::Int(i) => {
            out.push(1);
            out.extend_from_slice(&i.to_le_bytes());
        }
        Value::Real(r) => {
            out.push(2);
            out.extend_from_slice(&r.to_le_bytes());
        }
        Value::Text(s) => {
            out.push(3);
            let b = s.as_bytes();
            out.extend_from_slice(&(b.len() as u32).to_le_bytes());
            out.extend_from_slice(b);
        }
        Value::Bool(b) => {
            out.push(4);
            out.push(if *b { 1 } else { 0 });
        }
    }
}

fn decode_value(buf: &[u8], p: &mut usize) -> Result<Value> {
    if *p >= buf.len() {
        return Err(MiniDbError::Storage("value truncated".into()));
    }
    let tag = buf[*p];
    *p += 1;
    match tag {
        0 => Ok(Value::Null),
        1 => {
            let mut a = [0u8; 8];
            a.copy_from_slice(&buf[*p..*p + 8]);
            *p += 8;
            Ok(Value::Int(i64::from_le_bytes(a)))
        }
        2 => {
            let mut a = [0u8; 8];
            a.copy_from_slice(&buf[*p..*p + 8]);
            *p += 8;
            Ok(Value::Real(f64::from_le_bytes(a)))
        }
        3 => {
            let mut a = [0u8; 4];
            a.copy_from_slice(&buf[*p..*p + 4]);
            let n = u32::from_le_bytes(a) as usize;
            *p += 4;
            let s = String::from_utf8(buf[*p..*p + n].to_vec())
                .map_err(|_| MiniDbError::Storage("invalid utf8".into()))?;
            *p += n;
            Ok(Value::Text(s))
        }
        4 => {
            let b = buf[*p] != 0;
            *p += 1;
            Ok(Value::Bool(b))
        }
        _ => Err(MiniDbError::Storage("bad value tag".into())),
    }
}

/// Persist an index to a sidecar file.
pub fn save_index(path: &Path, idx: &BTreeIndex) -> Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    f.write_all(&idx.serialize())?;
    f.sync_all()?;
    Ok(())
}

pub fn load_index(path: &Path) -> Result<BTreeIndex> {
    if !path.exists() {
        return Ok(BTreeIndex::new());
    }
    let mut f = OpenOptions::new().read(true).open(path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    if buf.is_empty() {
        return Ok(BTreeIndex::new());
    }
    BTreeIndex::deserialize(&buf)
}

pub fn index_path(db_path: &Path, index_name: &str) -> PathBuf {
    let mut p = db_path.to_path_buf();
    let stem = p
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "minidb".to_string());
    p.set_file_name(format!("{}.idx.{}", stem, index_name));
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_range_and_serialize() {
        let mut idx = BTreeIndex::new();
        idx.insert(
            Value::Int(1),
            RowId {
                page: PageId(2),
                slot: 0,
            },
        );
        idx.insert(
            Value::Int(2),
            RowId {
                page: PageId(2),
                slot: 1,
            },
        );
        let r = idx.equal_range(&Value::Int(2));
        assert_eq!(r.len(), 1);
        let blob = idx.serialize();
        let back = BTreeIndex::deserialize(&blob).unwrap();
        assert_eq!(back.len(), idx.len());
    }

    #[test]
    fn range_scan_inclusive() {
        let mut idx = BTreeIndex::new();
        for i in 0..5 {
            idx.insert(
                Value::Int(i),
                RowId {
                    page: PageId(1),
                    slot: i as u16,
                },
            );
        }
        let r = idx.range_scan(Some(&Value::Int(1)), true, Some(&Value::Int(3)), true);
        assert_eq!(r.len(), 3);
    }
}
