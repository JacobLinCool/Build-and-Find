pub mod expression;

use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::parser::ast;
use crate::plan::physical::PhysicalPlan;
use crate::storage::btree::IndexKey;
use crate::storage::heap::HeapFile;
use crate::types::{ColumnType, Value};
use expression::eval_expr;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

pub struct Executor<'a> {
    pub catalog: &'a mut Catalog,
    pub heap: &'a mut HeapFile,
    pub tx_id: u64,
    plan: PhysicalPlan,
    output_format: OutputFormat,
}

impl<'a> Executor<'a> {
    pub fn new(
        catalog: &'a mut Catalog,
        heap: &'a mut HeapFile,
        tx_id: u64,
        plan: PhysicalPlan,
    ) -> Self {
        Self {
            catalog,
            heap,
            tx_id,
            plan,
            output_format: OutputFormat::Table,
        }
    }

    pub fn with_format(mut self, format: OutputFormat) -> Self {
        self.output_format = format;
        self
    }

    pub fn execute(&mut self) -> Result<ExecutionResult> {
        match self.plan.clone() {
            PhysicalPlan::Insert {
                table,
                columns,
                values,
            } => self.execute_insert(&table, columns, &values),
            PhysicalPlan::Update {
                table,
                assignments,
                where_clause,
            } => self.execute_update(&table, &assignments, where_clause.as_ref()),
            PhysicalPlan::Delete {
                table,
                where_clause,
            } => self.execute_delete(&table, where_clause.as_ref()),
            plan => self.execute_query(&plan),
        }
    }

    fn execute_insert(
        &mut self,
        table: &str,
        columns: Option<Vec<String>>,
        values: &[ast::Expr],
    ) -> Result<ExecutionResult> {
        let schema = self.catalog.get_table(table)?.clone();
        let mut row_values: Vec<Value> = Vec::new();
        let empty_row: Vec<Value> = Vec::new();

        if let Some(ref cols) = columns {
            for col_def in &schema.columns {
                if let Some(pos) = cols.iter().position(|c| c == &col_def.name) {
                    row_values.push(eval_expr(&values[pos], &empty_row, &[])?);
                } else if let Some(ref default) = col_def.default {
                    row_values.push(default.clone());
                } else if col_def.primary_key || col_def.not_null {
                    return Err(MiniDbError::Runtime(format!(
                        "Column '{}' is required",
                        col_def.name
                    )));
                } else {
                    row_values.push(Value::Null);
                }
            }
        } else {
            for (i, col_def) in schema.columns.iter().enumerate() {
                if i < values.len() {
                    row_values.push(eval_expr(&values[i], &empty_row, &[])?);
                } else if let Some(ref default) = col_def.default {
                    row_values.push(default.clone());
                } else {
                    row_values.push(Value::Null);
                }
            }
        }

        for (i, col_def) in schema.columns.iter().enumerate() {
            if col_def.not_null && row_values[i].is_null() {
                return Err(MiniDbError::Constraint(format!(
                    "Column '{}' cannot be NULL",
                    col_def.name
                )));
            }
        }

        let data = expression::serialize_row(&row_values);
        let row_id = self.heap.add_row(table, &data)?;

        let index_names: Vec<String> = self
            .catalog
            .find_all_indexes_for_table(table)
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        for idx_name in &index_names {
            let col_name = self.catalog.get_index(idx_name)?.column.clone();
            if let Some(pos) = schema.columns.iter().position(|c| c.name == col_name) {
                let key = IndexKey::from_value(&row_values[pos]);
                self.catalog.add_index_entry(idx_name, key, row_id)?;
            }
        }

        Ok(ExecutionResult {
            columns: vec!["rows_affected".to_string()],
            column_types: vec![ColumnType::Int],
            rows: vec![vec![Value::Int(1)]],
            affected_rows: 1,
            is_query: false,
        })
    }

    fn execute_update(
        &mut self,
        table: &str,
        assignments: &[ast::Assignment],
        where_clause: Option<&ast::Expr>,
    ) -> Result<ExecutionResult> {
        let schema = self.catalog.get_table(table)?.clone();
        let row_schema: Vec<(String, ColumnType)> = schema
            .columns
            .iter()
            .map(|c| (c.name.clone(), c.col_type.clone()))
            .collect();
        let row_ids = self.heap.all_row_ids();
        let mut count = 0i64;

        for row_id in row_ids {
            let data = match self.heap.get_row(row_id) {
                Ok(d) if !d.is_empty() && d[0] != 0xFF => d,
                _ => continue,
            };
            let row = expression::deserialize_row(&data, &row_schema);
            if let Some(wc) = where_clause {
                match eval_expr(wc, &row, &row_schema)? {
                    Value::Bool(true) => {}
                    _ => continue,
                }
            }
            let mut new_row = row.clone();
            for assignment in assignments {
                if let Some(pos) = row_schema
                    .iter()
                    .position(|(name, _)| name == &assignment.column)
                {
                    new_row[pos] = eval_expr(&assignment.expr, &row, &row_schema)?;
                }
            }
            let new_data = expression::serialize_row(&new_row);
            self.heap.update_row(row_id, &new_data)?;
            count += 1;
        }

        Ok(ExecutionResult {
            columns: vec!["rows_affected".to_string()],
            column_types: vec![ColumnType::Int],
            rows: vec![vec![Value::Int(count)]],
            affected_rows: count as usize,
            is_query: false,
        })
    }

    fn execute_delete(
        &mut self,
        table: &str,
        where_clause: Option<&ast::Expr>,
    ) -> Result<ExecutionResult> {
        let schema = self.catalog.get_table(table)?.clone();
        let row_schema: Vec<(String, ColumnType)> = schema
            .columns
            .iter()
            .map(|c| (c.name.clone(), c.col_type.clone()))
            .collect();
        let row_ids = self.heap.all_row_ids();
        let mut count = 0i64;

        for row_id in row_ids {
            let data = match self.heap.get_row(row_id) {
                Ok(d) if !d.is_empty() && d[0] != 0xFF => d,
                _ => continue,
            };
            let row = expression::deserialize_row(&data, &row_schema);
            if let Some(wc) = where_clause {
                match eval_expr(wc, &row, &row_schema)? {
                    Value::Bool(true) => {}
                    _ => continue,
                }
            }
            let mut del_data = vec![0xFFu8];
            del_data.extend_from_slice(&row_id.0.to_be_bytes());
            self.heap.update_row(row_id, &del_data)?;
            count += 1;
        }

        Ok(ExecutionResult {
            columns: vec!["rows_affected".to_string()],
            column_types: vec![ColumnType::Int],
            rows: vec![vec![Value::Int(count)]],
            affected_rows: count as usize,
            is_query: false,
        })
    }

    fn execute_query(&mut self, plan: &PhysicalPlan) -> Result<ExecutionResult> {
        match plan.clone() {
            PhysicalPlan::SeqScan { table, alias } => {
                let schema = self.catalog.get_table(&table)?;
                let col_schema: Vec<(String, ColumnType)> = schema
                    .columns
                    .iter()
                    .map(|c| {
                        let name = alias
                            .as_ref()
                            .map(|a| format!("{}.{}", a, c.name))
                            .unwrap_or_else(|| c.name.clone());
                        (name, c.col_type.clone())
                    })
                    .collect();
                let row_ids = self.heap.all_row_ids();
                let mut rows = Vec::new();
                for row_id in row_ids {
                    if let Ok(data) = self.heap.get_row(row_id) {
                        if !data.is_empty() && data[0] != 0xFF {
                            rows.push(expression::deserialize_row(&data, &col_schema));
                        }
                    }
                }
                Ok(ExecutionResult {
                    columns: col_schema.iter().map(|(n, _)| n.clone()).collect(),
                    column_types: col_schema.iter().map(|(_, t)| t.clone()).collect(),
                    rows,
                    affected_rows: 0,
                    is_query: true,
                })
            }
            PhysicalPlan::IndexScan {
                table,
                index_name,
                predicate,
                column: _,
            } => {
                let idx_data = self.catalog.get_index_data(&index_name)?;
                let row_ids = idx_data.lookup(&predicate);
                let schema = self.catalog.get_table(&table)?;
                let col_schema: Vec<(String, ColumnType)> = schema
                    .columns
                    .iter()
                    .map(|c| (c.name.clone(), c.col_type.clone()))
                    .collect();
                let mut rows = Vec::new();
                for row_id in row_ids {
                    if let Ok(data) = self.heap.get_row(row_id) {
                        if !data.is_empty() && data[0] != 0xFF {
                            rows.push(expression::deserialize_row(&data, &col_schema));
                        }
                    }
                }
                Ok(ExecutionResult {
                    columns: col_schema.iter().map(|(n, _)| n.clone()).collect(),
                    column_types: col_schema.iter().map(|(_, t)| t.clone()).collect(),
                    rows,
                    affected_rows: 0,
                    is_query: true,
                })
            }
            PhysicalPlan::Filter { input, predicate } => {
                let child = self.execute_query(&input)?;
                let child_schema: Vec<(String, ColumnType)> = child
                    .columns
                    .iter()
                    .zip(child.column_types.iter())
                    .map(|(n, t)| (n.clone(), t.clone()))
                    .collect();
                let mut rows = Vec::new();
                for row in &child.rows {
                    if let Value::Bool(true) = eval_expr(&predicate, row, &child_schema)? {
                        rows.push(row.clone());
                    }
                }
                Ok(ExecutionResult {
                    columns: child.columns,
                    column_types: child.column_types,
                    rows,
                    affected_rows: 0,
                    is_query: true,
                })
            }
            PhysicalPlan::Project {
                input,
                expressions,
                distinct,
            } => {
                let child = self.execute_query(&input)?;
                let child_schema: Vec<(String, ColumnType)> = child
                    .columns
                    .iter()
                    .zip(child.column_types.iter())
                    .map(|(n, t)| (n.clone(), t.clone()))
                    .collect();
                let mut output_schema = Vec::new();
                let mut rows = Vec::new();
                let mut seen = HashSet::new();
                for row in &child.rows {
                    let mut projected = Vec::new();
                    let mut has_star = false;
                    for expr in &expressions {
                        match expr {
                            ast::Expr::Star => {
                                has_star = true;
                                projected.extend(row.iter().cloned());
                            }
                            _ => {
                                projected.push(eval_expr(expr, row, &child_schema)?);
                            }
                        }
                    }
                    if !has_star && output_schema.is_empty() {
                        for (i, expr) in expressions.iter().enumerate() {
                            let name = match expr {
                                ast::Expr::ColumnRef {
                                    table: Some(t),
                                    column: c,
                                } => format!("{}.{}", t, c),
                                ast::Expr::ColumnRef {
                                    table: None,
                                    column: c,
                                } => c.clone(),
                                _ => format!("col_{}", i),
                            };
                            output_schema.push((name, ColumnType::Text));
                        }
                    }
                    if distinct {
                        let key: String = projected
                            .iter()
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .join("\0");
                        if !seen.insert(key) {
                            continue;
                        }
                    }
                    rows.push(projected);
                }
                if output_schema.is_empty() {
                    output_schema = child_schema;
                }
                Ok(ExecutionResult {
                    columns: output_schema.iter().map(|(n, _)| n.clone()).collect(),
                    column_types: output_schema.iter().map(|(_, t)| t.clone()).collect(),
                    rows,
                    affected_rows: 0,
                    is_query: true,
                })
            }
            PhysicalPlan::Sort { input, order_by } => {
                let mut child = self.execute_query(&input)?;
                let child_schema: Vec<(String, ColumnType)> = child
                    .columns
                    .iter()
                    .zip(child.column_types.iter())
                    .map(|(n, t)| (n.clone(), t.clone()))
                    .collect();
                child.rows.sort_by(|a, b| {
                    for item in &order_by {
                        let va = eval_expr(&item.expr, a, &child_schema).unwrap_or(Value::Null);
                        let vb = eval_expr(&item.expr, b, &child_schema).unwrap_or(Value::Null);
                        match va.partial_cmp(&vb) {
                            Some(std::cmp::Ordering::Equal) => {}
                            Some(ord) => return if item.desc { ord.reverse() } else { ord },
                            None => {}
                        }
                    }
                    std::cmp::Ordering::Equal
                });
                Ok(child)
            }
            PhysicalPlan::Limit {
                input,
                limit,
                offset,
            } => {
                let child = self.execute_query(&input)?;
                let skipped = offset.min(child.rows.len());
                let taken = limit.min(child.rows.len().saturating_sub(skipped));
                let rows = child.rows.into_iter().skip(skipped).take(taken).collect();
                Ok(ExecutionResult {
                    columns: child.columns,
                    column_types: child.column_types,
                    rows,
                    affected_rows: 0,
                    is_query: true,
                })
            }
            PhysicalPlan::NestedLoopJoin { left, right, on } => {
                let left_r = self.execute_query(&left)?;
                let right_r = self.execute_query(&right)?;
                let left_s: Vec<(String, ColumnType)> = left_r
                    .columns
                    .iter()
                    .zip(left_r.column_types.iter())
                    .map(|(n, t)| (n.clone(), t.clone()))
                    .collect();
                let right_s: Vec<(String, ColumnType)> = right_r
                    .columns
                    .iter()
                    .zip(right_r.column_types.iter())
                    .map(|(n, t)| (n.clone(), t.clone()))
                    .collect();
                let combined_s: Vec<(String, ColumnType)> =
                    left_s.iter().chain(right_s.iter()).cloned().collect();
                let mut rows = Vec::new();
                for lrow in &left_r.rows {
                    for rrow in &right_r.rows {
                        let combined: Vec<Value> =
                            lrow.iter().chain(rrow.iter()).cloned().collect();
                        if let Value::Bool(true) = eval_expr(&on, &combined, &combined_s)? {
                            rows.push(combined);
                        }
                    }
                }
                Ok(ExecutionResult {
                    columns: combined_s.iter().map(|(n, _)| n.clone()).collect(),
                    column_types: combined_s.iter().map(|(_, t)| t.clone()).collect(),
                    rows,
                    affected_rows: 0,
                    is_query: true,
                })
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Debug)]
pub struct ExecutionResult {
    pub columns: Vec<String>,
    pub column_types: Vec<ColumnType>,
    pub rows: Vec<Vec<Value>>,
    pub affected_rows: usize,
    pub is_query: bool,
}

impl ExecutionResult {
    pub fn format_table(&self) -> String {
        if self.rows.is_empty() && !self.is_query {
            return String::new();
        }
        let mut widths: Vec<usize> = self.columns.iter().map(|c| c.len()).collect();
        for row in &self.rows {
            for (i, val) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(val.to_string().len());
                }
            }
        }
        let mut out = String::new();
        for (i, col) in self.columns.iter().enumerate() {
            if i > 0 {
                out += " | ";
            }
            out += &format!("{:width$}", col, width = widths[i]);
        }
        out += "\n";
        for (i, w) in widths.iter().enumerate() {
            if i > 0 {
                out += "-+-";
            }
            out += &"-".repeat(*w);
        }
        out += "\n";
        for row in &self.rows {
            for (i, val) in row.iter().enumerate() {
                if i > 0 {
                    out += " | ";
                }
                out += &format!("{:width$}", val.to_string(), width = widths[i]);
            }
            out += "\n";
        }
        out
    }

    pub fn format_csv(&self) -> String {
        let mut wtr = csv::Writer::from_writer(vec![]);
        let _ = wtr.write_record(&self.columns);
        for row in &self.rows {
            let records: Vec<String> = row.iter().map(|v| v.to_string()).collect();
            let _ = wtr.write_record(&records);
        }
        String::from_utf8(wtr.into_inner().unwrap_or_default()).unwrap_or_default()
    }

    pub fn format_json(&self) -> String {
        let mut objects = Vec::new();
        for row in &self.rows {
            let mut obj = serde_json::Map::new();
            for (i, col) in self.columns.iter().enumerate() {
                if let Some(val) = row.get(i) {
                    let json_val = match val {
                        Value::Null => serde_json::Value::Null,
                        Value::Int(v) => serde_json::Value::Number((*v).into()),
                        Value::Real(v) => serde_json::Number::from_f64(*v)
                            .map(serde_json::Value::Number)
                            .unwrap_or(serde_json::Value::Null),
                        Value::Text(v) => serde_json::Value::String(v.clone()),
                        Value::Bool(v) => serde_json::Value::Bool(*v),
                    };
                    obj.insert(col.clone(), json_val);
                }
            }
            objects.push(serde_json::Value::Object(obj));
        }
        serde_json::to_string_pretty(&objects).unwrap_or_else(|_| "[]".into())
    }
}
