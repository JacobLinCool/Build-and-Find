use super::PhysicalOperator;
use crate::catalog::Catalog;
use crate::error::Result;
use crate::exec::evaluator::ExprEvaluator;
use crate::exec::operators::seq_scan::serialize_row;
use crate::parser::ast::Expr;
use crate::storage::manager::StorageManager;
use crate::types::{Row, Value};

pub struct UpdateOp<'a> {
    table: String,
    set_clauses: Vec<(String, Expr)>,
    where_clause: Option<Expr>,
    catalog: &'a mut Catalog,
    storage: &'a mut StorageManager,
    done: bool,
    count: usize,
}

impl<'a> UpdateOp<'a> {
    pub fn new(
        table: String,
        set_clauses: Vec<(String, Expr)>,
        where_clause: Option<Expr>,
        catalog: &'a mut Catalog,
        storage: &'a mut StorageManager,
    ) -> Self {
        UpdateOp {
            table,
            set_clauses,
            where_clause,
            catalog,
            storage,
            done: false,
            count: 0,
        }
    }
}

impl<'a> PhysicalOperator for UpdateOp<'a> {
    fn open(&mut self) -> Result<()> {
        self.done = false;
        self.count = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.done {
            return Ok(None);
        }

        let schema = self.catalog.get_table(&self.table)?.clone();
        let root_page = schema.root_page;
        let col_names: Vec<String> = schema.columns.iter().map(|c| c.name.clone()).collect();
        let empty_row = Row::new(vec![Value::Null; col_names.len()]);

        let where_evaluator = self
            .where_clause
            .as_ref()
            .map(|expr| ExprEvaluator::new(expr.clone(), col_names.clone()));

        let set_evaluators: Vec<(usize, ExprEvaluator)> = self
            .set_clauses
            .iter()
            .filter_map(|(col_name, expr)| {
                col_names
                    .iter()
                    .position(|c| c == col_name)
                    .map(|idx| (idx, ExprEvaluator::new(expr.clone(), col_names.clone())))
            })
            .collect();

        let rows = self.storage.scan_page(root_page)?;

        for (slot, data) in rows {
            let values = super::seq_scan::deserialize_row(&data, col_names.len())?;
            let row = Row::new(values.clone());

            if let Some(ref evaluator) = where_evaluator {
                let result = evaluator.evaluate(&row)?;
                match result {
                    Value::Bool(true) => {}
                    _ => continue,
                }
            }

            let mut new_values = values.clone();
            for (idx, evaluator) in &set_evaluators {
                let new_val = evaluator.evaluate(&row)?;
                new_values[*idx] = new_val;
            }

            // Validate
            self.catalog.validate_row(&self.table, &new_values)?;

            // Update row
            let row_data = serialize_row(&new_values);
            let mut page = self.storage.load_page(root_page)?;
            let slot_offset = 4 + slot as usize * 4;
            let offset = page.read_u16(slot_offset) as usize;
            let len = page.read_u16(slot_offset + 2) as usize;

            // Simple approach: delete and re-insert
            self.storage.delete_row(root_page, slot)?;
            self.storage.insert_row(root_page, &row_data)?;

            self.count += 1;
        }

        self.done = true;

        let summary = if self.count == 1 {
            "ok 1 row updated".to_string()
        } else {
            format!("ok {} rows updated", self.count)
        };

        log::info!("{}", summary);

        Ok(Some(Row::new(vec![Value::Text(summary)])))
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        vec!["result".to_string()]
    }
}
