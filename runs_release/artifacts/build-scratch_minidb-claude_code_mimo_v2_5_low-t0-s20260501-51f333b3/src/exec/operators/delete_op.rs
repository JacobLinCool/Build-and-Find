use super::PhysicalOperator;
use crate::catalog::Catalog;
use crate::error::Result;
use crate::exec::evaluator::ExprEvaluator;
use crate::parser::ast::Expr;
use crate::storage::manager::StorageManager;
use crate::types::{Row, Value};

pub struct DeleteOp<'a> {
    table: String,
    where_clause: Option<Expr>,
    catalog: &'a mut Catalog,
    storage: &'a mut StorageManager,
    done: bool,
    count: usize,
}

impl<'a> DeleteOp<'a> {
    pub fn new(
        table: String,
        where_clause: Option<Expr>,
        catalog: &'a mut Catalog,
        storage: &'a mut StorageManager,
    ) -> Self {
        DeleteOp {
            table,
            where_clause,
            catalog,
            storage,
            done: false,
            count: 0,
        }
    }
}

impl<'a> PhysicalOperator for DeleteOp<'a> {
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

        let where_evaluator = self
            .where_clause
            .as_ref()
            .map(|expr| ExprEvaluator::new(expr.clone(), col_names.clone()));

        let rows = self.storage.scan_page(root_page)?;

        for (slot, data) in rows {
            let values = super::seq_scan::deserialize_row(&data, col_names.len())?;
            let row = Row::new(values);

            if let Some(ref evaluator) = where_evaluator {
                let result = evaluator.evaluate(&row)?;
                match result {
                    Value::Bool(true) => {}
                    _ => continue,
                }
            }

            self.storage.delete_row(root_page, slot)?;
            self.count += 1;
        }

        self.done = true;

        let summary = if self.count == 1 {
            "ok 1 row deleted".to_string()
        } else {
            format!("ok {} rows deleted", self.count)
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
