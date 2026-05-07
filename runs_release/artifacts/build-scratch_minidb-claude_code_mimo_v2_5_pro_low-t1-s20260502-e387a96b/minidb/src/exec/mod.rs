pub mod operators;

use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;
use crate::plan::physical::PhysicalPlan;
use crate::storage::btree::BTreeIndex;
use crate::storage::heap::{self, HeapFile};
use crate::storage::wal::Wal;
use crate::storage::{PageId, Pager};
use crate::txn::TransactionManager;
use crate::types::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct QueryContext<'a> {
    pub catalog: &'a mut Catalog,
    pub heaps: &'a mut HashMap<String, HeapFile>,
    pub indexes: &'a mut HashMap<String, BTreeIndex>,
    pub txn: &'a mut TransactionManager,
    pub pager: Arc<Mutex<Pager>>,
}

pub struct Executor;

impl Executor {
    pub fn execute_plan(plan: PhysicalPlan, ctx: &mut QueryContext) -> Result<QueryResult> {
        match plan {
            PhysicalPlan::SeqScan {
                table_name, schema, ..
            } => {
                let rows = if let Some(heap) = ctx.heaps.get(&table_name) {
                    heap.scan_rows()?
                } else {
                    return Err(MiniDbError::TableNotFound(table_name));
                };
                let columns = schema.columns.iter().map(|c| c.name.clone()).collect();
                Ok(QueryResult {
                    columns,
                    rows,
                    rows_affected: 0,
                })
            }
            PhysicalPlan::IndexScan {
                table_name,
                schema,
                index_id,
                ..
            } => {
                // Find index by ID
                let idx_entry = ctx
                    .catalog
                    .data
                    .indexes
                    .values()
                    .find(|e| e.index_id == index_id)
                    .cloned();
                let rids = if let Some(entry) = idx_entry {
                    if let Some(idx) = ctx.indexes.get(&entry.schema.name.to_lowercase()) {
                        // For now, fall back to seq scan
                        vec![]
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                };

                let heap = ctx
                    .heaps
                    .get(&table_name)
                    .ok_or_else(|| MiniDbError::TableNotFound(table_name.clone()))?;
                let rows = if rids.is_empty() {
                    heap.scan_rows()?
                } else {
                    let all_with_rids = heap.scan_rows_with_rids()?;
                    let rid_set: std::collections::HashSet<u64> = rids.into_iter().collect();
                    all_with_rids
                        .into_iter()
                        .filter(|(rid, _)| rid_set.contains(rid))
                        .map(|(_, row)| row)
                        .collect()
                };
                let columns = schema.columns.iter().map(|c| c.name.clone()).collect();
                Ok(QueryResult {
                    columns,
                    rows,
                    rows_affected: 0,
                })
            }
            PhysicalPlan::Filter { input, predicate } => {
                let input_result = Self::execute_plan(*input, ctx)?;
                let col_map = build_col_map(&input_result.columns);
                let mut filtered = Vec::new();
                for row in &input_result.rows {
                    let val = eval_expr(&predicate, &col_map, row, None)?;
                    if val.is_truthy() == Some(true) {
                        filtered.push(row.clone());
                    }
                }
                Ok(QueryResult {
                    columns: input_result.columns,
                    rows: filtered,
                    rows_affected: 0,
                })
            }
            PhysicalPlan::Project { input, items } => {
                let input_result = Self::execute_plan(*input, ctx)?;
                let col_map = build_col_map(&input_result.columns);
                let mut proj_columns = Vec::new();
                let mut proj_exprs = Vec::new();

                for item in &items {
                    match item {
                        SelectItem::Star => {
                            for col in &input_result.columns {
                                proj_columns.push(col.clone());
                                proj_exprs.push(Expr::ColumnRef {
                                    table: None,
                                    name: col.clone(),
                                });
                            }
                        }
                        SelectItem::Expr { expr, alias } => {
                            let name = alias.clone().unwrap_or_else(|| expr_name(expr));
                            proj_columns.push(name);
                            proj_exprs.push(expr.clone());
                        }
                    }
                }

                let mut proj_rows = Vec::new();
                for row in &input_result.rows {
                    let mut new_row = Vec::new();
                    for expr in &proj_exprs {
                        new_row.push(eval_expr(expr, &col_map, row, None)?);
                    }
                    proj_rows.push(new_row);
                }
                Ok(QueryResult {
                    columns: proj_columns,
                    rows: proj_rows,
                    rows_affected: 0,
                })
            }
            PhysicalPlan::NestedLoopJoin {
                left, right, on, ..
            } => {
                let left_result = Self::execute_plan(*left, ctx)?;
                let right_result = Self::execute_plan(*right, ctx)?;

                let mut join_columns = left_result.columns.clone();
                join_columns.extend(right_result.columns.clone());

                let mut join_rows = Vec::new();
                for left_row in &left_result.rows {
                    for right_row in &right_result.rows {
                        let mut combined_row = left_row.clone();
                        combined_row.extend(right_row.iter().cloned());

                        let keep = if let Some(ref pred) = on {
                            let val = eval_expr(
                                pred,
                                &build_col_map(&join_columns),
                                &combined_row,
                                None,
                            )?;
                            val.is_truthy() == Some(true)
                        } else {
                            true
                        };

                        if keep {
                            join_rows.push(combined_row);
                        }
                    }
                }
                Ok(QueryResult {
                    columns: join_columns,
                    rows: join_rows,
                    rows_affected: 0,
                })
            }
            PhysicalPlan::Sort { input, keys } => {
                let input_result = Self::execute_plan(*input, ctx)?;
                let col_map = build_col_map(&input_result.columns);
                let mut rows = input_result.rows;

                rows.sort_by(|a, b| {
                    for key in &keys {
                        let va = eval_expr(&key.expr, &col_map, a, None).unwrap_or(Value::Null);
                        let vb = eval_expr(&key.expr, &col_map, b, None).unwrap_or(Value::Null);
                        let ord = va.compare(&vb).unwrap_or(std::cmp::Ordering::Equal);
                        if ord != std::cmp::Ordering::Equal {
                            return if key.asc { ord } else { ord.reverse() };
                        }
                    }
                    std::cmp::Ordering::Equal
                });
                Ok(QueryResult {
                    columns: input_result.columns,
                    rows,
                    rows_affected: 0,
                })
            }
            PhysicalPlan::Limit {
                input,
                limit,
                offset,
            } => {
                let input_result = Self::execute_plan(*input, ctx)?;
                let offset_val = offset.map_or(0, |e| {
                    eval_expr_static(&e)
                        .ok()
                        .and_then(|v| match v {
                            Value::Int(n) => Some(n.max(0) as usize),
                            _ => None,
                        })
                        .unwrap_or(0)
                });
                let limit_val = limit.map_or(usize::MAX, |e| {
                    eval_expr_static(&e)
                        .ok()
                        .and_then(|v| match v {
                            Value::Int(n) => Some(n.max(0) as usize),
                            _ => None,
                        })
                        .unwrap_or(usize::MAX)
                });
                let rows: Vec<Row> = input_result
                    .rows
                    .into_iter()
                    .skip(offset_val)
                    .take(limit_val)
                    .collect();
                Ok(QueryResult {
                    columns: input_result.columns,
                    rows,
                    rows_affected: 0,
                })
            }
            PhysicalPlan::Values { rows: row_exprs } => {
                let mut rows = Vec::new();
                for row_expr in &row_exprs {
                    let mut row = Vec::new();
                    for expr in row_expr {
                        row.push(eval_expr_static(expr)?);
                    }
                    rows.push(row);
                }
                Ok(QueryResult {
                    columns: vec![],
                    rows,
                    rows_affected: 0,
                })
            }
            PhysicalPlan::Insert {
                table_name,
                columns,
                values,
                ..
            } => Self::execute_insert(&table_name, columns, values, ctx),
            PhysicalPlan::Update {
                table_name,
                assignments,
                filter,
                ..
            } => {
                let predicate = match filter {
                    Some(boxed) => match *boxed {
                        PhysicalPlan::Filter { predicate, .. } => Some(predicate),
                        _ => None,
                    },
                    None => None,
                };
                Self::execute_update(&table_name, assignments, predicate, ctx)
            }
            PhysicalPlan::Delete {
                table_name, filter, ..
            } => {
                // Extract predicate from filter plan if present
                let predicate = match filter {
                    Some(boxed) => match *boxed {
                        PhysicalPlan::Filter { predicate, .. } => Some(predicate),
                        _ => None,
                    },
                    None => None,
                };
                Self::execute_delete(&table_name, predicate, ctx)
            }
        }
    }

    fn execute_insert(
        table_name: &str,
        columns: Option<Vec<String>>,
        values: Vec<Vec<Expr>>,
        ctx: &mut QueryContext,
    ) -> Result<QueryResult> {
        let table_entry = ctx
            .catalog
            .get_table(table_name)
            .ok_or_else(|| MiniDbError::TableNotFound(table_name.to_string()))?
            .clone();
        let schema = table_entry.schema.clone();

        let col_indices: Vec<usize> = if let Some(ref cols) = columns {
            cols.iter()
                .map(|c| {
                    schema
                        .columns
                        .iter()
                        .position(|sc| sc.name.eq_ignore_ascii_case(c))
                        .ok_or_else(|| MiniDbError::ColumnNotFound(c.clone()))
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            (0..schema.columns.len()).collect()
        };

        let txn_id = ctx.txn.ensure_transaction()?;
        let was_auto = !ctx.txn.in_transaction();

        let mut rows_inserted = 0u64;
        for value_row in &values {
            let mut full_row: Vec<Value> = schema
                .columns
                .iter()
                .map(|c| c.default.clone().unwrap_or(Value::Null))
                .collect();

            for (i, expr) in value_row.iter().enumerate() {
                let val = eval_expr_static(expr)?;
                let idx = col_indices[i];
                full_row[idx] = val;
            }

            // Check NOT NULL constraints
            for (i, col) in schema.columns.iter().enumerate() {
                if col.not_null && full_row[i].is_null() {
                    if col.default.is_some() && (columns.is_none() || !col_indices.contains(&i)) {
                        // Default will fill it - but we already set it, so check again
                    } else {
                        return Err(MiniDbError::ConstraintViolation(format!(
                            "column '{}' cannot be NULL",
                            col.name
                        )));
                    }
                }
            }

            // Check PRIMARY KEY uniqueness
            if let Some(pk_idx) = schema.columns.iter().position(|c| c.primary_key) {
                let pk_val = full_row[pk_idx].clone();
                let heap = ctx
                    .heaps
                    .get(table_name)
                    .ok_or_else(|| MiniDbError::TableNotFound(table_name.to_string()))?;
                let existing = heap.scan_rows()?;
                for existing_row in &existing {
                    if existing_row[pk_idx] == pk_val {
                        return Err(MiniDbError::ConstraintViolation(
                            "duplicate key value violates primary key constraint".into(),
                        ));
                    }
                }
            }

            // Check UNIQUE constraints
            for (i, col) in schema.columns.iter().enumerate() {
                if col.unique && !col.primary_key {
                    let val = full_row[i].clone();
                    let heap = ctx
                        .heaps
                        .get(table_name)
                        .ok_or_else(|| MiniDbError::TableNotFound(table_name.to_string()))?;
                    let existing = heap.scan_rows()?;
                    for existing_row in &existing {
                        if existing_row[i] == val {
                            return Err(MiniDbError::ConstraintViolation(format!(
                                "duplicate key value violates unique constraint on column '{}'",
                                col.name
                            )));
                        }
                    }
                }
            }

            let heap = ctx
                .heaps
                .get_mut(table_name)
                .ok_or_else(|| MiniDbError::TableNotFound(table_name.to_string()))?;
            heap.insert_row(&full_row, ctx.txn.wal(), txn_id)?;
            rows_inserted += 1;
        }

        ctx.txn.auto_commit_if_needed(was_auto)?;

        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: rows_inserted,
        })
    }

    fn execute_update(
        table_name: &str,
        assignments: Vec<(String, Expr)>,
        predicate: Option<Expr>,
        ctx: &mut QueryContext,
    ) -> Result<QueryResult> {
        let table_entry = ctx
            .catalog
            .get_table(table_name)
            .ok_or_else(|| MiniDbError::TableNotFound(table_name.to_string()))?
            .clone();
        let schema = table_entry.schema.clone();
        let columns: Vec<String> = schema.columns.iter().map(|c| c.name.clone()).collect();
        let col_map = build_col_map(&columns);

        // Collect rows and rids before mutating
        let all_with_rids = {
            let heap = ctx
                .heaps
                .get(table_name)
                .ok_or_else(|| MiniDbError::TableNotFound(table_name.to_string()))?;
            heap.scan_rows_with_rids()?
        };

        let txn_id = ctx.txn.ensure_transaction()?;
        let was_auto = !ctx.txn.in_transaction();

        let mut rows_affected = 0u64;
        for (rid, row) in &all_with_rids {
            // Apply filter predicate if present
            if let Some(ref pred) = predicate {
                let val = eval_expr(pred, &col_map, row, None)?;
                if val.is_truthy() != Some(true) {
                    continue;
                }
            }
            let mut new_row = row.clone();
            for (col_name, expr) in &assignments {
                let col_idx = columns
                    .iter()
                    .position(|c| c.eq_ignore_ascii_case(col_name))
                    .ok_or_else(|| MiniDbError::ColumnNotFound(col_name.clone()))?;
                let val = eval_expr(expr, &col_map, row, None)?;
                new_row[col_idx] = val;
            }

            let heap = ctx.heaps.get_mut(table_name).unwrap();
            heap.update_by_rid(*rid, &new_row, ctx.txn.wal(), txn_id)?;
            rows_affected += 1;
        }

        ctx.txn.auto_commit_if_needed(was_auto)?;

        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected,
        })
    }

    fn execute_delete(
        table_name: &str,
        predicate: Option<Expr>,
        ctx: &mut QueryContext,
    ) -> Result<QueryResult> {
        let table_entry = ctx
            .catalog
            .get_table(table_name)
            .ok_or_else(|| MiniDbError::TableNotFound(table_name.to_string()))?
            .clone();
        let columns: Vec<String> = table_entry
            .schema
            .columns
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let col_map = build_col_map(&columns);

        // Collect rids before mutating
        let all_with_rids = {
            let heap = ctx
                .heaps
                .get(table_name)
                .ok_or_else(|| MiniDbError::TableNotFound(table_name.to_string()))?;
            heap.scan_rows_with_rids()?
        };

        let txn_id = ctx.txn.ensure_transaction()?;
        let was_auto = !ctx.txn.in_transaction();

        let mut rows_affected = 0u64;
        for (rid, row) in &all_with_rids {
            // Apply filter predicate if present
            if let Some(ref pred) = predicate {
                let val = eval_expr(pred, &col_map, row, None)?;
                if val.is_truthy() != Some(true) {
                    continue;
                }
            }
            let heap = ctx.heaps.get_mut(table_name).unwrap();
            heap.delete_by_rid(*rid, ctx.txn.wal(), txn_id)?;
            rows_affected += 1;
        }

        ctx.txn.auto_commit_if_needed(was_auto)?;

        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected,
        })
    }
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    pub rows_affected: u64,
}

pub fn build_col_map(columns: &[String]) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for (i, col) in columns.iter().enumerate() {
        map.insert(col.to_lowercase(), i);
    }
    map
}

pub fn eval_expr(
    expr: &Expr,
    col_map: &HashMap<String, usize>,
    row: &Row,
    right_row: Option<&Row>,
) -> Result<Value> {
    match expr {
        Expr::Literal(v) => Ok(v.clone()),
        Expr::ColumnRef { table, name } => {
            let key = name.to_lowercase();
            if let Some(&idx) = col_map.get(&key) {
                if idx < row.len() {
                    return Ok(row[idx].clone());
                }
            }
            if let Some(tbl) = table {
                let qualified = format!("{}.{}", tbl.to_lowercase(), key);
                if let Some(&idx) = col_map.get(&qualified) {
                    if idx < row.len() {
                        return Ok(row[idx].clone());
                    }
                }
            }
            Err(MiniDbError::ColumnNotFound(name.clone()))
        }
        Expr::BinaryOp { left, op, right } => {
            let lv = eval_expr(left, col_map, row, right_row)?;
            let rv = eval_expr(right, col_map, row, right_row)?;
            match op {
                BinaryOp::Eq => Ok(lv.eq_check(&rv)),
                BinaryOp::Neq => Ok(lv.neq_check(&rv)),
                BinaryOp::Lt => Ok(lv.lt_check(&rv)),
                BinaryOp::Le => Ok(lv.le_check(&rv)),
                BinaryOp::Gt => Ok(lv.gt_check(&rv)),
                BinaryOp::Ge => Ok(lv.ge_check(&rv)),
                BinaryOp::Add => lv.add(&rv),
                BinaryOp::Sub => lv.sub(&rv),
                BinaryOp::Mul => lv.mul(&rv),
                BinaryOp::Div => lv.div(&rv),
                BinaryOp::And => Ok(lv.and(&rv)),
                BinaryOp::Or => Ok(lv.or(&rv)),
                BinaryOp::Concat => lv.concat(&rv),
            }
        }
        Expr::UnaryOp { op, operand } => {
            let v = eval_expr(operand, col_map, row, right_row)?;
            match op {
                UnaryOp::Neg => v.negate(),
                UnaryOp::Not => Ok(v.not()),
            }
        }
        Expr::FunctionCall { name, args } => eval_function(name, args, col_map, row, right_row),
        Expr::IsNull(inner) => {
            let v = eval_expr(inner, col_map, row, right_row)?;
            Ok(Value::Bool(v.is_null()))
        }
        Expr::IsNotNull(inner) => {
            let v = eval_expr(inner, col_map, row, right_row)?;
            Ok(Value::Bool(!v.is_null()))
        }
        Expr::InList { expr, list } => {
            let v = eval_expr(expr, col_map, row, right_row)?;
            for item in list {
                let iv = eval_expr(item, col_map, row, right_row)?;
                if v.eq_check(&iv) == Value::Bool(true) {
                    return Ok(Value::Bool(true));
                }
            }
            Ok(Value::Bool(false))
        }
        Expr::Between { expr, low, high } => {
            let v = eval_expr(expr, col_map, row, right_row)?;
            let lo = eval_expr(low, col_map, row, right_row)?;
            let hi = eval_expr(high, col_map, row, right_row)?;
            Ok(Value::Bool(
                v.ge_check(&lo) == Value::Bool(true) && v.le_check(&hi) == Value::Bool(true),
            ))
        }
        Expr::Case {
            operands,
            else_expr,
        } => {
            for (cond, result) in operands {
                let cv = eval_expr(cond, col_map, row, right_row)?;
                if cv.is_truthy() == Some(true) {
                    return eval_expr(result, col_map, row, right_row);
                }
            }
            if let Some(else_e) = else_expr {
                eval_expr(else_e, col_map, row, right_row)
            } else {
                Ok(Value::Null)
            }
        }
        Expr::Subquery(_) => Err(MiniDbError::NotImplemented(
            "subqueries not supported".into(),
        )),
    }
}

fn eval_expr_static(expr: &Expr) -> Result<Value> {
    let empty: Row = vec![];
    eval_expr(expr, &HashMap::new(), &empty, None)
}

fn eval_function(
    name: &str,
    args: &[Expr],
    col_map: &HashMap<String, usize>,
    row: &Row,
    right_row: Option<&Row>,
) -> Result<Value> {
    match name.to_uppercase().as_str() {
        "LENGTH" | "LEN" => {
            let v = eval_expr(&args[0], col_map, row, right_row)?;
            match v {
                Value::Text(s) => Ok(Value::Int(s.len() as i64)),
                Value::Null => Ok(Value::Null),
                _ => Err(MiniDbError::TypeMismatch("LENGTH requires TEXT".into())),
            }
        }
        "LOWER" => {
            let v = eval_expr(&args[0], col_map, row, right_row)?;
            match v {
                Value::Text(s) => Ok(Value::Text(s.to_lowercase())),
                Value::Null => Ok(Value::Null),
                _ => Err(MiniDbError::TypeMismatch("LOWER requires TEXT".into())),
            }
        }
        "UPPER" => {
            let v = eval_expr(&args[0], col_map, row, right_row)?;
            match v {
                Value::Text(s) => Ok(Value::Text(s.to_uppercase())),
                Value::Null => Ok(Value::Null),
                _ => Err(MiniDbError::TypeMismatch("UPPER requires TEXT".into())),
            }
        }
        "SUBSTR" | "SUBSTRING" => {
            let s = eval_expr(&args[0], col_map, row, right_row)?;
            let start = eval_expr(&args[1], col_map, row, right_row)?;
            let len = if args.len() > 2 {
                Some(eval_expr(&args[2], col_map, row, right_row)?)
            } else {
                None
            };
            match (s, start) {
                (Value::Text(s), Value::Int(pos)) => {
                    let start_idx = if pos > 0 { (pos - 1) as usize } else { 0 };
                    let end_idx = if let Some(Value::Int(l)) = len {
                        std::cmp::min(start_idx + l as usize, s.len())
                    } else {
                        s.len()
                    };
                    if start_idx >= s.len() {
                        Ok(Value::Text(String::new()))
                    } else {
                        Ok(Value::Text(s[start_idx..end_idx].to_string()))
                    }
                }
                (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                _ => Err(MiniDbError::TypeMismatch(
                    "SUBSTR requires (TEXT, INT)".into(),
                )),
            }
        }
        "ABS" => {
            let v = eval_expr(&args[0], col_map, row, right_row)?;
            match v {
                Value::Int(n) => Ok(Value::Int(n.abs())),
                Value::Real(r) => Ok(Value::Real(r.abs())),
                Value::Null => Ok(Value::Null),
                _ => Err(MiniDbError::TypeMismatch("ABS requires numeric".into())),
            }
        }
        "COALESCE" => {
            for arg in args {
                let v = eval_expr(arg, col_map, row, right_row)?;
                if !v.is_null() {
                    return Ok(v);
                }
            }
            Ok(Value::Null)
        }
        "COUNT" => {
            if args.len() == 1 {
                if let Expr::ColumnRef { name, .. } = &args[0] {
                    if name == "*" {
                        return Ok(Value::Int(row.len() as i64));
                    }
                }
                let v = eval_expr(&args[0], col_map, row, right_row)?;
                Ok(Value::Int(if v.is_null() { 0 } else { 1 }))
            } else {
                Ok(Value::Int(1))
            }
        }
        _ => Err(MiniDbError::RuntimeError(format!(
            "unknown function: {name}"
        ))),
    }
}

fn expr_name(expr: &Expr) -> String {
    match expr {
        Expr::ColumnRef { name, .. } => name.clone(),
        Expr::FunctionCall { name, .. } => name.clone(),
        Expr::Literal(v) => format!("{v}"),
        _ => "?column?".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_arithmetic() {
        let col_map = HashMap::new();
        let row: Row = vec![];
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Literal(Value::Int(3))),
            op: BinaryOp::Add,
            right: Box::new(Expr::Literal(Value::Int(4))),
        };
        assert_eq!(
            eval_expr(&expr, &col_map, &row, None).unwrap(),
            Value::Int(7)
        );
    }

    #[test]
    fn test_eval_column_ref() {
        let mut col_map = HashMap::new();
        col_map.insert("x".to_string(), 0);
        let row = vec![Value::Int(42)];
        let expr = Expr::ColumnRef {
            table: None,
            name: "x".to_string(),
        };
        assert_eq!(
            eval_expr(&expr, &col_map, &row, None).unwrap(),
            Value::Int(42)
        );
    }

    #[test]
    fn test_eval_functions() {
        let col_map = HashMap::new();
        let row: Row = vec![];
        let expr = Expr::FunctionCall {
            name: "LENGTH".to_string(),
            args: vec![Expr::Literal(Value::Text("hello".to_string()))],
        };
        assert_eq!(
            eval_expr(&expr, &col_map, &row, None).unwrap(),
            Value::Int(5)
        );
    }

    #[test]
    fn test_eval_coalesce() {
        let col_map = HashMap::new();
        let row: Row = vec![];
        let expr = Expr::FunctionCall {
            name: "COALESCE".to_string(),
            args: vec![Expr::Literal(Value::Null), Expr::Literal(Value::Int(42))],
        };
        assert_eq!(
            eval_expr(&expr, &col_map, &row, None).unwrap(),
            Value::Int(42)
        );
    }

    #[test]
    fn test_eval_null_propagation() {
        let col_map = HashMap::new();
        let row: Row = vec![];
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Literal(Value::Null)),
            op: BinaryOp::Add,
            right: Box::new(Expr::Literal(Value::Int(1))),
        };
        assert!(eval_expr(&expr, &col_map, &row, None).unwrap().is_null());
    }
}
