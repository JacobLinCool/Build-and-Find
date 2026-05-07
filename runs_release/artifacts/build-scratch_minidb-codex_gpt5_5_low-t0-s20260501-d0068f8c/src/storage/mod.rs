pub mod btree;
pub mod heap;
pub mod wal;

use crate::{
    catalog::{Catalog, IndexSchema, TableSchema},
    error::{MiniDbError, Result},
    parser::ast::Statement,
    types::{Row, Value},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PageId(pub u64);

pub type RowId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredRow {
    pub id: RowId,
    pub values: Row,
    pub deleted: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TableData {
    pub next_row_id: RowId,
    pub rows: Vec<StoredRow>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatabaseImage {
    pub catalog: Catalog,
    pub tables: BTreeMap<String, TableData>,
}

pub struct StorageEngine {
    path: PathBuf,
    wal: wal::Wal,
    image: DatabaseImage,
}

impl StorageEngine {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let wal = wal::Wal::open(path.with_extension("wal"))?;
        let mut image = if path.exists() && fs::metadata(&path)?.len() > 0 {
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            DatabaseImage::default()
        };
        for tx in wal.replay()? {
            for op in tx {
                Self::apply_op(&mut image, op)?;
            }
        }
        let mut s = Self { path, wal, image };
        s.flush()?;
        Ok(s)
    }
    pub fn init(path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        fs::write(path, serde_json::to_vec_pretty(&DatabaseImage::default())?)?;
        let wal = path.with_extension("wal");
        if wal.exists() {
            fs::remove_file(wal)?;
        }
        Ok(())
    }
    pub fn catalog(&self) -> &Catalog {
        &self.image.catalog
    }
    pub fn image(&self) -> &DatabaseImage {
        &self.image
    }
    pub fn flush(&mut self) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.image)?;
        let pages = bytes.len().div_ceil(PAGE_SIZE);
        log::debug!(
            "flushing {} bytes across {} logical pages",
            bytes.len(),
            pages
        );
        fs::write(&self.path, bytes)?;
        self.wal.truncate()?;
        Ok(())
    }
    pub fn append_tx(&mut self, ops: &[wal::WalOp]) -> Result<()> {
        self.wal.begin()?;
        for op in ops {
            self.wal.op(op)?;
            Self::apply_op(&mut self.image, op.clone())?;
        }
        self.wal.commit()?;
        self.flush()
    }
    pub fn commit_applied_tx(&mut self, ops: &[wal::WalOp]) -> Result<()> {
        self.wal.begin()?;
        for op in ops {
            self.wal.op(op)?;
        }
        self.wal.commit()?;
        self.flush()
    }
    pub fn apply_uncommitted(&mut self, op: wal::WalOp) -> Result<()> {
        Self::apply_op(&mut self.image, op)
    }
    pub fn table_rows(&self, table: &str) -> Result<Vec<(RowId, Row)>> {
        let rows = self
            .image
            .tables
            .get(table)
            .ok_or_else(|| MiniDbError::Sql(format!("unknown table {table}")))?
            .rows
            .iter()
            .filter(|r| !r.deleted)
            .map(|r| (r.id, r.values.clone()))
            .collect();
        Ok(rows)
    }
    fn apply_op(image: &mut DatabaseImage, op: wal::WalOp) -> Result<()> {
        match op {
            wal::WalOp::CreateTable { schema } => {
                image.catalog.create_table(schema.clone())?;
                image.tables.insert(schema.name, TableData::default());
            }
            wal::WalOp::DropTable { name } => {
                image.catalog.drop_table(&name)?;
                image.tables.remove(&name);
            }
            wal::WalOp::CreateIndex { index } => {
                image.catalog.indexes.insert(index.name.clone(), index);
            }
            wal::WalOp::DropIndex { name } => {
                image.catalog.indexes.remove(&name);
            }
            wal::WalOp::Insert { table, row } => {
                let t = image
                    .tables
                    .get_mut(&table)
                    .ok_or_else(|| MiniDbError::Sql(format!("unknown table {table}")))?;
                let id = t.next_row_id;
                t.next_row_id += 1;
                t.rows.push(StoredRow {
                    id,
                    values: row,
                    deleted: false,
                });
            }
            wal::WalOp::Update { table, row_id, row } => {
                let t = image
                    .tables
                    .get_mut(&table)
                    .ok_or_else(|| MiniDbError::Sql(format!("unknown table {table}")))?;
                if let Some(r) = t.rows.iter_mut().find(|r| r.id == row_id && !r.deleted) {
                    r.values = row;
                }
            }
            wal::WalOp::Delete { table, row_id } => {
                let t = image
                    .tables
                    .get_mut(&table)
                    .ok_or_else(|| MiniDbError::Sql(format!("unknown table {table}")))?;
                if let Some(r) = t.rows.iter_mut().find(|r| r.id == row_id) {
                    r.deleted = true;
                }
            }
        }
        Ok(())
    }
}

pub fn schema_from_stmt(stmt: &Statement) -> Option<TableSchema> {
    if let Statement::CreateTable {
        name,
        columns,
        checks,
    } = stmt
    {
        Some(TableSchema {
            name: name.clone(),
            columns: columns.clone(),
            checks: checks.clone(),
        })
    } else {
        None
    }
}

pub fn index_schema(name: String, table: String, column: String) -> IndexSchema {
    IndexSchema {
        name,
        table,
        column,
    }
}

pub fn indexed_rows(image: &DatabaseImage, table: &str, column: &str) -> Result<btree::BTreeIndex> {
    let schema = image.catalog.table(table)?;
    let pos = schema
        .columns
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case(column))
        .ok_or_else(|| MiniDbError::Sql(format!("unknown column {column}")))?;
    let mut idx = btree::BTreeIndex::default();
    if let Some(data) = image.tables.get(table) {
        for row in data.rows.iter().filter(|r| !r.deleted) {
            idx.insert(
                row.values.get(pos).cloned().map_or(Value::Null, |v| v),
                row.id,
            );
        }
    }
    Ok(idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_size_constant() {
        assert_eq!(PAGE_SIZE, 4096);
        let _ = PageId(1);
    }
}
