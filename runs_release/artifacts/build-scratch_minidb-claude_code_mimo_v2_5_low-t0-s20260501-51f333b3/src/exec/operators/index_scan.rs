use super::PhysicalOperator;
use crate::catalog::Catalog;
use crate::error::Result;
use crate::parser::ast::Expr;
use crate::storage::manager::StorageManager;
use crate::types::{Row, Value};

pub struct IndexScan<'a> {
    table: String,
    column: String,
    predicate: Expr,
    catalog: &'a mut Catalog,
    storage: &'a mut StorageManager,
    row_ids: Vec<(u64, u16)>,
    pos: usize,
    column_count: usize,
}

impl<'a> IndexScan<'a> {
    pub fn new(
        table: String,
        column: String,
        predicate: Expr,
        catalog: &'a mut Catalog,
        storage: &'a mut StorageManager,
    ) -> Result<Self> {
        let schema = catalog.get_table(&table)?;
        let column_count = schema.columns.len();

        Ok(IndexScan {
            table,
            column,
            predicate,
            catalog,
            storage,
            row_ids: Vec::new(),
            pos: 0,
            column_count,
        })
    }
}

impl<'a> PhysicalOperator for IndexScan<'a> {
    fn open(&mut self) -> Result<()> {
        // For now, fall back to seq scan since index lookup is complex
        let schema = self.catalog.get_table(&self.table)?;
        let root_page = schema.root_page;
        self.row_ids = self
            .storage
            .scan_page(root_page)?
            .into_iter()
            .map(|(slot, _)| (root_page, slot))
            .collect();
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        while self.pos < self.row_ids.len() {
            let (page_id, slot) = self.row_ids[self.pos];
            self.pos += 1;

            let data = self.storage.get_row(page_id, slot)?;
            let values = super::seq_scan::deserialize_row(&data, self.column_count)?;
            return Ok(Some(Row::new(values)));
        }
        Ok(None)
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        self.catalog
            .get_table(&self.table)
            .map(|s| s.columns.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default()
    }
}
