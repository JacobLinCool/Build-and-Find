use crate::error::{MiniDbError, Result};
use crate::storage::page::PageData;
use crate::storage::{PageId, Pager, PAGE_SIZE};
use crate::types::{IndexSchema, TableSchema};
use log::debug;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const CATALOG_MAGIC: u16 = 0x4341; // "CA"

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CatalogData {
    pub tables: HashMap<String, TableEntry>,
    pub indexes: HashMap<String, IndexEntry>,
    pub next_table_id: u64,
    pub next_index_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableEntry {
    pub schema: TableSchema,
    pub root_page: u64,
    pub table_id: u64,
    #[serde(default)]
    pub pages: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub schema: IndexSchema,
    pub root_page: u64,
    pub index_id: u64,
}

pub struct Catalog {
    pub data: CatalogData,
    pager: Arc<Mutex<Pager>>,
}

impl Catalog {
    pub fn load(pager: Arc<Mutex<Pager>>) -> Result<Self> {
        let page_count = pager.lock().unwrap().page_count();
        if page_count == 0 {
            // Fresh database - allocate catalog page
            let pid = pager.lock().unwrap().alloc_page();
            let data = CatalogData::default();
            let cat = Self { data, pager };
            cat.save()?;
            Ok(cat)
        } else {
            let raw = pager.lock().unwrap().read_page(PageId(0))?;
            let magic = u16::from_le_bytes(raw[0..2].try_into().unwrap());
            if magic != CATALOG_MAGIC {
                return Err(MiniDbError::Storage("invalid catalog page".into()));
            }
            let data_len = u32::from_le_bytes(raw[2..6].try_into().unwrap()) as usize;
            if data_len > PAGE_SIZE - 6 {
                return Err(MiniDbError::Storage("catalog data too large".into()));
            }
            let json = std::str::from_utf8(&raw[6..6 + data_len])
                .map_err(|_| MiniDbError::Storage("invalid catalog UTF-8".into()))?;
            let data: CatalogData = serde_json::from_str(json)
                .map_err(|e| MiniDbError::Storage(format!("catalog deserialization: {e}")))?;
            Ok(Self { data, pager })
        }
    }

    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string(&self.data)
            .map_err(|e| MiniDbError::Storage(format!("catalog serialization: {e}")))?;
        let json_bytes = json.as_bytes();
        if json_bytes.len() > PAGE_SIZE - 6 {
            return Err(MiniDbError::Storage("catalog too large for page".into()));
        }
        let mut raw = [0u8; PAGE_SIZE];
        raw[0..2].copy_from_slice(&CATALOG_MAGIC.to_le_bytes());
        raw[2..6].copy_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        raw[6..6 + json_bytes.len()].copy_from_slice(json_bytes);
        self.pager.lock().unwrap().write_page(PageId(0), &raw)?;
        Ok(())
    }

    pub fn create_table(&mut self, schema: TableSchema) -> Result<u64> {
        let name = schema.name.to_lowercase();
        if self.data.tables.contains_key(&name) {
            return Err(MiniDbError::TableExists(name));
        }
        // Check for duplicate columns
        let mut seen = std::collections::HashSet::new();
        for col in &schema.columns {
            let lc = col.name.to_lowercase();
            if !seen.insert(lc.clone()) {
                return Err(MiniDbError::DuplicateColumn(col.name.clone()));
            }
        }

        let table_id = self.data.next_table_id;
        self.data.next_table_id += 1;

        let root_page = {
            let mut pager = self.pager.lock().unwrap();
            let pid = pager.alloc_page();
            // Initialize root page
            let page = PageData::new_heap(pid);
            pager.write_page(pid, &page.raw)?;
            pid.0
        };

        self.data.tables.insert(
            name,
            TableEntry {
                schema,
                root_page,
                table_id,
                pages: vec![root_page],
            },
        );
        self.save()?;
        Ok(table_id)
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        let lc = name.to_lowercase();
        if self.data.tables.remove(&lc).is_none() {
            return Err(MiniDbError::TableNotFound(name.to_string()));
        }
        // Remove associated indexes
        let to_remove: Vec<String> = self
            .data
            .indexes
            .values()
            .filter(|idx| idx.schema.table_name.to_lowercase() == lc)
            .map(|idx| idx.schema.name.to_lowercase())
            .collect();
        for idx_name in to_remove {
            self.data.indexes.remove(&idx_name);
        }
        self.save()?;
        Ok(())
    }

    pub fn get_table(&self, name: &str) -> Option<&TableEntry> {
        self.data.tables.get(&name.to_lowercase())
    }

    pub fn create_index(
        &mut self,
        index_name: &str,
        table_name: &str,
        column_name: &str,
    ) -> Result<u64> {
        let idx_lc = index_name.to_lowercase();
        let tbl_lc = table_name.to_lowercase();
        let col_lc = column_name.to_lowercase();

        if self.data.indexes.contains_key(&idx_lc) {
            return Err(MiniDbError::IndexExists(index_name.to_string()));
        }
        let table = self
            .data
            .tables
            .get(&tbl_lc)
            .ok_or_else(|| MiniDbError::TableNotFound(table_name.to_string()))?;
        if !table
            .schema
            .columns
            .iter()
            .any(|c| c.name.to_lowercase() == col_lc)
        {
            return Err(MiniDbError::ColumnNotFound(column_name.to_string()));
        }

        let index_id = self.data.next_index_id;
        self.data.next_index_id += 1;

        let root_page = {
            let mut pager = self.pager.lock().unwrap();
            let pid = pager.alloc_page();
            // Initialize B-tree leaf page
            let mut raw = [0u8; PAGE_SIZE];
            raw[0..2].copy_from_slice(&0x424Cu16.to_le_bytes()); // LEAF_MAGIC
            pager.write_page(pid, &raw)?;
            pid.0
        };

        self.data.indexes.insert(
            idx_lc,
            IndexEntry {
                schema: IndexSchema {
                    name: index_name.to_string(),
                    table_name: table_name.to_string(),
                    column_name: column_name.to_string(),
                },
                root_page,
                index_id,
            },
        );
        self.save()?;
        Ok(index_id)
    }

    pub fn drop_index(&mut self, name: &str) -> Result<()> {
        let lc = name.to_lowercase();
        if self.data.indexes.remove(&lc).is_none() {
            return Err(MiniDbError::IndexNotFound(name.to_string()));
        }
        self.save()?;
        Ok(())
    }

    pub fn get_index(&self, name: &str) -> Option<&IndexEntry> {
        self.data.indexes.get(&name.to_lowercase())
    }

    pub fn get_index_for_column(&self, table_name: &str, column_name: &str) -> Option<&IndexEntry> {
        let tbl_lc = table_name.to_lowercase();
        let col_lc = column_name.to_lowercase();
        self.data.indexes.values().find(|idx| {
            idx.schema.table_name.to_lowercase() == tbl_lc
                && idx.schema.column_name.to_lowercase() == col_lc
        })
    }

    pub fn update_table_pages(&mut self, name: &str, pages: Vec<u64>) -> Result<()> {
        let lc = name.to_lowercase();
        if let Some(entry) = self.data.tables.get_mut(&lc) {
            entry.pages = pages;
            self.save()?;
        }
        Ok(())
    }

    pub fn table_names(&self) -> Vec<String> {
        self.data.tables.keys().cloned().collect()
    }

    pub fn index_entries(&self) -> Vec<&IndexEntry> {
        self.data.indexes.values().collect()
    }

    pub fn pager(&self) -> Arc<Mutex<Pager>> {
        self.pager.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ColumnDef, ColumnType};
    use std::fs;

    fn setup() -> (Arc<Mutex<Pager>>, std::path::PathBuf) {
        let path =
            std::env::temp_dir().join(format!("minidb_catalog_test_{}.db", std::process::id()));
        let _ = fs::remove_file(&path);
        let pager = Pager::open(&path).unwrap();
        (Arc::new(Mutex::new(pager)), path)
    }

    fn test_schema(name: &str) -> TableSchema {
        TableSchema {
            name: name.into(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    col_type: ColumnType::Int,
                    primary_key: true,
                    not_null: true,
                    unique: true,
                    default: None,
                },
                ColumnDef {
                    name: "name".into(),
                    col_type: ColumnType::Text,
                    primary_key: false,
                    not_null: false,
                    unique: false,
                    default: None,
                },
            ],
            check_expr: None,
        }
    }

    #[test]
    fn test_catalog_create_table() {
        let (pager, path) = setup();
        {
            let mut cat = Catalog::load(pager.clone()).unwrap();
            cat.create_table(test_schema("users")).unwrap();
            assert!(cat.get_table("users").is_some());
            assert!(cat.get_table("USERS").is_some()); // case insensitive
        }
        // Reload
        {
            let cat = Catalog::load(pager.clone()).unwrap();
            assert!(cat.get_table("users").is_some());
        }
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_catalog_duplicate_table() {
        let (pager, path) = setup();
        {
            let mut cat = Catalog::load(pager.clone()).unwrap();
            cat.create_table(test_schema("users")).unwrap();
            assert!(cat.create_table(test_schema("users")).is_err());
        }
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_catalog_create_drop_index() {
        let (pager, path) = setup();
        {
            let mut cat = Catalog::load(pager.clone()).unwrap();
            cat.create_table(test_schema("users")).unwrap();
            cat.create_index("idx_name", "users", "name").unwrap();
            assert!(cat.get_index("idx_name").is_some());
            cat.drop_index("idx_name").unwrap();
            assert!(cat.get_index("idx_name").is_none());
        }
        let _ = fs::remove_file(&path);
    }
}
