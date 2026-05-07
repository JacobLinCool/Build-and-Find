//! Secondary-index driven scan.

use super::{resolve_index_predicate, ExecContext, PhysicalOperator, ResolvedIndexPredicate};
use crate::error::{MiniDbError, Result};
use crate::plan::IndexPredicate;
use crate::storage::Index;
use crate::types::{Row, Schema};

pub struct IndexScan {
    table: String,
    alias: Option<String>,
    index: String,
    predicate: IndexPredicate,
    schema: Schema,
    buffer: Vec<Row>,
    cursor: usize,
}

impl IndexScan {
    pub fn new(
        table: String,
        alias: Option<String>,
        index: String,
        predicate: IndexPredicate,
        schema: Schema,
    ) -> Self {
        Self {
            table,
            alias,
            index,
            predicate,
            schema,
            buffer: Vec::new(),
            cursor: 0,
        }
    }

    pub fn table_name(&self) -> &str {
        &self.table
    }

    pub fn index_name(&self) -> &str {
        &self.index
    }

    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }
}

impl PhysicalOperator for IndexScan {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        let resolved = resolve_index_predicate(&self.predicate)?;
        let tbl = ctx.catalog.table(&self.table)?;
        let idx = tbl
            .indexes
            .get(&self.index.to_ascii_lowercase())
            .ok_or_else(|| {
                MiniDbError::Catalog(format!("index `{}` not loaded", self.index))
            })?;
        let row_ids = match resolved {
            ResolvedIndexPredicate::Eq(v) => idx.lookup_eq(&v),
            ResolvedIndexPredicate::Range { lo, hi } => idx.lookup_range(
                lo.as_ref().map(|(v, _)| v),
                hi.as_ref().map(|(v, _)| v),
            ),
        };
        self.buffer.clear();
        for rid in row_ids {
            if let Some(r) = tbl.heap.get(rid) {
                self.buffer.push(r.clone());
            }
        }
        self.cursor = 0;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecContext<'_>) -> Result<Option<Row>> {
        if self.cursor >= self.buffer.len() {
            return Ok(None);
        }
        let row = self.buffer[self.cursor].clone();
        self.cursor += 1;
        Ok(Some(row))
    }

    fn close(&mut self, _ctx: &mut ExecContext<'_>) -> Result<()> {
        self.buffer.clear();
        Ok(())
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}
