use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;
use crate::plan::logical::ScanOperator;
use crate::storage::heap::HeapFile;
use crate::types::{Row, RowId, Value};
use std::collections::HashMap;

pub enum ExecResult {
    Rows {
        columns: Vec<String>,
        rows: Vec<Row>,
    },
    Changed {
        rows_affected: usize,
    },
}

pub trait PhysicalOperator {
    fn open(&mut self, ctx: &mut ExecContext) -> Result<()>;
    fn next(&mut self, ctx: &mut ExecContext) -> Result<Option<(RowId, Row)>>;
    fn close(&mut self) -> Result<()>;
    fn schema(&self) -> Vec<String>;
}

pub struct ExecContext<'a> {
    pub catalog: &'a mut Catalog,
    pub heap: &'a mut HeapFile,
}

pub fn evaluate_expr(expr: &Expr, row: &Row, columns: &[String]) -> Result<Value> {
    match expr {
        Expr::Literal(lit) => Ok(lit.to_value()),
        Expr::ColumnRef { table: _, column } => {
            let col_lower = column.to_lowercase();
            for (i, name) in columns.iter().enumerate() {
                if name.eq_ignore_ascii_case(&col_lower) {
                    return Ok(row.get(i).cloned().unwrap_or(Value::Null));
                }
            }
            Err(MiniDbError::Runtime(format!(
                "column '{}' not found",
                column
            )))
        }
        Expr::Star => Err(MiniDbError::Runtime(
            "* cannot be used in expressions".into(),
        )),
        Expr::Function { name, args } => {
            let func_name = name.to_uppercase();
            match func_name.as_str() {
                "LENGTH" => {
                    let val = evaluate_expr(&args[0], row, columns)?;
                    match val {
                        Value::Text(s) => Ok(Value::Int(s.len() as i64)),
                        Value::Null => Ok(Value::Null),
                        _ => Ok(Value::Int(val.to_string().len() as i64)),
                    }
                }
                "LOWER" => {
                    let val = evaluate_expr(&args[0], row, columns)?;
                    match val {
                        Value::Text(s) => Ok(Value::Text(s.to_lowercase())),
                        Value::Null => Ok(Value::Null),
                        _ => Ok(Value::Text(val.to_string().to_lowercase())),
                    }
                }
                "UPPER" => {
                    let val = evaluate_expr(&args[0], row, columns)?;
                    match val {
                        Value::Text(s) => Ok(Value::Text(s.to_uppercase())),
                        Value::Null => Ok(Value::Null),
                        _ => Ok(Value::Text(val.to_string().to_uppercase())),
                    }
                }
                "SUBSTR" => {
                    let val = evaluate_expr(&args[0], row, columns)?;
                    let start =
                        evaluate_expr(&args[1], row, columns)?.as_int().unwrap_or(0) as usize;
                    let len = if args.len() > 2 {
                        evaluate_expr(&args[2], row, columns)?.as_int().unwrap_or(0) as usize
                    } else {
                        usize::MAX
                    };
                    match val {
                        Value::Text(s) => {
                            let start = start.saturating_sub(1);
                            let end = (start + len).min(s.len());
                            Ok(Value::Text(s[start..end].to_string()))
                        }
                        Value::Null => Ok(Value::Null),
                        _ => Ok(Value::Text(String::new())),
                    }
                }
                "ABS" => {
                    let val = evaluate_expr(&args[0], row, columns)?;
                    match val {
                        Value::Int(v) => Ok(Value::Int(v.abs())),
                        Value::Real(v) => Ok(Value::Real(crate::types::RealOrd(v.0.abs()))),
                        Value::Null => Ok(Value::Null),
                        _ => Err(MiniDbError::Runtime(format!(
                            "ABS: unsupported type {}",
                            val.type_name()
                        ))),
                    }
                }
                "COALESCE" => {
                    for arg in args {
                        let val = evaluate_expr(arg, row, columns)?;
                        if !val.is_null() {
                            return Ok(val);
                        }
                    }
                    Ok(Value::Null)
                }
                _ => Err(MiniDbError::Runtime(format!("unknown function: {}", name))),
            }
        }
        Expr::BinaryOp { op, left, right } => {
            match op {
                BinOp::IsNull => {
                    let val = evaluate_expr(left, row, columns)?;
                    return Ok(Value::Bool(val.is_null()));
                }
                BinOp::IsNotNull => {
                    let val = evaluate_expr(left, row, columns)?;
                    return Ok(Value::Bool(!val.is_null()));
                }
                _ => {}
            }

            let l = evaluate_expr(left, row, columns)?;
            let r = evaluate_expr(right, row, columns)?;

            match op {
                BinOp::Eq => Ok(Value::Bool(l.sql_eq(&r).unwrap_or(false))),
                BinOp::Ne => Ok(Value::Bool(l.sql_ne(&r).unwrap_or(false))),
                BinOp::Lt => Ok(Value::Bool(l.sql_lt(&r).unwrap_or(false))),
                BinOp::Le => Ok(Value::Bool(l.sql_le(&r).unwrap_or(false))),
                BinOp::Gt => Ok(Value::Bool(l.sql_gt(&r).unwrap_or(false))),
                BinOp::Ge => Ok(Value::Bool(l.sql_ge(&r).unwrap_or(false))),
                BinOp::And => {
                    let lv = l.as_bool();
                    let rv = r.as_bool();
                    match (lv, rv) {
                        (Some(false), _) | (_, Some(false)) => Ok(Value::Bool(false)),
                        (Some(true), Some(true)) => Ok(Value::Bool(true)),
                        _ => Ok(Value::Null),
                    }
                }
                BinOp::Or => {
                    let lv = l.as_bool();
                    let rv = r.as_bool();
                    match (lv, rv) {
                        (Some(true), _) | (_, Some(true)) => Ok(Value::Bool(true)),
                        (Some(false), Some(false)) => Ok(Value::Bool(false)),
                        _ => Ok(Value::Null),
                    }
                }
                BinOp::Add => {
                    if l.is_null() || r.is_null() {
                        Ok(Value::Null)
                    } else if let (Some(a), Some(b)) = (l.as_int(), r.as_int()) {
                        Ok(Value::Int(a + b))
                    } else if let (Some(a), Some(b)) = (l.as_real(), r.as_real()) {
                        Ok(Value::Real(crate::types::RealOrd(a + b)))
                    } else {
                        Ok(Value::Null)
                    }
                }
                BinOp::Sub => {
                    if l.is_null() || r.is_null() {
                        Ok(Value::Null)
                    } else if let (Some(a), Some(b)) = (l.as_int(), r.as_int()) {
                        Ok(Value::Int(a - b))
                    } else if let (Some(a), Some(b)) = (l.as_real(), r.as_real()) {
                        Ok(Value::Real(crate::types::RealOrd(a - b)))
                    } else {
                        Ok(Value::Null)
                    }
                }
                BinOp::Mul => {
                    if l.is_null() || r.is_null() {
                        Ok(Value::Null)
                    } else if let (Some(a), Some(b)) = (l.as_int(), r.as_int()) {
                        Ok(Value::Int(a * b))
                    } else if let (Some(a), Some(b)) = (l.as_real(), r.as_real()) {
                        Ok(Value::Real(crate::types::RealOrd(a * b)))
                    } else {
                        Ok(Value::Null)
                    }
                }
                BinOp::Div => {
                    if l.is_null() || r.is_null() {
                        Ok(Value::Null)
                    } else if let (Some(a), Some(b)) = (l.as_int(), r.as_int()) {
                        if b == 0 {
                            Err(MiniDbError::Runtime("division by zero".into()))
                        } else {
                            Ok(Value::Int(a / b))
                        }
                    } else if let (Some(a), Some(b)) = (l.as_real(), r.as_real()) {
                        if b == 0.0 {
                            Err(MiniDbError::Runtime("division by zero".into()))
                        } else {
                            Ok(Value::Real(crate::types::RealOrd(a / b)))
                        }
                    } else {
                        Ok(Value::Null)
                    }
                }
                BinOp::Mod => {
                    if l.is_null() || r.is_null() {
                        Ok(Value::Null)
                    } else if let (Some(a), Some(b)) = (l.as_int(), r.as_int()) {
                        if b == 0 {
                            Err(MiniDbError::Runtime("division by zero".into()))
                        } else {
                            Ok(Value::Int(a % b))
                        }
                    } else {
                        Ok(Value::Null)
                    }
                }
                BinOp::Concat => {
                    if l.is_null() || r.is_null() {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Text(format!("{}{}", l, r)))
                    }
                }
                _ => Err(MiniDbError::Runtime(format!(
                    "unsupported operator: {:?}",
                    op
                ))),
            }
        }
        Expr::UnaryOp { op, expr } => {
            let val = evaluate_expr(expr, row, columns)?;
            match op {
                UnaryOp::Not => match val.as_bool() {
                    Some(b) => Ok(Value::Bool(!b)),
                    None => Ok(Value::Null),
                },
                UnaryOp::Neg => match val {
                    Value::Int(v) => Ok(Value::Int(-v)),
                    Value::Real(v) => Ok(Value::Real(crate::types::RealOrd(-v.0))),
                    Value::Null => Ok(Value::Null),
                    _ => Err(MiniDbError::Runtime(format!(
                        "cannot negate {}",
                        val.type_name()
                    ))),
                },
            }
        }
    }
}

// SeqScan operator
pub struct SeqScanOperator {
    table: String,
    rows: Vec<(RowId, Row)>,
    pos: usize,
}

impl SeqScanOperator {
    pub fn new(table: String) -> Self {
        SeqScanOperator {
            table,
            rows: Vec::new(),
            pos: 0,
        }
    }
}

impl PhysicalOperator for SeqScanOperator {
    fn open(&mut self, ctx: &mut ExecContext) -> Result<()> {
        self.rows = ctx.heap.scan_table(&self.table)?;
        self.pos = 0;
        Ok(())
    }
    fn next(&mut self, _ctx: &mut ExecContext) -> Result<Option<(RowId, Row)>> {
        if self.pos < self.rows.len() {
            let result = self.rows[self.pos].clone();
            self.pos += 1;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> Vec<String> {
        vec![]
    }
}

// IndexScan operator
pub struct IndexScanOperator {
    table: String,
    column: String,
    op: ScanOperator,
    value: Value,
    cached_rows: Vec<(RowId, Row)>,
    pos: usize,
}

impl IndexScanOperator {
    pub fn new(table: String, column: String, op: ScanOperator, value: Value) -> Self {
        IndexScanOperator {
            table,
            column,
            op,
            value,
            cached_rows: Vec::new(),
            pos: 0,
        }
    }
}

impl PhysicalOperator for IndexScanOperator {
    fn open(&mut self, ctx: &mut ExecContext) -> Result<()> {
        let row_ids = {
            let idx = ctx
                .catalog
                .find_index(&self.table, &self.column)
                .ok_or_else(|| {
                    MiniDbError::Runtime(format!("no index on {}.{}", self.table, self.column))
                })?;
            idx.scan(scan_op_to_btree(&self.op), &self.value)
        };

        let all_rows = ctx.heap.scan_table(&self.table)?;
        let row_map: HashMap<RowId, Row> = all_rows.into_iter().collect();
        self.cached_rows = row_ids
            .iter()
            .filter_map(|rid| row_map.get(rid).map(|r| (*rid, r.clone())))
            .collect();
        self.pos = 0;
        Ok(())
    }
    fn next(&mut self, _ctx: &mut ExecContext) -> Result<Option<(RowId, Row)>> {
        if self.pos < self.cached_rows.len() {
            let result = self.cached_rows[self.pos].clone();
            self.pos += 1;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> Vec<String> {
        vec![]
    }
}

fn scan_op_to_btree(op: &ScanOperator) -> crate::storage::btree::ScanOp {
    match op {
        ScanOperator::Eq => crate::storage::btree::ScanOp::Eq,
        ScanOperator::Gt => crate::storage::btree::ScanOp::Gt,
        ScanOperator::Ge => crate::storage::btree::ScanOp::Ge,
        ScanOperator::Lt => crate::storage::btree::ScanOp::Lt,
        ScanOperator::Le => crate::storage::btree::ScanOp::Le,
    }
}

// Filter operator
pub struct FilterOperator {
    _predicate: Expr,
    columns: Vec<String>,
    buffer: Vec<(RowId, Row)>,
    pos: usize,
}

impl FilterOperator {
    pub fn new(predicate: Expr, _input: Box<dyn PhysicalOperator>, columns: Vec<String>) -> Self {
        FilterOperator {
            _predicate: predicate,
            columns,
            buffer: Vec::new(),
            pos: 0,
        }
    }

    pub fn new_with_data(predicate: Expr, data: Vec<(RowId, Row)>, columns: Vec<String>) -> Self {
        FilterOperator {
            _predicate: predicate,
            columns,
            buffer: data,
            pos: 0,
        }
    }
}

impl PhysicalOperator for FilterOperator {
    fn open(&mut self, _ctx: &mut ExecContext) -> Result<()> {
        // Pre-filter is done externally
        self.pos = 0;
        Ok(())
    }
    fn next(&mut self, _ctx: &mut ExecContext) -> Result<Option<(RowId, Row)>> {
        if self.pos < self.buffer.len() {
            let result = self.buffer[self.pos].clone();
            self.pos += 1;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> Vec<String> {
        self.columns.clone()
    }
}

// Projection operator
pub struct ProjectionOperator {
    items: Vec<SelectItem>,
    _columns: Vec<String>,
    buffer: Vec<(RowId, Row)>,
    pos: usize,
}

impl ProjectionOperator {
    pub fn new(
        items: Vec<SelectItem>,
        _input: Box<dyn PhysicalOperator>,
        columns: Vec<String>,
    ) -> Self {
        ProjectionOperator {
            items,
            _columns: columns,
            buffer: Vec::new(),
            pos: 0,
        }
    }

    pub fn new_with_data(
        items: Vec<SelectItem>,
        data: Vec<(RowId, Row)>,
        columns: Vec<String>,
    ) -> Self {
        ProjectionOperator {
            items,
            _columns: columns,
            buffer: data,
            pos: 0,
        }
    }
}

impl PhysicalOperator for ProjectionOperator {
    fn open(&mut self, _ctx: &mut ExecContext) -> Result<()> {
        self.pos = 0;
        Ok(())
    }
    fn next(&mut self, _ctx: &mut ExecContext) -> Result<Option<(RowId, Row)>> {
        if self.pos < self.buffer.len() {
            let result = self.buffer[self.pos].clone();
            self.pos += 1;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> Vec<String> {
        self.items
            .iter()
            .map(|item| match item {
                SelectItem::Star => "*".to_string(),
                SelectItem::Expr { expr, alias } => {
                    if let Some(a) = alias {
                        a.clone()
                    } else {
                        format!("{:?}", expr)
                    }
                }
            })
            .collect()
    }
}

// Sort operator
pub struct SortOperator {
    order_by: Vec<OrderByItem>,
    columns: Vec<String>,
    buffer: Vec<(RowId, Row)>,
    pos: usize,
}

impl SortOperator {
    pub fn new(
        order_by: Vec<OrderByItem>,
        _input: Box<dyn PhysicalOperator>,
        columns: Vec<String>,
    ) -> Self {
        SortOperator {
            order_by,
            columns,
            buffer: Vec::new(),
            pos: 0,
        }
    }

    pub fn new_with_data(
        order_by: Vec<OrderByItem>,
        data: Vec<(RowId, Row)>,
        columns: Vec<String>,
    ) -> Self {
        SortOperator {
            order_by,
            columns,
            buffer: data,
            pos: 0,
        }
    }
}

impl PhysicalOperator for SortOperator {
    fn open(&mut self, _ctx: &mut ExecContext) -> Result<()> {
        let columns = self.columns.clone();
        let order_by = self.order_by.clone();
        self.buffer.sort_by(|a, b| {
            for item in &order_by {
                let va = evaluate_expr(&item.expr, &a.1, &columns).unwrap_or(Value::Null);
                let vb = evaluate_expr(&item.expr, &b.1, &columns).unwrap_or(Value::Null);
                let cmp = match &item.direction {
                    OrderDirection::Asc => va.cmp(&vb),
                    OrderDirection::Desc => vb.cmp(&va),
                };
                if cmp != std::cmp::Ordering::Equal {
                    return cmp;
                }
            }
            std::cmp::Ordering::Equal
        });
        self.pos = 0;
        Ok(())
    }
    fn next(&mut self, _ctx: &mut ExecContext) -> Result<Option<(RowId, Row)>> {
        if self.pos < self.buffer.len() {
            let result = self.buffer[self.pos].clone();
            self.pos += 1;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> Vec<String> {
        self.columns.clone()
    }
}

// Limit operator
pub struct LimitOperator {
    _count: Expr,
    _offset: Option<Expr>,
    columns: Vec<String>,
    buffer: Vec<(RowId, Row)>,
    pos: usize,
}

impl LimitOperator {
    pub fn new(
        count: Expr,
        offset: Option<Expr>,
        _input: Box<dyn PhysicalOperator>,
        columns: Vec<String>,
    ) -> Self {
        LimitOperator {
            _count: count,
            _offset: offset,
            columns,
            buffer: Vec::new(),
            pos: 0,
        }
    }

    pub fn new_with_data(
        count: Expr,
        offset: Option<Expr>,
        data: Vec<(RowId, Row)>,
        columns: Vec<String>,
    ) -> Self {
        LimitOperator {
            _count: count,
            _offset: offset,
            columns,
            buffer: data,
            pos: 0,
        }
    }
}

impl PhysicalOperator for LimitOperator {
    fn open(&mut self, _ctx: &mut ExecContext) -> Result<()> {
        // Pre-computed externally
        self.pos = 0;
        Ok(())
    }
    fn next(&mut self, _ctx: &mut ExecContext) -> Result<Option<(RowId, Row)>> {
        if self.pos < self.buffer.len() {
            let result = self.buffer[self.pos].clone();
            self.pos += 1;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> Vec<String> {
        self.columns.clone()
    }
}

// Insert operator (not part of PhysicalOperator trait - handled directly)
pub struct InsertOperator {
    table: String,
    columns: Option<Vec<String>>,
    rows: Vec<Vec<Expr>>,
    executed: bool,
    pub rows_affected: usize,
}

impl InsertOperator {
    pub fn new(table: String, columns: Option<Vec<String>>, rows: Vec<Vec<Expr>>) -> Self {
        InsertOperator {
            table,
            columns,
            rows,
            executed: false,
            rows_affected: 0,
        }
    }

    pub fn execute(&mut self, ctx: &mut ExecContext) -> Result<()> {
        if self.executed {
            return Ok(());
        }

        let table_info = ctx.catalog.get_table(&self.table)?.clone();
        let mut count = 0;

        for row_exprs in &self.rows {
            let mut values: Vec<Value> = Vec::new();
            for expr in row_exprs {
                values.push(evaluate_expr(expr, &Row::new(), &[])?);
            }

            let mut row: Row = Vec::new();

            if let Some(ref cols) = self.columns {
                for col_def in &table_info.columns {
                    let pos = cols
                        .iter()
                        .position(|c| c.eq_ignore_ascii_case(&col_def.name));
                    if let Some(idx) = pos {
                        let val = col_def
                            .col_type
                            .coerce(values[idx].clone())
                            .map_err(|e| MiniDbError::Sql(e))?;
                        if col_def.not_null && val.is_null() {
                            return Err(MiniDbError::Constraint(format!(
                                "column '{}' cannot be NULL",
                                col_def.name
                            )));
                        }
                        row.push(val);
                    } else {
                        let val = col_def.default.clone().unwrap_or(Value::Null);
                        let val = col_def
                            .col_type
                            .coerce(val)
                            .map_err(|e| MiniDbError::Sql(e))?;
                        if col_def.not_null && val.is_null() {
                            return Err(MiniDbError::Constraint(format!(
                                "column '{}' cannot be NULL",
                                col_def.name
                            )));
                        }
                        row.push(val);
                    }
                }
            } else {
                if values.len() != table_info.columns.len() {
                    return Err(MiniDbError::Sql(format!(
                        "expected {} values, got {}",
                        table_info.columns.len(),
                        values.len()
                    )));
                }
                for (i, col_def) in table_info.columns.iter().enumerate() {
                    let val = col_def
                        .col_type
                        .coerce(values[i].clone())
                        .map_err(|e| MiniDbError::Sql(e))?;
                    if col_def.not_null && val.is_null() {
                        return Err(MiniDbError::Constraint(format!(
                            "column '{}' cannot be NULL",
                            col_def.name
                        )));
                    }
                    row.push(val);
                }
            }

            // Check uniqueness constraints
            if table_info.columns.iter().any(|c| c.unique) {
                let existing = ctx.heap.scan_table(&self.table)?;
                for col_def in &table_info.columns {
                    if col_def.unique {
                        if let Some(col_idx) = table_info.column_index(&col_def.name) {
                            let new_val = &row[col_idx];
                            if !new_val.is_null() {
                                for (_, existing_row) in &existing {
                                    if col_idx < existing_row.len()
                                        && new_val.sql_eq(&existing_row[col_idx]).unwrap_or(false)
                                    {
                                        return Err(MiniDbError::Constraint(format!(
                                            "UNIQUE constraint failed on column '{}'",
                                            col_def.name
                                        )));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            ctx.heap.insert_row(&self.table, &row)?;

            // Update indexes
            let indexes: Vec<(String, String)> = ctx
                .catalog
                .all_indexes()
                .iter()
                .filter(|i| i.table == self.table)
                .map(|i| (i.name.clone(), i.column.clone()))
                .collect();

            for (idx_name, col_name) in &indexes {
                if let Some(col_idx) = table_info.column_index(col_name) {
                    let key = row[col_idx].clone();
                    let all_rows = ctx.heap.scan_table(&self.table)?;
                    if let Some((rid, _)) = all_rows.last() {
                        if let Some(btree) = ctx.catalog.btree_indexes.get_mut(idx_name) {
                            btree.insert(key, *rid);
                        }
                    }
                }
            }

            count += 1;
        }

        self.rows_affected = count;
        self.executed = true;
        Ok(())
    }
}

// Update operator
pub struct UpdateOperator {
    table: String,
    assignments: Vec<Assignment>,
    where_clause: Option<Expr>,
    columns: Vec<String>,
    executed: bool,
    pub rows_affected: usize,
}

impl UpdateOperator {
    pub fn new(
        table: String,
        assignments: Vec<Assignment>,
        where_clause: Option<Expr>,
        columns: Vec<String>,
    ) -> Self {
        UpdateOperator {
            table,
            assignments,
            where_clause,
            columns,
            executed: false,
            rows_affected: 0,
        }
    }

    pub fn execute(&mut self, ctx: &mut ExecContext) -> Result<()> {
        if self.executed {
            return Ok(());
        }

        let table_info = ctx.catalog.get_table(&self.table)?.clone();
        let all_rows = ctx.heap.scan_table(&self.table)?;
        let mut count = 0;

        for (rid, row) in &all_rows {
            if let Some(ref where_expr) = self.where_clause {
                let val = evaluate_expr(where_expr, row, &self.columns)?;
                if !val.as_bool().unwrap_or(false) {
                    continue;
                }
            }

            let mut new_row = row.clone();
            for assign in &self.assignments {
                if let Some(col_idx) = table_info.column_index(&assign.column) {
                    let mut new_val = evaluate_expr(&assign.value, row, &self.columns)?;
                    let col_def = &table_info.columns[col_idx];
                    new_val = col_def
                        .col_type
                        .coerce(new_val)
                        .map_err(|e| MiniDbError::Sql(e))?;
                    new_row[col_idx] = new_val;
                } else {
                    return Err(MiniDbError::Sql(format!(
                        "column '{}' does not exist in table '{}'",
                        assign.column, self.table
                    )));
                }
            }

            ctx.heap.update_row(&self.table, rid, &new_row)?;
            count += 1;
        }

        self.rows_affected = count;
        self.executed = true;
        Ok(())
    }
}

// Delete operator
pub struct DeleteOperator {
    table: String,
    where_clause: Option<Expr>,
    columns: Vec<String>,
    executed: bool,
    pub rows_affected: usize,
}

impl DeleteOperator {
    pub fn new(table: String, where_clause: Option<Expr>, columns: Vec<String>) -> Self {
        DeleteOperator {
            table,
            where_clause,
            columns,
            executed: false,
            rows_affected: 0,
        }
    }

    pub fn execute(&mut self, ctx: &mut ExecContext) -> Result<()> {
        if self.executed {
            return Ok(());
        }

        let all_rows = ctx.heap.scan_table(&self.table)?;
        let mut to_delete = Vec::new();

        for (rid, row) in &all_rows {
            if let Some(ref where_expr) = self.where_clause {
                let val = evaluate_expr(where_expr, row, &self.columns)?;
                if !val.as_bool().unwrap_or(false) {
                    continue;
                }
            }
            to_delete.push(*rid);
        }

        let mut count = 0;
        for rid in to_delete {
            ctx.heap.delete_row(&self.table, &rid)?;

            // Remove from indexes
            let indexes: Vec<String> = ctx
                .catalog
                .all_indexes()
                .iter()
                .filter(|i| i.table == self.table)
                .map(|i| i.name.clone())
                .collect();

            for idx_name in &indexes {
                if let Some(btree) = ctx.catalog.btree_indexes.get_mut(idx_name) {
                    btree.remove_by_row_id(&rid);
                }
            }

            count += 1;
        }

        self.rows_affected = count;
        self.executed = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_literal() {
        let row = vec![];
        let cols = vec![];
        let val = evaluate_expr(&Expr::Literal(Literal::Integer(42)), &row, &cols).unwrap();
        assert_eq!(val, Value::Int(42));
    }

    #[test]
    fn test_evaluate_arithmetic() {
        let row = vec![];
        let cols = vec![];
        let expr = Expr::BinaryOp {
            op: BinOp::Add,
            left: Box::new(Expr::Literal(Literal::Integer(1))),
            right: Box::new(Expr::Literal(Literal::Integer(2))),
        };
        let val = evaluate_expr(&expr, &row, &cols).unwrap();
        assert_eq!(val, Value::Int(3));
    }

    #[test]
    fn test_evaluate_function_length() {
        let row = vec![Value::Text("hello".into())];
        let cols = vec!["name".into()];
        let expr = Expr::Function {
            name: "LENGTH".into(),
            args: vec![Expr::ColumnRef {
                table: None,
                column: "name".into(),
            }],
        };
        let val = evaluate_expr(&expr, &row, &cols).unwrap();
        assert_eq!(val, Value::Int(5));
    }
}
