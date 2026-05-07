use super::PhysicalOperator;
use crate::catalog::Catalog;
use crate::error::Result;
use crate::exec::evaluator::ExprEvaluator;
use crate::exec::operators::seq_scan::serialize_row;
use crate::parser::ast::Expr;
use crate::storage::manager::StorageManager;
use crate::types::{Row, Value};

pub struct InsertOp<'a> {
    table: String,
    columns: Option<Vec<String>>,
    values: Vec<Vec<Expr>>,
    catalog: &'a mut Catalog,
    storage: &'a mut StorageManager,
    inserted: bool,
    count: usize,
}

impl<'a> InsertOp<'a> {
    pub fn new(
        table: String,
        columns: Option<Vec<String>>,
        values: Vec<Vec<Expr>>,
        catalog: &'a mut Catalog,
        storage: &'a mut StorageManager,
    ) -> Self {
        InsertOp {
            table,
            columns,
            values,
            catalog,
            storage,
            inserted: false,
            count: 0,
        }
    }
}

impl<'a> PhysicalOperator for InsertOp<'a> {
    fn open(&mut self) -> Result<()> {
        self.inserted = false;
        self.count = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.inserted {
            return Ok(None);
        }

        let schema = self.catalog.get_table(&self.table)?.clone();
        let root_page = schema.root_page;
        let col_names: Vec<String> = schema.columns.iter().map(|c| c.name.clone()).collect();

        // Build evaluator context with empty row (for expressions)
        let empty_row = Row::new(vec![Value::Null; col_names.len()]);
        let evaluator = ExprEvaluator::new(
            Expr::Literal(crate::types::SqlLiteral::Null),
            col_names.clone(),
        );

        for row_values in &self.values {
            let mut full_values = self.catalog.default_values(&self.table)?;

            if let Some(ref cols) = self.columns {
                for (i, col_name) in cols.iter().enumerate() {
                    if let Some(idx) = col_names.iter().position(|c| c == col_name) {
                        if i < row_values.len() {
                            let val = evaluator.evaluate_with_row(&row_values[i], &empty_row)?;
                            full_values[idx] = val;
                        }
                    }
                }
            } else {
                for (i, expr) in row_values.iter().enumerate() {
                    if i < full_values.len() {
                        let val = evaluator.evaluate_with_row(expr, &empty_row)?;
                        full_values[i] = val;
                    }
                }
            }

            self.catalog.validate_row(&self.table, &full_values)?;

            // Handle INSERT with specific columns
            let actual_values = if let Some(ref cols) = self.columns {
                let mut vals = self.catalog.default_values(&self.table)?;
                for (i, col_name) in cols.iter().enumerate() {
                    if let Some(idx) = col_names.iter().position(|c| c == col_name) {
                        if i < row_values.len() {
                            vals[idx] = evaluator.evaluate_with_row(&row_values[i], &empty_row)?;
                        }
                    }
                }
                vals
            } else {
                let mut vals = Vec::new();
                for (i, expr) in row_values.iter().enumerate() {
                    if i < schema.columns.len() {
                        vals.push(evaluator.evaluate_with_row(expr, &empty_row)?);
                    }
                }
                // Pad with defaults
                while vals.len() < schema.columns.len() {
                    let defaults = self.catalog.default_values(&self.table)?;
                    if vals.len() < defaults.len() {
                        vals.push(defaults[vals.len()].clone());
                    } else {
                        break;
                    }
                }
                vals
            };

            let row_data = serialize_row(&actual_values);
            self.storage.insert_row(root_page, &row_data)?;

            // Update indexes if any
            let index_cols: Vec<(String, u64)> = {
                let catalog = &self.catalog;
                catalog
                    .all_index_names()
                    .iter()
                    .filter_map(|name| {
                        let idx = catalog.get_index(name).ok()?;
                        if idx.table == self.table {
                            Some((idx.column.clone(), idx.root_page))
                        } else {
                            None
                        }
                    })
                    .collect()
            };

            for (col_name, idx_root) in index_cols {
                if let Some(col_idx) = col_names.iter().position(|c| *c == col_name) {
                    let val = &actual_values[col_idx];
                    let slot = self.storage.num_rows(root_page)? as u16 - 1;
                    self.storage.btree_insert(idx_root, val, root_page, slot)?;
                }
            }

            self.count += 1;
        }

        self.inserted = true;

        let summary = if self.count == 1 {
            "ok 1 row inserted".to_string()
        } else {
            format!("ok {} rows inserted", self.count)
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
