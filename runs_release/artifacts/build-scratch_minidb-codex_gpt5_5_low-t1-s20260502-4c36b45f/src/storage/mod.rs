pub mod btree;
pub mod wal;

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    catalog::Catalog,
    error::{MiniDbError, Result},
    parser::ast::Statement,
    types::{Row, RowId, Value},
};

pub const PAGE_SIZE: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PageId(pub u64);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredRow {
    pub id: RowId,
    pub values: Row,
    pub deleted: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TableData {
    pub next_row_id: RowId,
    pub rows: Vec<StoredRow>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DatabaseImage {
    pub catalog: Catalog,
    pub tables: BTreeMap<String, TableData>,
    pub indexes: BTreeMap<String, btree::BTreeIndex>,
}

pub struct Storage {
    path: PathBuf,
    wal: wal::Wal,
    pub image: DatabaseImage,
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let wal = wal::Wal::open(path.with_extension("wal"))?;
        let mut image = if path.exists() {
            let bytes = fs::read(&path)?;
            if bytes.is_empty() {
                DatabaseImage::default()
            } else {
                serde_json::from_slice(&bytes).map_err(|e| MiniDbError::Storage(e.to_string()))?
            }
        } else {
            DatabaseImage::default()
        };
        let records = wal.read_records()?;
        for rec in records {
            match rec {
                wal::WalRecord::Begin(_) => {}
                wal::WalRecord::Commit(_) => {}
                wal::WalRecord::Rollback(_) => {}
                wal::WalRecord::Statement { statement, .. } => {
                    apply_statement_image(&mut image, &statement)?
                }
            }
        }
        let mut s = Self { path, wal, image };
        s.flush()?;
        s.wal.truncate()?;
        Ok(s)
    }

    pub fn init(path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        fs::write(
            path,
            serde_json::to_vec_pretty(&DatabaseImage::default())
                .map_err(|e| MiniDbError::Storage(e.to_string()))?,
        )?;
        let _ = fs::remove_file(path.with_extension("wal"));
        Ok(())
    }

    pub fn append_wal(&mut self, record: &wal::WalRecord) -> Result<()> {
        self.wal.append(record)
    }
    pub fn flush(&mut self) -> Result<()> {
        fs::write(
            &self.path,
            serde_json::to_vec_pretty(&self.image)
                .map_err(|e| MiniDbError::Storage(e.to_string()))?,
        )?;
        Ok(())
    }
    pub fn clear_wal(&mut self) -> Result<()> {
        self.wal.truncate()
    }

    pub fn rebuild_indexes(&mut self) -> Result<()> {
        self.image.indexes.clear();
        for meta in self.image.catalog.indexes.values() {
            let schema = self.image.catalog.table(&meta.table)?;
            let col = schema
                .columns
                .iter()
                .position(|c| c.name == meta.column)
                .ok_or_else(|| MiniDbError::Sql(format!("unknown column '{}'", meta.column)))?;
            let mut idx = btree::BTreeIndex::default();
            if let Some(t) = self.image.tables.get(&meta.table) {
                for r in t.rows.iter().filter(|r| !r.deleted) {
                    idx.insert(r.values.get(col).cloned().map_or(Value::Null, |v| v), r.id);
                }
            }
            self.image.indexes.insert(meta.name.clone(), idx);
        }
        Ok(())
    }
}

fn apply_statement_image(image: &mut DatabaseImage, stmt: &Statement) -> Result<()> {
    match stmt {
        Statement::CreateTable(schema) => {
            image.catalog.create_table(schema.clone())?;
            image
                .tables
                .entry(schema.name.to_ascii_lowercase())
                .or_default();
        }
        Statement::DropTable(name) => {
            image.catalog.drop_table(name)?;
            image.tables.remove(&name.to_ascii_lowercase());
        }
        Statement::CreateIndex {
            name,
            table,
            column,
        } => {
            image.catalog.indexes.insert(
                name.to_ascii_lowercase(),
                crate::catalog::IndexMeta {
                    name: name.to_ascii_lowercase(),
                    table: table.to_ascii_lowercase(),
                    column: column.to_ascii_lowercase(),
                },
            );
        }
        Statement::DropIndex(name) => {
            image.catalog.indexes.remove(&name.to_ascii_lowercase());
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_size_is_fixed() {
        assert_eq!(PAGE_SIZE, 4096);
        assert_eq!(PageId(1).0, 1);
    }
}
