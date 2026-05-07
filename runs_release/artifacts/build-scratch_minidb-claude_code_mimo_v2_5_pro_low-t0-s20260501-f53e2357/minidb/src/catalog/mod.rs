use std::collections::HashMap;

use crate::error::{MiniDbError, Result};
use crate::storage::btree::BTreeIndex;
use crate::storage::heap::HeapFile;
use crate::storage::page::{PageId, PageManager, PAGE_SIZE};
use crate::types::{ColumnDef, ColumnType, Value};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableMeta {
    pub id: u64,
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub check_constraints: Vec<String>, // serialized expressions
    pub heap_page: PageId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    pub id: u64,
    pub name: String,
    pub table_id: u64,
    pub column_name: String,
    pub column_index: usize,
    pub root_page: PageId,
}

const CATALOG_PAGE: PageId = PageId(0);

pub struct Catalog {
    pub tables: HashMap<String, TableMeta>,
    pub indexes: HashMap<String, IndexMeta>,
    next_table_id: u64,
    next_index_id: u64,
    dirty: bool,
}

impl Catalog {
    pub fn load(pm: &mut PageManager) -> Result<Self> {
        if pm.page_count() == 0 {
            // Fresh database - allocate catalog page
            let mut page = [0u8; PAGE_SIZE];
            page[0] = 0; // page_type = catalog
            page[1..9].copy_from_slice(&1u64.to_le_bytes()); // next_table_id
            page[9..17].copy_from_slice(&1u64.to_le_bytes()); // next_index_id
            let len_bytes = (0u32).to_le_bytes();
            page[17..21].copy_from_slice(&len_bytes);
            pm.allocate_page()?;
            pm.write_page(CATALOG_PAGE, &page)?;

            return Ok(Self {
                tables: HashMap::new(),
                indexes: HashMap::new(),
                next_table_id: 1,
                next_index_id: 1,
                dirty: false,
            });
        }

        let page = pm.read_page(CATALOG_PAGE)?;
        let next_table_id = u64::from_le_bytes(page[1..9].try_into().unwrap());
        let next_index_id = u64::from_le_bytes(page[9..17].try_into().unwrap());
        let data_len = u32::from_le_bytes(page[17..21].try_into().unwrap()) as usize;

        let (tables, indexes) = if data_len > 0 {
            let json = std::str::from_utf8(&page[21..21 + data_len]).unwrap_or("{}");
            serde_json::from_str::<(HashMap<String, TableMeta>, HashMap<String, IndexMeta>)>(json)
                .unwrap_or_default()
        } else {
            (HashMap::new(), HashMap::new())
        };

        Ok(Self {
            tables,
            indexes,
            next_table_id,
            next_index_id,
            dirty: false,
        })
    }

    pub fn save(&mut self, pm: &mut PageManager) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }

        let json = serde_json::to_string(&(&self.tables, &self.indexes))?;
        let json_bytes = json.as_bytes();

        let mut page = [0u8; PAGE_SIZE];
        page[0] = 0;
        page[1..9].copy_from_slice(&self.next_table_id.to_le_bytes());
        page[9..17].copy_from_slice(&self.next_index_id.to_le_bytes());
        page[17..21].copy_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        page[21..21 + json_bytes.len()].copy_from_slice(json_bytes);

        pm.write_page(CATALOG_PAGE, &page)?;
        pm.flush()?;
        self.dirty = false;
        Ok(())
    }

    pub fn create_table(
        &mut self,
        pm: &mut PageManager,
        name: String,
        columns: Vec<ColumnDef>,
        check_constraints: Vec<String>,
    ) -> Result<()> {
        if self.tables.contains_key(&name) {
            return Err(MiniDbError::Catalog {
                message: format!("table '{}' already exists", name),
            });
        }

        let table_id = self.next_table_id;
        self.next_table_id += 1;

        let heap_page = pm.allocate_page()?;
        let mut page = [0u8; PAGE_SIZE];
        HeapFile::init_page(&mut page);
        pm.write_page(heap_page, &page)?;

        let meta = TableMeta {
            id: table_id,
            name: name.clone(),
            columns,
            check_constraints,
            heap_page,
        };
        self.tables.insert(name, meta);
        self.dirty = true;
        Ok(())
    }

    pub fn drop_table(&mut self, pm: &mut PageManager, name: &str) -> Result<()> {
        if !self.tables.contains_key(name) {
            return Err(MiniDbError::Catalog {
                message: format!("table '{}' does not exist", name),
            });
        }

        let table_id = self.tables[name].id;

        // Remove associated indexes
        let idx_names: Vec<String> = self
            .indexes
            .values()
            .filter(|idx| idx.table_id == table_id)
            .map(|idx| idx.name.clone())
            .collect();
        for idx_name in idx_names {
            self.indexes.remove(&idx_name);
        }

        self.tables.remove(name);
        self.dirty = true;
        Ok(())
    }

    pub fn create_index(
        &mut self,
        pm: &mut PageManager,
        index_name: String,
        table_name: &str,
        column_name: &str,
    ) -> Result<()> {
        if self.indexes.contains_key(&index_name) {
            return Err(MiniDbError::Catalog {
                message: format!("index '{}' already exists", index_name),
            });
        }

        let table = self
            .tables
            .get(table_name)
            .ok_or_else(|| MiniDbError::Catalog {
                message: format!("table '{}' does not exist", table_name),
            })?;

        let col_idx = table
            .columns
            .iter()
            .position(|c| c.name == column_name)
            .ok_or_else(|| MiniDbError::Catalog {
                message: format!(
                    "column '{}' does not exist in table '{}'",
                    column_name, table_name
                ),
            })?;

        let index_id = self.next_index_id;
        self.next_index_id += 1;

        let root_page = pm.allocate_page()?;
        let mut page = [0u8; PAGE_SIZE];
        BTreeIndex::init_root(&mut page);
        pm.write_page(root_page, &page)?;

        // Build index from existing data
        let heap = HeapFile::new(table.id, table.heap_page);
        let rows = heap.scan_all(pm)?;
        let bt = BTreeIndex::new(index_id, root_page);
        for (pid, slot, row) in &rows {
            if col_idx < row.len() {
                bt.insert(pm, &row[col_idx], *pid, *slot as u32)?;
            }
        }

        let meta = IndexMeta {
            id: index_id,
            name: index_name.clone(),
            table_id: table.id,
            column_name: column_name.to_string(),
            column_index: col_idx,
            root_page,
        };
        self.indexes.insert(index_name, meta);
        self.dirty = true;
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<()> {
        if !self.indexes.contains_key(name) {
            return Err(MiniDbError::Catalog {
                message: format!("index '{}' does not exist", name),
            });
        }
        self.indexes.remove(name);
        self.dirty = true;
        Ok(())
    }

    pub fn get_table(&self, name: &str) -> Result<&TableMeta> {
        self.tables.get(name).ok_or_else(|| MiniDbError::Catalog {
            message: format!("table '{}' does not exist", name),
        })
    }

    pub fn get_table_mut(&mut self, name: &str) -> Result<&mut TableMeta> {
        self.tables
            .get_mut(name)
            .ok_or_else(|| MiniDbError::Catalog {
                message: format!("table '{}' does not exist", name),
            })
    }

    pub fn get_index_for_column(&self, table_name: &str, column_name: &str) -> Option<&IndexMeta> {
        let table = self.tables.get(table_name)?;
        self.indexes
            .values()
            .find(|idx| idx.table_id == table.id && idx.column_name == column_name)
    }

    pub fn validate_row(&self, table: &TableMeta, row: &[Value], is_update: bool) -> Result<()> {
        // Check NOT NULL
        for (i, col) in table.columns.iter().enumerate() {
            let val = row.get(i).unwrap_or(&Value::Null);
            if col.not_null && val.is_null() {
                return Err(MiniDbError::Constraint {
                    message: format!("column '{}' cannot be NULL", col.name),
                });
            }
            if col.primary_key && val.is_null() {
                return Err(MiniDbError::Constraint {
                    message: format!("primary key column '{}' cannot be NULL", col.name),
                });
            }
            // Type check
            if !val.is_null() && !val.is_compatible_with(col.col_type) {
                return Err(MiniDbError::Type {
                    message: format!(
                        "type mismatch for column '{}': expected {}, got {}",
                        col.name,
                        col.col_type,
                        val.type_name()
                    ),
                });
            }
        }
        Ok(())
    }

    pub fn apply_defaults(&self, table: &TableMeta, row: &mut Vec<Value>) {
        while row.len() < table.columns.len() {
            let col = &table.columns[row.len()];
            if let Some(ref default) = col.default {
                row.push(default.clone());
            } else {
                row.push(Value::Null);
            }
        }
    }

    pub fn check_unique_constraints(
        &self,
        pm: &mut PageManager,
        table: &TableMeta,
        row: &[Value],
        exclude_page: Option<PageId>,
        exclude_slot: Option<usize>,
    ) -> Result<()> {
        let heap = HeapFile::new(table.id, table.heap_page);
        let existing = heap.scan_all(pm)?;

        for (i, col) in table.columns.iter().enumerate() {
            if (col.primary_key || col.unique) && i < row.len() && !row[i].is_null() {
                for (pid, slot, erow) in &existing {
                    if Some(*pid) == exclude_page && Some(*slot) == exclude_slot {
                        continue;
                    }
                    if i < erow.len() && erow[i] == row[i] {
                        return Err(MiniDbError::Constraint {
                            message: format!(
                                "unique constraint violation: column '{}' value '{}' already exists",
                                col.name, row[i]
                            ),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup() -> (PageManager, Catalog) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut pm = PageManager::open(&path).unwrap();
        let catalog = Catalog::load(&mut pm).unwrap();
        (pm, catalog)
    }

    #[test]
    fn test_create_and_get_table() {
        let (mut pm, mut cat) = setup();
        let cols = vec![
            ColumnDef::new("id", ColumnType::Int),
            ColumnDef::new("name", ColumnType::Text),
        ];
        cat.create_table(&mut pm, "users".into(), cols, vec![])
            .unwrap();
        assert!(cat.tables.contains_key("users"));
        let t = cat.get_table("users").unwrap();
        assert_eq!(t.columns.len(), 2);
    }

    #[test]
    fn test_drop_table() {
        let (mut pm, mut cat) = setup();
        let cols = vec![ColumnDef::new("id", ColumnType::Int)];
        cat.create_table(&mut pm, "t".into(), cols, vec![]).unwrap();
        cat.drop_table(&mut pm, "t").unwrap();
        assert!(!cat.tables.contains_key("t"));
    }

    #[test]
    fn test_create_index() {
        let (mut pm, mut cat) = setup();
        let cols = vec![
            ColumnDef::new("id", ColumnType::Int),
            ColumnDef::new("name", ColumnType::Text),
        ];
        cat.create_table(&mut pm, "users".into(), cols, vec![])
            .unwrap();
        cat.create_index(&mut pm, "idx_name".into(), "users", "name")
            .unwrap();
        assert!(cat.indexes.contains_key("idx_name"));
    }

    #[test]
    fn test_unique_constraint() {
        let (mut pm, mut cat) = setup();
        let mut col = ColumnDef::new("email", ColumnType::Text);
        col.unique = true;
        cat.create_table(&mut pm, "users".into(), vec![col], vec![])
            .unwrap();

        let heap = HeapFile::new(1, cat.get_table("users").unwrap().heap_page);
        heap.insert_row(&mut pm, &[Value::Text("test@example.com".into())])
            .unwrap();

        let table = cat.get_table("users").unwrap();
        let result = cat.check_unique_constraints(
            &mut pm,
            table,
            &[Value::Text("test@example.com".into())],
            None,
            None,
        );
        assert!(result.is_err());
    }
}
