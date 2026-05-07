//! In-memory schema catalog. Tables and indexes are described here; the
//! catalog is persisted as a small binary blob in a sidecar file alongside
//! the database file (`<db>.catalog`).

use crate::error::{MiniDbError, Result};
use crate::parser::ast::Expr;
use crate::storage::page::PageId;
use crate::types::DataType;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ColumnSchema {
    pub name: String,
    pub data_type: DataType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default_blob: Option<Vec<u8>>, // serialised expression, treated opaque here
    pub default_expr: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnSchema>,
    pub first_page: Option<PageId>,
    pub checks_blob: Vec<Vec<u8>>, // serialised CHECK expressions
    pub checks: Vec<Expr>,
}

impl TableSchema {
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }
    pub fn data_types(&self) -> Vec<DataType> {
        self.columns.iter().map(|c| c.data_type).collect()
    }
}

#[derive(Debug, Clone)]
pub struct IndexSchema {
    pub name: String,
    pub table: String,
    pub column: String,
}

#[derive(Debug, Default, Clone)]
pub struct Catalog {
    pub tables: HashMap<String, TableSchema>,
    pub indexes: HashMap<String, IndexSchema>,
}

impl Catalog {
    pub fn new() -> Self {
        Catalog::default()
    }

    pub fn add_table(&mut self, t: TableSchema) -> Result<()> {
        if self.tables.contains_key(&t.name) {
            return Err(MiniDbError::AlreadyExists(format!("table {}", t.name)));
        }
        self.tables.insert(t.name.clone(), t);
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        if self.tables.remove(name).is_none() {
            return Err(MiniDbError::NoSuchTable(name.into()));
        }
        // Drop dependent indexes
        let drops: Vec<_> = self
            .indexes
            .iter()
            .filter_map(|(k, v)| {
                if v.table == name {
                    Some(k.clone())
                } else {
                    None
                }
            })
            .collect();
        for k in drops {
            self.indexes.remove(&k);
        }
        Ok(())
    }

    pub fn get_table(&self, name: &str) -> Result<&TableSchema> {
        self.tables
            .get(name)
            .ok_or_else(|| MiniDbError::NoSuchTable(name.into()))
    }

    pub fn get_table_mut(&mut self, name: &str) -> Result<&mut TableSchema> {
        self.tables
            .get_mut(name)
            .ok_or_else(|| MiniDbError::NoSuchTable(name.into()))
    }

    pub fn add_index(&mut self, i: IndexSchema) -> Result<()> {
        if self.indexes.contains_key(&i.name) {
            return Err(MiniDbError::AlreadyExists(format!("index {}", i.name)));
        }
        if !self.tables.contains_key(&i.table) {
            return Err(MiniDbError::NoSuchTable(i.table.clone()));
        }
        let t = self.tables.get(&i.table).unwrap();
        if t.column_index(&i.column).is_none() {
            return Err(MiniDbError::NoSuchColumn(i.column.clone()));
        }
        self.indexes.insert(i.name.clone(), i);
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<IndexSchema> {
        self.indexes
            .remove(name)
            .ok_or_else(|| MiniDbError::NoSuchIndex(name.into()))
    }

    pub fn indexes_for(&self, table: &str) -> Vec<&IndexSchema> {
        self.indexes.values().filter(|i| i.table == table).collect()
    }
}

/// Sidecar file path for catalog persistence.
pub fn catalog_path(db_path: &Path) -> PathBuf {
    let mut p = db_path.to_path_buf();
    let stem = p
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "minidb".to_string());
    p.set_file_name(format!("{}.catalog", stem));
    p
}

pub fn save_catalog(path: &Path, cat: &Catalog) -> Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    f.write_all(&serialize(cat))?;
    f.sync_all()?;
    Ok(())
}

pub fn load_catalog(path: &Path) -> Result<Catalog> {
    if !path.exists() {
        return Ok(Catalog::new());
    }
    let mut buf = Vec::new();
    OpenOptions::new()
        .read(true)
        .open(path)?
        .read_to_end(&mut buf)?;
    if buf.is_empty() {
        return Ok(Catalog::new());
    }
    deserialize(&buf)
}

fn serialize(c: &Catalog) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(c.tables.len() as u32).to_le_bytes());
    for t in c.tables.values() {
        write_str(&mut out, &t.name);
        out.extend_from_slice(&(t.columns.len() as u32).to_le_bytes());
        for col in &t.columns {
            write_str(&mut out, &col.name);
            out.push(match col.data_type {
                DataType::Int => 1,
                DataType::Real => 2,
                DataType::Text => 3,
                DataType::Bool => 4,
            });
            let flags =
                (col.primary_key as u8) | ((col.not_null as u8) << 1) | ((col.unique as u8) << 2);
            out.push(flags);
            match &col.default_blob {
                None => {
                    out.push(0);
                }
                Some(b) => {
                    out.push(1);
                    write_blob(&mut out, b);
                }
            }
        }
        match t.first_page {
            None => out.push(0),
            Some(p) => {
                out.push(1);
                out.extend_from_slice(&p.raw().to_le_bytes());
            }
        }
        out.extend_from_slice(&(t.checks_blob.len() as u32).to_le_bytes());
        for c in &t.checks_blob {
            write_blob(&mut out, c);
        }
    }
    out.extend_from_slice(&(c.indexes.len() as u32).to_le_bytes());
    for i in c.indexes.values() {
        write_str(&mut out, &i.name);
        write_str(&mut out, &i.table);
        write_str(&mut out, &i.column);
    }
    out
}

fn deserialize(buf: &[u8]) -> Result<Catalog> {
    let mut p = 0;
    let n_tables = read_u32(buf, &mut p)? as usize;
    let mut cat = Catalog::new();
    for _ in 0..n_tables {
        let name = read_str(buf, &mut p)?;
        let n_cols = read_u32(buf, &mut p)? as usize;
        let mut cols = Vec::with_capacity(n_cols);
        for _ in 0..n_cols {
            let cname = read_str(buf, &mut p)?;
            if p >= buf.len() {
                return Err(MiniDbError::Catalog("truncated catalog".into()));
            }
            let dt = match buf[p] {
                1 => DataType::Int,
                2 => DataType::Real,
                3 => DataType::Text,
                4 => DataType::Bool,
                _ => return Err(MiniDbError::Catalog("bad type tag".into())),
            };
            p += 1;
            let flags = buf[p];
            p += 1;
            let primary_key = flags & 1 != 0;
            let not_null = flags & 2 != 0;
            let unique = flags & 4 != 0;
            let has_default = buf[p] != 0;
            p += 1;
            let default_blob = if has_default {
                Some(read_blob(buf, &mut p)?)
            } else {
                None
            };
            cols.push(ColumnSchema {
                name: cname,
                data_type: dt,
                primary_key,
                not_null,
                unique,
                default_blob,
                default_expr: None,
            });
        }
        let has_first = buf[p] != 0;
        p += 1;
        let first_page = if has_first {
            let mut a = [0u8; 8];
            a.copy_from_slice(&buf[p..p + 8]);
            p += 8;
            Some(PageId(u64::from_le_bytes(a)))
        } else {
            None
        };
        let n_checks = read_u32(buf, &mut p)? as usize;
        let mut checks_blob = Vec::with_capacity(n_checks);
        for _ in 0..n_checks {
            checks_blob.push(read_blob(buf, &mut p)?);
        }
        cat.tables.insert(
            name.clone(),
            TableSchema {
                name,
                columns: cols,
                first_page,
                checks_blob,
                checks: Vec::new(),
            },
        );
    }
    let n_idx = read_u32(buf, &mut p)? as usize;
    for _ in 0..n_idx {
        let name = read_str(buf, &mut p)?;
        let table = read_str(buf, &mut p)?;
        let column = read_str(buf, &mut p)?;
        cat.indexes.insert(
            name.clone(),
            IndexSchema {
                name,
                table,
                column,
            },
        );
    }
    Ok(cat)
}

fn write_str(out: &mut Vec<u8>, s: &str) {
    write_blob(out, s.as_bytes());
}
fn write_blob(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}
fn read_u32(buf: &[u8], p: &mut usize) -> Result<u32> {
    if *p + 4 > buf.len() {
        return Err(MiniDbError::Catalog("truncated u32".into()));
    }
    let mut a = [0u8; 4];
    a.copy_from_slice(&buf[*p..*p + 4]);
    *p += 4;
    Ok(u32::from_le_bytes(a))
}
fn read_blob(buf: &[u8], p: &mut usize) -> Result<Vec<u8>> {
    let n = read_u32(buf, p)? as usize;
    if *p + n > buf.len() {
        return Err(MiniDbError::Catalog("truncated blob".into()));
    }
    let v = buf[*p..*p + n].to_vec();
    *p += n;
    Ok(v)
}
fn read_str(buf: &[u8], p: &mut usize) -> Result<String> {
    let b = read_blob(buf, p)?;
    String::from_utf8(b).map_err(|_| MiniDbError::Catalog("invalid utf8".into()))
}
