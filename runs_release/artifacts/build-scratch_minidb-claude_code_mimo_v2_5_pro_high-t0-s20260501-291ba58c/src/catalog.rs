use crate::error::{MiniDbError, Result};
use crate::storage::page::{PageId, PageManager, PAGE_SIZE};
use crate::types::{IndexSchema, TableSchema};
use std::collections::HashMap;

/// Catalog stores table and index metadata in a special system page (page 0).
/// Format: JSON serialized catalog data.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Catalog {
    tables: HashMap<String, TableSchema>,
    indexes: HashMap<String, IndexSchema>,
    table_pages: HashMap<String, Vec<PageId>>,
    index_roots: HashMap<String, PageId>,
    next_page: u64,
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            tables: HashMap::new(),
            indexes: HashMap::new(),
            table_pages: HashMap::new(),
            index_roots: HashMap::new(),
            next_page: 1, // page 0 is catalog
        }
    }
}

impl Catalog {
    pub const CATALOG_PAGE: PageId = PageId(0);

    pub fn load(pm: &mut PageManager) -> Result<Self> {
        if pm.page_count() == 0 {
            return Err(MiniDbError::Catalog("database file is empty".to_string()));
        }
        let page = pm.read_page(Self::CATALOG_PAGE)?;
        let data = &page[..];
        // Find null terminator
        let len = data.iter().position(|&b| b == 0).unwrap_or(PAGE_SIZE);
        if len == 0 {
            return Ok(Self::default());
        }
        let json = std::str::from_utf8(&data[..len])
            .map_err(|e| MiniDbError::Catalog(format!("invalid catalog encoding: {e}")))?;
        serde_json::from_str(json)
            .map_err(|e| MiniDbError::Catalog(format!("invalid catalog JSON: {e}")))
    }

    pub fn save(&self, pm: &mut PageManager) -> Result<()> {
        let json = serde_json::to_string(self)?;
        let bytes = json.as_bytes();
        if bytes.len() + 1 > PAGE_SIZE {
            return Err(MiniDbError::Catalog(
                "catalog too large for page".to_string(),
            ));
        }
        let mut page = [0u8; PAGE_SIZE];
        page[..bytes.len()].copy_from_slice(bytes);
        pm.write_page(Self::CATALOG_PAGE, &page)?;
        Ok(())
    }

    pub fn init(pm: &mut PageManager) -> Result<Self> {
        let catalog = Self::default();
        pm.alloc_page()?; // page 0 for catalog
        catalog.save(pm)?;
        Ok(catalog)
    }

    pub fn create_table(&mut self, schema: TableSchema) -> Result<()> {
        if self.tables.contains_key(&schema.name) {
            return Err(MiniDbError::Catalog(format!(
                "table '{}' already exists",
                schema.name
            )));
        }
        let name = schema.name.clone();
        self.tables.insert(name.clone(), schema);
        self.table_pages.insert(name, Vec::new());
        Ok(())
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        if !self.tables.contains_key(name) {
            return Err(MiniDbError::Catalog(format!(
                "table '{name}' does not exist"
            )));
        }
        self.tables.remove(name);
        self.table_pages.remove(name);
        // Remove indexes on this table
        let idx_names: Vec<String> = self
            .indexes
            .iter()
            .filter(|(_, idx)| idx.table == name)
            .map(|(n, _)| n.clone())
            .collect();
        for idx_name in idx_names {
            self.indexes.remove(&idx_name);
            self.index_roots.remove(&idx_name);
        }
        Ok(())
    }

    pub fn get_table(&self, name: &str) -> Option<&TableSchema> {
        self.tables.get(name)
    }

    pub fn table_names(&self) -> Vec<String> {
        self.tables.keys().cloned().collect()
    }

    pub fn get_table_pages(&self, name: &str) -> &[PageId] {
        self.table_pages
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn add_table_page(&mut self, name: &str, pid: PageId) {
        self.table_pages
            .entry(name.to_string())
            .or_default()
            .push(pid);
    }

    pub fn create_index(&mut self, schema: IndexSchema, root_page: PageId) -> Result<()> {
        if self.indexes.contains_key(&schema.name) {
            return Err(MiniDbError::Catalog(format!(
                "index '{}' already exists",
                schema.name
            )));
        }
        if !self.tables.contains_key(&schema.table) {
            return Err(MiniDbError::Catalog(format!(
                "table '{}' does not exist",
                schema.table
            )));
        }
        let name = schema.name.clone();
        self.indexes.insert(name.clone(), schema);
        self.index_roots.insert(name, root_page);
        Ok(())
    }

    pub fn drop_index(&mut self, name: &str) -> Result<()> {
        if !self.indexes.contains_key(name) {
            return Err(MiniDbError::Catalog(format!(
                "index '{name}' does not exist"
            )));
        }
        self.indexes.remove(name);
        self.index_roots.remove(name);
        Ok(())
    }

    pub fn get_index(&self, name: &str) -> Option<&IndexSchema> {
        self.indexes.get(name)
    }

    pub fn get_index_root(&self, name: &str) -> Option<PageId> {
        self.index_roots.get(name).copied()
    }

    pub fn index_names(&self) -> Vec<String> {
        self.indexes.keys().cloned().collect()
    }

    pub fn get_indexes_for_table(&self, table: &str) -> Vec<(&IndexSchema, PageId)> {
        self.indexes
            .values()
            .filter(|idx| idx.table == table)
            .filter_map(|idx| self.index_roots.get(&idx.name).map(|&pid| (idx, pid)))
            .collect()
    }

    pub fn alloc_page(&mut self) -> PageId {
        let pid = PageId(self.next_page);
        self.next_page += 1;
        pid
    }

    pub fn schema_string(&self) -> String {
        let mut result = String::new();
        for (_, table) in &self.tables {
            result.push_str(&format!("CREATE TABLE {} (", table.name));
            let col_strs: Vec<String> = table
                .columns
                .iter()
                .map(|c| {
                    let mut s = format!("{} {}", c.name, c.col_type);
                    if c.primary_key {
                        s.push_str(" PRIMARY KEY");
                    }
                    if c.not_null {
                        s.push_str(" NOT NULL");
                    }
                    if c.unique {
                        s.push_str(" UNIQUE");
                    }
                    if let Some(ref default) = c.default {
                        s.push_str(&format!(" DEFAULT {}", default));
                    }
                    s
                })
                .collect();
            result.push_str(&col_strs.join(", "));
            result.push_str(");\n");
        }
        for (_, idx) in &self.indexes {
            result.push_str(&format!(
                "CREATE INDEX {} ON {} ({});\n",
                idx.name, idx.table, idx.column
            ));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ColumnDef, ColumnType};
    use tempfile::tempdir;

    #[test]
    fn test_catalog_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut pm = PageManager::create(&path).unwrap();
        let mut cat = Catalog::init(&mut pm).unwrap();

        let schema = TableSchema {
            name: "users".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".to_string(),
                    col_type: ColumnType::Int,
                    primary_key: true,
                    not_null: true,
                    unique: true,
                    default: None,
                },
                ColumnDef {
                    name: "name".to_string(),
                    col_type: ColumnType::Text,
                    primary_key: false,
                    not_null: false,
                    unique: false,
                    default: None,
                },
            ],
            check_expr: None,
        };
        cat.create_table(schema).unwrap();
        cat.save(&mut pm).unwrap();

        let cat2 = Catalog::load(&mut pm).unwrap();
        assert!(cat2.get_table("users").is_some());
        assert_eq!(cat2.get_table("users").unwrap().columns.len(), 2);
    }
}
