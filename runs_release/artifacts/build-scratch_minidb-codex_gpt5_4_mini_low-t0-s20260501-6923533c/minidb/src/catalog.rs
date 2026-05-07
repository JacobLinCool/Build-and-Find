use crate::types::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Table {
    pub schema: Schema,
    pub rows: BTreeMap<u64, Row>,
    pub next_row_id: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Index {
    pub table: String,
    pub column: String,
    pub entries: BTreeMap<String, Vec<u64>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub tables: BTreeMap<String, Table>,
    pub indexes: BTreeMap<String, Index>,
}

impl Catalog {
    pub fn table_names(&self) -> Vec<String> {
        self.tables.keys().cloned().collect()
    }
    pub fn index_names(&self) -> Vec<String> {
        self.indexes.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_names() {
        let c = Catalog::default();
        assert!(c.table_names().is_empty());
    }
}
