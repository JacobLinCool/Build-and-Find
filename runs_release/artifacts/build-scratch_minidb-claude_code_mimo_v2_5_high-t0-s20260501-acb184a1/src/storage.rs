use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::error::{MiniDbError, Result};
use crate::types::{IndexDef, Row, RowId, TableSchema, Value};

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageId(pub u64);

/// Page data buffer
#[derive(Debug, Clone)]
pub struct Page {
    pub id: PageId,
    pub data: [u8; PAGE_SIZE],
    pub dirty: bool,
}

impl Page {
    pub fn new(id: PageId) -> Self {
        Page {
            id,
            data: [0; PAGE_SIZE],
            dirty: false,
        }
    }
}

/// Manages pages in a database file.
pub struct PageManager {
    file: File,
    pages: BTreeMap<PageId, Page>,
    next_page_id: u64,
    catalog_pages: Vec<PageId>,
}

impl PageManager {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let metadata = file.metadata()?;
        let file_len = metadata.len();

        let mut pm = PageManager {
            file,
            pages: BTreeMap::new(),
            next_page_id: 0,
            catalog_pages: vec![PageId(0)],
        };

        if file_len >= PAGE_SIZE as u64 {
            // Load page 0 (catalog page)
            pm.file.seek(SeekFrom::Start(0))?;
            let mut page = Page::new(PageId(0));
            pm.file.read_exact(&mut page.data)?;
            pm.pages.insert(PageId(0), page);
        } else if file_len > 0 {
            // Partial page
            pm.file.seek(SeekFrom::Start(0))?;
            let mut page = Page::new(PageId(0));
            let to_read = file_len.min(PAGE_SIZE as u64) as usize;
            pm.file.read_exact(&mut page.data[..to_read])?;
            pm.pages.insert(PageId(0), page);
        } else {
            // Empty file - initialize page 0
            let page = Page::new(PageId(0));
            pm.pages.insert(PageId(0), page);
        }

        pm.next_page_id = 1;
        Ok(pm)
    }

    pub fn alloc_page(&mut self) -> PageId {
        let id = PageId(self.next_page_id);
        self.next_page_id += 1;
        self.pages.insert(id, Page::new(id));
        id
    }

    pub fn get_page(&mut self, id: PageId) -> Option<&Page> {
        if self.pages.contains_key(&id) {
            return self.pages.get(&id);
        }
        // Try to load from disk
        let offset = id.0 * PAGE_SIZE as u64;
        let mut page = Page::new(id);
        if self.file.seek(SeekFrom::Start(offset)).is_ok() {
            if self.file.read_exact(&mut page.data).is_ok() {
                self.pages.insert(id, page);
                return self.pages.get(&id);
            }
        }
        None
    }

    pub fn get_page_mut(&mut self, id: PageId) -> Option<&mut Page> {
        if !self.pages.contains_key(&id) {
            let offset = id.0 * PAGE_SIZE as u64;
            let mut page = Page::new(id);
            if self.file.seek(SeekFrom::Start(offset)).is_ok() {
                if self.file.read_exact(&mut page.data).is_ok() {
                    self.pages.insert(id, page);
                }
            }
        }
        self.pages.get_mut(&id)
    }

    pub fn flush_page(&mut self, id: PageId) -> Result<()> {
        if let Some(page) = self.pages.get(&id) {
            let offset = id.0 * PAGE_SIZE as u64;
            self.file.seek(SeekFrom::Start(offset))?;
            self.file.write_all(&page.data)?;
        }
        Ok(())
    }

    pub fn flush_all(&mut self) -> Result<()> {
        let page_ids: Vec<PageId> = self.pages.keys().cloned().collect();
        for id in page_ids {
            self.flush_page(id)?;
        }
        self.file.sync_all()?;
        Ok(())
    }

    pub fn write_catalog(&mut self) -> Result<()> {
        self.flush_page(PageId(0))?;
        self.file.sync_all()?;
        Ok(())
    }

    pub fn save_schema(&mut self, tables: &[TableSchema], indexes: &[IndexDef]) -> Result<()> {
        let all_data = CatalogData {
            tables: tables.to_vec(),
            indexes: indexes.to_vec(),
        };
        let json =
            serde_json::to_vec(&all_data).map_err(|e| MiniDbError::StorageError(e.to_string()))?;

        // Store in page 0: first 4 bytes = length, then JSON data
        let page = self
            .pages
            .get_mut(&PageId(0))
            .ok_or_else(|| MiniDbError::StorageError("No catalog page".into()))?;
        let max_data = PAGE_SIZE - 4;
        let len = json.len().min(max_data);
        page.data[..4].copy_from_slice(&(len as u32).to_le_bytes());
        page.data[4..4 + len].copy_from_slice(&json[..len]);
        // Zero out the rest
        for i in (4 + len)..PAGE_SIZE {
            page.data[i] = 0;
        }
        page.dirty = true;

        Ok(())
    }

    pub fn load_schemas(&self) -> Result<(Vec<TableSchema>, Vec<IndexDef>)> {
        let page = self
            .pages
            .get(&PageId(0))
            .ok_or_else(|| MiniDbError::StorageError("No catalog page".into()))?;
        let len_bytes: [u8; 4] = page.data[..4].try_into().unwrap();
        let len = u32::from_le_bytes(len_bytes) as usize;
        if len == 0 || len > PAGE_SIZE - 4 {
            return Ok((Vec::new(), Vec::new()));
        }
        let json_bytes = &page.data[4..4 + len];
        let data: CatalogData = serde_json::from_slice(json_bytes)
            .map_err(|e| MiniDbError::StorageError(e.to_string()))?;
        Ok((data.tables, data.indexes))
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CatalogData {
    tables: Vec<TableSchema>,
    indexes: Vec<IndexDef>,
}

/// WAL record types
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum WalRecord {
    CreateTable {
        schema: TableSchema,
    },
    DropTable {
        name: String,
    },
    Insert {
        table: String,
        row_id: RowId,
        row: Row,
    },
    Update {
        table: String,
        row_id: RowId,
        old_row: Row,
        new_row: Row,
    },
    Delete {
        table: String,
        row_id: RowId,
    },
    CreateIndex {
        index: IndexDef,
    },
    DropIndex {
        name: String,
    },
    Commit {
        xid: u64,
    },
    Begin {
        xid: u64,
    },
}

/// Write-Ahead Log
pub struct Wal {
    file: File,
    buffer: Vec<u8>,
    path: std::path::PathBuf,
}

impl Wal {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        Ok(Wal {
            file,
            buffer: Vec::new(),
            path: path.to_path_buf(),
        })
    }

    pub fn append(&mut self, record: &WalRecord) -> Result<u64> {
        let payload =
            serde_json::to_vec(record).map_err(|e| MiniDbError::WalError(e.to_string()))?;
        let crc = crc32fast::hash(&payload);

        let offset = self.file.seek(SeekFrom::End(0))?;

        // Write: length(4) + crc(4) + payload
        let len_bytes = (payload.len() as u32).to_le_bytes();
        let crc_bytes = crc.to_le_bytes();

        self.file.write_all(&len_bytes)?;
        self.file.write_all(&crc_bytes)?;
        self.file.write_all(&payload)?;

        Ok(offset)
    }

    pub fn truncate(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        Ok(())
    }

    pub fn read_all(&mut self) -> Result<Vec<WalRecord>> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut records = Vec::new();
        loop {
            let mut len_buf = [0u8; 4];
            match self.file.read_exact(&mut len_buf) {
                Ok(_) => {}
                Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(MiniDbError::IoError(e)),
            }
            let len = u32::from_le_bytes(len_buf) as usize;
            let mut crc_buf = [0u8; 4];
            self.file.read_exact(&mut crc_buf)?;
            let stored_crc = u32::from_le_bytes(crc_buf);

            let mut payload = vec![0u8; len];
            self.file.read_exact(&mut payload)?;

            let actual_crc = crc32fast::hash(&payload);
            if actual_crc != stored_crc {
                log::warn!("WAL CRC mismatch at offset, stopping replay");
                break;
            }

            let record: WalRecord = serde_json::from_slice(&payload)
                .map_err(|e| MiniDbError::WalError(e.to_string()))?;
            records.push(record);
        }
        Ok(records)
    }
}

/// Heap file for row storage - stores rows as JSON in a separate file
pub struct HeapFile {
    rows: Vec<(RowId, Row)>,
    next_row_id: u64,
    table_name: String,
    file_path: std::path::PathBuf,
}

impl HeapFile {
    pub fn new(table_name: String, db_path: &std::path::Path) -> Self {
        let file_path = db_path.with_extension(format!("{}.heap", table_name));
        let mut hf = HeapFile {
            rows: Vec::new(),
            next_row_id: 1,
            table_name,
            file_path,
        };
        let _ = hf.load_from_file();
        hf
    }

    fn load_from_file(&mut self) -> Result<()> {
        if !self.file_path.exists() {
            return Ok(());
        }
        let content = std::fs::read(&self.file_path).map_err(MiniDbError::IoError)?;
        if content.is_empty() {
            return Ok(());
        }
        let data: Vec<Row> = serde_json::from_slice(&content)
            .map_err(|e| MiniDbError::StorageError(e.to_string()))?;
        self.next_row_id = data.len() as u64 + 1;
        self.rows = data
            .into_iter()
            .enumerate()
            .map(|(i, r)| (RowId(i as u64 + 1), r))
            .collect();
        Ok(())
    }

    fn save_to_file(&self) -> Result<()> {
        let data: Vec<&Row> = self.rows.iter().map(|(_, r)| r).collect();
        let json =
            serde_json::to_vec(&data).map_err(|e| MiniDbError::StorageError(e.to_string()))?;
        std::fs::write(&self.file_path, &json).map_err(MiniDbError::IoError)?;
        Ok(())
    }

    pub fn next_row_id(&mut self) -> RowId {
        let id = RowId(self.next_row_id);
        self.next_row_id += 1;
        id
    }

    pub fn insert_row(&mut self, row: Row) -> RowId {
        let row_id = self.next_row_id();
        self.rows.push((row_id, row));
        row_id
    }

    pub fn update_row(&mut self, row_id: RowId, new_row: Row) -> bool {
        if let Some(entry) = self.rows.iter_mut().find(|(rid, _)| *rid == row_id) {
            entry.1 = new_row;
            true
        } else {
            false
        }
    }

    pub fn delete_row(&mut self, row_id: RowId) -> bool {
        let len_before = self.rows.len();
        self.rows.retain(|(rid, _)| *rid != row_id);
        self.rows.len() < len_before
    }

    pub fn flush(&self) -> Result<()> {
        self.save_to_file()
    }

    pub fn get_all_rows(&self) -> Vec<(RowId, &Row)> {
        self.rows.iter().map(|(rid, row)| (*rid, row)).collect()
    }

    pub fn delete_file(&self) -> Result<()> {
        if self.file_path.exists() {
            std::fs::remove_file(&self.file_path).map_err(MiniDbError::IoError)?;
        }
        Ok(())
    }
}

/// B-tree node for secondary index
#[derive(Debug, Clone)]
pub enum BTreeNode {
    Leaf {
        keys: Vec<Value>,
        row_ids: Vec<Vec<RowId>>,
        next: Option<PageId>,
    },
    Internal {
        keys: Vec<Value>,
        children: Vec<PageId>,
    },
}

/// B-tree index on a single column
pub struct BTreeIndex {
    root: Option<PageId>,
    nodes: BTreeMap<PageId, BTreeNode>,
    column_type: crate::types::ColumnType,
    next_page: u64,
}

impl BTreeIndex {
    pub fn new(column_type: crate::types::ColumnType) -> Self {
        BTreeIndex {
            root: None,
            nodes: BTreeMap::new(),
            column_type,
            next_page: 0,
        }
    }

    fn alloc_node(&mut self) -> PageId {
        let id = PageId(self.next_page);
        self.next_page += 1;
        id
    }

    pub fn insert(&mut self, key: Value, row_id: RowId) {
        if self.root.is_none() {
            let root = self.alloc_node();
            self.nodes.insert(
                root,
                BTreeNode::Leaf {
                    keys: vec![key],
                    row_ids: vec![vec![row_id]],
                    next: None,
                },
            );
            self.root = Some(root);
            return;
        }

        let root = self.root.unwrap();
        let result = self.insert_into_node(root, key, row_id);

        if let Some((new_key, new_child)) = result {
            let old_root = self.root.unwrap();
            let new_root = self.alloc_node();
            self.nodes.insert(
                new_root,
                BTreeNode::Internal {
                    keys: vec![new_key],
                    children: vec![old_root, new_child],
                },
            );
            self.root = Some(new_root);
        }
    }

    fn insert_into_node(
        &mut self,
        node_id: PageId,
        key: Value,
        row_id: RowId,
    ) -> Option<(Value, PageId)> {
        let node = self.nodes.get(&node_id).cloned()?;
        match node {
            BTreeNode::Leaf {
                mut keys,
                mut row_ids,
                next,
            } => {
                // Find insertion position
                let pos = keys
                    .iter()
                    .position(|k| compare_values(k, &key, true) != std::cmp::Ordering::Less)
                    .unwrap_or(keys.len());

                if pos < keys.len()
                    && compare_values(&keys[pos], &key, false) == std::cmp::Ordering::Equal
                {
                    row_ids[pos].push(row_id);
                } else {
                    keys.insert(pos, key);
                    row_ids.insert(pos, vec![row_id]);
                }

                // Split if too large (simple: split at 100 keys)
                if keys.len() > 100 {
                    let mid = keys.len() / 2;
                    let right_keys = keys.split_off(mid);
                    let right_row_ids = row_ids.split_off(mid);

                    let new_leaf = self.alloc_node();
                    self.nodes.insert(
                        new_leaf,
                        BTreeNode::Leaf {
                            keys: right_keys.clone(),
                            row_ids: right_row_ids,
                            next,
                        },
                    );
                    self.nodes.insert(
                        node_id,
                        BTreeNode::Leaf {
                            keys,
                            row_ids,
                            next: Some(new_leaf),
                        },
                    );
                    Some((right_keys.into_iter().next().unwrap(), new_leaf))
                } else {
                    self.nodes.insert(
                        node_id,
                        BTreeNode::Leaf {
                            keys,
                            row_ids,
                            next,
                        },
                    );
                    None
                }
            }
            BTreeNode::Internal { keys, children } => {
                // Find child to descend into
                let mut pos = 0;
                for (i, k) in keys.iter().enumerate() {
                    if compare_values(&key, k, false) == std::cmp::Ordering::Greater {
                        pos = i + 1;
                    } else {
                        break;
                    }
                }
                let child_id = children[pos];
                let result = self.insert_into_node(child_id, key, row_id);

                if let Some((new_key, new_child)) = result {
                    let mut keys = keys;
                    let mut children = children;
                    keys.insert(pos, new_key);
                    children.insert(pos + 1, new_child);

                    if keys.len() > 100 {
                        let mid = keys.len() / 2;
                        let split_key = keys.remove(mid);
                        let right_keys = keys.split_off(mid);
                        let right_children = children.split_off(mid + 1);

                        let new_internal = self.alloc_node();
                        self.nodes.insert(
                            new_internal,
                            BTreeNode::Internal {
                                keys: right_keys,
                                children: right_children,
                            },
                        );
                        self.nodes
                            .insert(node_id, BTreeNode::Internal { keys, children });
                        return Some((split_key, new_internal));
                    }

                    self.nodes
                        .insert(node_id, BTreeNode::Internal { keys, children });
                }
                None
            }
        }
    }

    pub fn search_eq(&self, key: &Value) -> Vec<RowId> {
        if let Some(root) = self.root {
            self.search_eq_node(root, key)
        } else {
            Vec::new()
        }
    }

    fn search_eq_node(&self, node_id: PageId, key: &Value) -> Vec<RowId> {
        if let Some(node) = self.nodes.get(&node_id) {
            match node {
                BTreeNode::Leaf {
                    keys,
                    row_ids,
                    next,
                } => {
                    for (i, k) in keys.iter().enumerate() {
                        if compare_values(k, key, false) == std::cmp::Ordering::Equal {
                            return row_ids[i].clone();
                        }
                        if compare_values(k, key, true) == std::cmp::Ordering::Greater {
                            break;
                        }
                    }
                    if let Some(next_id) = next {
                        return self.search_eq_node(*next_id, key);
                    }
                    Vec::new()
                }
                BTreeNode::Internal { keys, children } => {
                    let mut child_idx = children.len() - 1;
                    for (i, k) in keys.iter().enumerate() {
                        if compare_values(key, k, false) != std::cmp::Ordering::Greater {
                            child_idx = i;
                            break;
                        }
                    }
                    self.search_eq_node(children[child_idx], key)
                }
            }
        } else {
            Vec::new()
        }
    }

    pub fn search_range(&self, min: Option<&Value>, max: Option<&Value>) -> Vec<RowId> {
        if let Some(root) = self.root {
            let mut results = Vec::new();
            self.collect_range(root, min, max, &mut results);
            results
        } else {
            Vec::new()
        }
    }

    fn collect_range(
        &self,
        node_id: PageId,
        min: Option<&Value>,
        max: Option<&Value>,
        results: &mut Vec<RowId>,
    ) {
        if let Some(node) = self.nodes.get(&node_id) {
            match node {
                BTreeNode::Leaf {
                    keys,
                    row_ids,
                    next,
                } => {
                    for (i, k) in keys.iter().enumerate() {
                        let above_min = min.map_or(true, |m| {
                            compare_values(k, m, false) != std::cmp::Ordering::Less
                        });
                        let below_max = max.map_or(true, |m| {
                            compare_values(k, m, false) != std::cmp::Ordering::Greater
                        });
                        if above_min && below_max {
                            results.extend_from_slice(&row_ids[i]);
                        }
                    }
                    if let Some(next_id) = next {
                        let above_min = min.map_or(true, |m| {
                            keys.last().map_or(true, |k| {
                                compare_values(k, m, false) != std::cmp::Ordering::Less
                            })
                        });
                        if above_min {
                            self.collect_range(*next_id, min, max, results);
                        }
                    }
                }
                BTreeNode::Internal { keys, children } => {
                    for (i, k) in keys.iter().enumerate() {
                        let above_min = min.map_or(true, |m| {
                            compare_values(k, m, false) != std::cmp::Ordering::Less
                        });
                        if above_min {
                            self.collect_range(children[i], min, max, results);
                        }
                    }
                    // Check last child
                    let last_above_min = min.map_or(true, |m| {
                        keys.last().map_or(true, |k| {
                            compare_values(k, m, false) != std::cmp::Ordering::Less
                        })
                    });
                    if last_above_min {
                        self.collect_range(*children.last().unwrap(), min, max, results);
                    }
                }
            }
        }
    }

    pub fn delete(&mut self, key: &Value, row_id: RowId) {
        if let Some(root) = self.root {
            self.delete_from_node(root, key, row_id);
        }
    }

    fn delete_from_node(&mut self, node_id: PageId, key: &Value, row_id: RowId) {
        if let Some(node) = self.nodes.get(&node_id).cloned() {
            match node {
                BTreeNode::Leaf {
                    mut keys,
                    mut row_ids,
                    next,
                } => {
                    for (i, k) in keys.iter().enumerate() {
                        if compare_values(k, key, false) == std::cmp::Ordering::Equal {
                            row_ids[i].retain(|&rid| rid != row_id);
                            if row_ids[i].is_empty() {
                                keys.remove(i);
                                row_ids.remove(i);
                            }
                            break;
                        }
                    }
                    self.nodes.insert(
                        node_id,
                        BTreeNode::Leaf {
                            keys,
                            row_ids,
                            next,
                        },
                    );
                }
                BTreeNode::Internal { keys, children } => {
                    let mut child_idx = children.len() - 1;
                    for (i, k) in keys.iter().enumerate() {
                        if compare_values(key, k, false) != std::cmp::Ordering::Greater {
                            child_idx = i;
                            break;
                        }
                    }
                    self.delete_from_node(children[child_idx], key, row_id);
                }
            }
        }
    }
}

fn compare_values(a: &Value, b: &Value, nulls_last: bool) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
        (Value::Null, _) => {
            if nulls_last {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Less
            }
        }
        (_, Value::Null) => {
            if nulls_last {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        }
        (Value::Int(a), Value::Int(b)) => a.cmp(b),
        (Value::Int(a), Value::Real(b)) => (*a as f64)
            .partial_cmp(b)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Value::Real(a), Value::Int(b)) => a
            .partial_cmp(&(*b as f64))
            .unwrap_or(std::cmp::Ordering::Equal),
        (Value::Real(a), Value::Real(b)) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
        (Value::Text(a), Value::Text(b)) => a.cmp(b),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        _ => std::cmp::Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_page_manager() {
        let dir = env::temp_dir().join("minidb_test_pm");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");
        let mut pm = PageManager::open(&path).unwrap();
        let pid = pm.alloc_page();
        assert_eq!(pid, PageId(1));
        let page = pm.get_page_mut(pid).unwrap();
        page.data[0] = 42;
        pm.flush_all().unwrap();
        drop(pm);

        let mut pm2 = PageManager::open(&path).unwrap();
        let page = pm2.get_page(pid).unwrap();
        assert_eq!(page.data[0], 42);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_wal() {
        let dir = env::temp_dir().join("minidb_test_wal");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.wal");
        let mut wal = Wal::open(&path).unwrap();

        let record = WalRecord::Begin { xid: 1 };
        wal.append(&record).unwrap();
        let record2 = WalRecord::Commit { xid: 1 };
        wal.append(&record2).unwrap();

        let records = wal.read_all().unwrap();
        assert_eq!(records.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_btree_index() {
        let mut idx = BTreeIndex::new(crate::types::ColumnType::Int);
        idx.insert(Value::Int(5), RowId(1));
        idx.insert(Value::Int(3), RowId(2));
        idx.insert(Value::Int(7), RowId(3));
        idx.insert(Value::Int(1), RowId(4));

        let results = idx.search_eq(&Value::Int(5));
        assert_eq!(results, vec![RowId(1)]);

        let results = idx.search_range(Some(&Value::Int(3)), Some(&Value::Int(7)));
        assert!(results.contains(&RowId(2)));
        assert!(results.contains(&RowId(1)));
        assert!(results.contains(&RowId(3)));
        assert!(!results.contains(&RowId(4)));
    }

    #[test]
    fn test_compare_values() {
        assert_eq!(
            compare_values(&Value::Int(1), &Value::Int(2), false),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_values(&Value::Int(2), &Value::Int(2), false),
            std::cmp::Ordering::Equal
        );
        assert_eq!(
            compare_values(&Value::Int(3), &Value::Int(2), false),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_values(&Value::Text("a".into()), &Value::Text("b".into()), false),
            std::cmp::Ordering::Less
        );
    }
}
