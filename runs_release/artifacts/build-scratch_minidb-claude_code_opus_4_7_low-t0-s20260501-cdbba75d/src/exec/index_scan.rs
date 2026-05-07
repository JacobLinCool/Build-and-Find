use super::operator::{ExecColumn, ExecSchema, PhysicalOperator, Tuple};
use crate::catalog::{IndexMeta, TableMeta};
use crate::error::Result;
use crate::plan::physical::IndexBound;
use crate::types::Value;

/// Volcano `IndexScan`: walks a B-tree index for an equality or range bound,
/// then materializes each matching heap row.
pub struct IndexScan {
    schema: ExecSchema,
    rows: Vec<Tuple>,
    cursor: usize,
}

impl IndexScan {
    pub fn new(
        meta: &TableMeta,
        idx: &IndexMeta,
        alias: Option<&str>,
        bound: &IndexBound,
        bound_lo: Option<Value>,
        bound_hi: Option<Value>,
    ) -> Self {
        let qualifier = alias.unwrap_or(&meta.name).to_string();
        let columns = meta
            .schema
            .columns
            .iter()
            .map(|c| ExecColumn {
                qualifier: Some(qualifier.clone()),
                name: c.name.clone(),
                data_type: Some(c.data_type),
            })
            .collect();
        let row_ids = match bound {
            IndexBound::Eq(_) => idx
                .btree
                .lookup_eq(bound_lo.as_ref().unwrap_or(&Value::Null)),
            IndexBound::Range { lo, hi } => idx.btree.range(
                bound_lo.as_ref(),
                lo.as_ref().map(|(_, inc)| *inc).unwrap_or(true),
                bound_hi.as_ref(),
                hi.as_ref().map(|(_, inc)| *inc).unwrap_or(true),
            ),
        };
        let rows = row_ids
            .into_iter()
            .filter_map(|rid| meta.rows.get(&rid).cloned())
            .collect();
        IndexScan {
            schema: ExecSchema { columns },
            rows,
            cursor: 0,
        }
    }
}

impl PhysicalOperator for IndexScan {
    fn open(&mut self) -> Result<()> {
        self.cursor = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Tuple>> {
        if self.cursor < self.rows.len() {
            let row = self.rows[self.cursor].clone();
            self.cursor += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &ExecSchema {
        &self.schema
    }
}
