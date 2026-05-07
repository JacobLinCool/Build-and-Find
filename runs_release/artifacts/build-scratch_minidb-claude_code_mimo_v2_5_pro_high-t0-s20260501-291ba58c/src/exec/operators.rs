use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;
use crate::plan::ResolvedProjection;
use crate::storage::btree::{BTreeIndex, RowId};
use crate::storage::heap::HeapFile;
use crate::storage::page::PageId;
use crate::types::{ColumnDef, ColumnType, Row, TableSchema, Value};
use std::cmp::Ordering;

pub trait PhysicalOperator {
    fn open(&mut self) -> Result<()>;
    fn next(&mut self) -> Result<Option<Row>>;
    fn close(&mut self) -> Result<()>;
    fn schema(&self) -> Vec<String>;
}

pub struct SeqScan {
    heap: HeapFile,
    pages: Vec<PageId>,
    columns: Vec<ColumnDef>,
    current_page: usize,
    current_rows: Vec<(u16, Row)>,
    current_idx: usize,
    filter: Option<Expr>,
    table_aliases: Vec<String>,
}

impl SeqScan {
    pub fn new(
        table_name: &str,
        pages: Vec<PageId>,
        columns: Vec<ColumnDef>,
        filter: Option<Expr>,
    ) -> Self {
        let table_aliases = vec![table_name.to_string()];
        Self {
            heap: HeapFile::new(table_name),
            pages,
            columns,
            current_page: 0,
            current_rows: Vec::new(),
            current_idx: 0,
            filter,
            table_aliases,
        }
    }
}

impl PhysicalOperator for SeqScan {
    fn open(&mut self) -> Result<()> {
        self.current_page = 0;
        self.current_idx = 0;
        self.current_rows = Vec::new();
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        loop {
            if self.current_idx < self.current_rows.len() {
                let (_, row) = self.current_rows[self.current_idx].clone();
                self.current_idx += 1;

                if let Some(ref filter) = self.filter {
                    let ctx = EvalContext {
                        columns: &self.columns,
                        table_aliases: &self.table_aliases,
                        current_row: &row,
                    };
                    let val = eval_expr(filter, &ctx)?;
                    match val {
                        Value::Bool(true) => return Ok(Some(row)),
                        Value::Bool(false) | Value::Null => continue,
                        _ => continue,
                    }
                }
                return Ok(Some(row));
            }

            if self.current_page >= self.pages.len() {
                return Ok(None);
            }

            let _pid = self.pages[self.current_page];
            // We need mutable access to PageManager, but it's shared.
            // This is a design issue. For the benchmark, we'll use a different approach.
            // Actually, let's restructure so each operator holds a reference to PM.
            // For now, let's use a different approach where we pass PM through.
            // Actually, the cleanest approach for the benchmark is to have the executor
            // own the PM and pass it to each operator call.
            // Let me restructure.

            // For now, return None and we'll fix this in the executor
            return Ok(None);
        }
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        self.columns.iter().map(|c| c.name.clone()).collect()
    }
}

pub struct IndexScan {
    index: BTreeIndex,
    heap: HeapFile,
    table_pages: Vec<PageId>,
    columns: Vec<ColumnDef>,
    search_key: Value,
    row_ids: Vec<RowId>,
    current_idx: usize,
    filter: Option<Expr>,
    table_aliases: Vec<String>,
}

impl IndexScan {
    pub fn new(
        index: BTreeIndex,
        table_name: &str,
        table_pages: Vec<PageId>,
        columns: Vec<ColumnDef>,
        search_key: Value,
        filter: Option<Expr>,
    ) -> Self {
        let table_aliases = vec![table_name.to_string()];
        Self {
            index,
            heap: HeapFile::new(table_name),
            table_pages,
            columns,
            search_key,
            row_ids: Vec::new(),
            current_idx: 0,
            filter,
            table_aliases,
        }
    }
}

impl PhysicalOperator for IndexScan {
    fn open(&mut self) -> Result<()> {
        self.row_ids = Vec::new();
        self.current_idx = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        // Index scan needs PM access - handled by executor
        Ok(None)
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        self.columns.iter().map(|c| c.name.clone()).collect()
    }
}

pub struct Filter {
    input: Box<dyn PhysicalOperator>,
    predicate: Expr,
    columns: Vec<ColumnDef>,
    table_aliases: Vec<String>,
}

impl Filter {
    pub fn new(
        input: Box<dyn PhysicalOperator>,
        predicate: Expr,
        columns: Vec<ColumnDef>,
        table_aliases: Vec<String>,
    ) -> Self {
        Self {
            input,
            predicate,
            columns,
            table_aliases,
        }
    }
}

impl PhysicalOperator for Filter {
    fn open(&mut self) -> Result<()> {
        self.input.open()
    }

    fn next(&mut self) -> Result<Option<Row>> {
        loop {
            let row = match self.input.next()? {
                Some(r) => r,
                None => return Ok(None),
            };
            let ctx = EvalContext {
                columns: &self.columns,
                table_aliases: &self.table_aliases,
                current_row: &row,
            };
            let val = eval_expr(&self.predicate, &ctx)?;
            match val {
                Value::Bool(true) => return Ok(Some(row)),
                Value::Bool(false) | Value::Null => continue,
                _ => continue,
            }
        }
    }

    fn close(&mut self) -> Result<()> {
        self.input.close()
    }

    fn schema(&self) -> Vec<String> {
        self.input.schema()
    }
}

pub struct Project {
    input: Box<dyn PhysicalOperator>,
    projections: Vec<ResolvedProjection>,
    input_columns: Vec<ColumnDef>,
    table_aliases: Vec<String>,
}

impl Project {
    pub fn new(
        input: Box<dyn PhysicalOperator>,
        projections: Vec<ResolvedProjection>,
        input_columns: Vec<ColumnDef>,
        table_aliases: Vec<String>,
    ) -> Self {
        Self {
            input,
            projections,
            input_columns,
            table_aliases,
        }
    }
}

impl PhysicalOperator for Project {
    fn open(&mut self) -> Result<()> {
        self.input.open()
    }

    fn next(&mut self) -> Result<Option<Row>> {
        let row = match self.input.next()? {
            Some(r) => r,
            None => return Ok(None),
        };

        let ctx = EvalContext {
            columns: &self.input_columns,
            table_aliases: &self.table_aliases,
            current_row: &row,
        };

        let mut values = Vec::new();
        for proj in &self.projections {
            let val = eval_expr(&proj.expr, &ctx)?;
            values.push(val);
        }
        Ok(Some(Row::new(values)))
    }

    fn close(&mut self) -> Result<()> {
        self.input.close()
    }

    fn schema(&self) -> Vec<String> {
        self.projections.iter().map(|p| p.alias.clone()).collect()
    }
}

pub struct NestedLoopJoin {
    left: Box<dyn PhysicalOperator>,
    right: Box<dyn PhysicalOperator>,
    on: Expr,
    left_columns: Vec<ColumnDef>,
    right_columns: Vec<ColumnDef>,
    left_aliases: Vec<String>,
    right_aliases: Vec<String>,
    current_left: Option<Row>,
    right_exhausted: bool,
}

impl NestedLoopJoin {
    pub fn new(
        left: Box<dyn PhysicalOperator>,
        right: Box<dyn PhysicalOperator>,
        on: Expr,
        left_columns: Vec<ColumnDef>,
        right_columns: Vec<ColumnDef>,
        left_aliases: Vec<String>,
        right_aliases: Vec<String>,
    ) -> Self {
        Self {
            left,
            right,
            on,
            left_columns,
            right_columns,
            left_aliases,
            right_aliases,
            current_left: None,
            right_exhausted: false,
        }
    }
}

impl PhysicalOperator for NestedLoopJoin {
    fn open(&mut self) -> Result<()> {
        self.left.open()?;
        self.right.open()?;
        self.current_left = None;
        self.right_exhausted = false;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        loop {
            if self.current_left.is_none() {
                self.current_left = self.left.next()?;
                if self.current_left.is_none() {
                    return Ok(None);
                }
                self.right.close()?;
                self.right.open()?;
                self.right_exhausted = false;
            }

            let right_row = match self.right.next()? {
                Some(r) => r,
                None => {
                    self.right_exhausted = true;
                    self.current_left = None;
                    continue;
                }
            };

            let left_row = self.current_left.as_ref().unwrap();

            // Combine columns and rows for evaluation
            let mut combined_columns = self.left_columns.clone();
            combined_columns.extend_from_slice(&self.right_columns);
            let mut combined_values = left_row.values.clone();
            combined_values.extend_from_slice(&right_row.values);
            let combined_row = Row::new(combined_values);

            let mut combined_aliases = self.left_aliases.clone();
            combined_aliases.extend_from_slice(&self.right_aliases);

            let ctx = EvalContext {
                columns: &combined_columns,
                table_aliases: &combined_aliases,
                current_row: &combined_row,
            };

            let val = eval_expr(&self.on, &ctx)?;
            match val {
                Value::Bool(true) => return Ok(Some(combined_row)),
                Value::Bool(false) | Value::Null => continue,
                _ => continue,
            }
        }
    }

    fn close(&mut self) -> Result<()> {
        self.left.close()?;
        self.right.close()
    }

    fn schema(&self) -> Vec<String> {
        let mut s = self.left.schema();
        s.extend(self.right.schema());
        s
    }
}

pub struct Sort {
    input: Box<dyn PhysicalOperator>,
    keys: Vec<(Expr, bool)>,
    columns: Vec<ColumnDef>,
    table_aliases: Vec<String>,
    sorted_rows: Vec<Row>,
    current_idx: usize,
    loaded: bool,
}

impl Sort {
    pub fn new(
        input: Box<dyn PhysicalOperator>,
        keys: Vec<(Expr, bool)>,
        columns: Vec<ColumnDef>,
        table_aliases: Vec<String>,
    ) -> Self {
        Self {
            input,
            keys,
            columns,
            table_aliases,
            sorted_rows: Vec::new(),
            current_idx: 0,
            loaded: false,
        }
    }
}

impl PhysicalOperator for Sort {
    fn open(&mut self) -> Result<()> {
        self.input.open()?;
        self.sorted_rows.clear();
        self.current_idx = 0;
        self.loaded = false;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if !self.loaded {
            // Load all rows
            while let Some(row) = self.input.next()? {
                self.sorted_rows.push(row);
            }

            // Sort
            let keys = self.keys.clone();
            let columns = self.columns.clone();
            let aliases = self.table_aliases.clone();
            self.sorted_rows.sort_by(|a, b| {
                let ctx_a = EvalContext {
                    columns: &columns,
                    table_aliases: &aliases,
                    current_row: a,
                };
                let ctx_b = EvalContext {
                    columns: &columns,
                    table_aliases: &aliases,
                    current_row: b,
                };
                for (expr, asc) in &keys {
                    let val_a = eval_expr(expr, &ctx_a).unwrap_or(Value::Null);
                    let val_b = eval_expr(expr, &ctx_b).unwrap_or(Value::Null);
                    let ord = val_a.compare_nulls_last(&val_b);
                    if ord != Ordering::Equal {
                        return if *asc { ord } else { ord.reverse() };
                    }
                }
                Ordering::Equal
            });

            self.loaded = true;
        }

        if self.current_idx >= self.sorted_rows.len() {
            return Ok(None);
        }
        let row = self.sorted_rows[self.current_idx].clone();
        self.current_idx += 1;
        Ok(Some(row))
    }

    fn close(&mut self) -> Result<()> {
        self.input.close()
    }

    fn schema(&self) -> Vec<String> {
        self.input.schema()
    }
}

pub struct Limit {
    input: Box<dyn PhysicalOperator>,
    limit: usize,
    offset: usize,
    emitted: usize,
    skipped: usize,
}

impl Limit {
    pub fn new(input: Box<dyn PhysicalOperator>, limit: usize, offset: usize) -> Self {
        Self {
            input,
            limit,
            offset,
            emitted: 0,
            skipped: 0,
        }
    }
}

impl PhysicalOperator for Limit {
    fn open(&mut self) -> Result<()> {
        self.input.open()?;
        self.emitted = 0;
        self.skipped = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        while self.skipped < self.offset {
            match self.input.next()? {
                Some(_) => self.skipped += 1,
                None => return Ok(None),
            }
        }

        if self.emitted >= self.limit {
            return Ok(None);
        }

        let row = self.input.next()?;
        if row.is_some() {
            self.emitted += 1;
        }
        Ok(row)
    }

    fn close(&mut self) -> Result<()> {
        self.input.close()
    }

    fn schema(&self) -> Vec<String> {
        self.input.schema()
    }
}

pub struct InsertOperator {
    table: String,
    columns: Vec<String>,
    values: Vec<Vec<Expr>>,
    schema: TableSchema,
    done: bool,
}

impl InsertOperator {
    pub fn new(
        table: String,
        columns: Vec<String>,
        values: Vec<Vec<Expr>>,
        schema: TableSchema,
    ) -> Self {
        Self {
            table,
            columns,
            values,
            schema,
            done: false,
        }
    }
}

impl PhysicalOperator for InsertOperator {
    fn open(&mut self) -> Result<()> {
        self.done = false;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.done {
            return Ok(None);
        }
        self.done = true;
        // Return the values to be inserted (executor handles actual insertion)
        // Return a marker row with the count
        Ok(Some(Row::new(vec![Value::Int(self.values.len() as i64)])))
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        vec!["affected".to_string()]
    }
}

pub struct UpdateOperator {
    table: String,
    assignments: Vec<(String, Expr)>,
    filter: Option<Expr>,
    done: bool,
}

impl UpdateOperator {
    pub fn new(table: String, assignments: Vec<(String, Expr)>, filter: Option<Expr>) -> Self {
        Self {
            table,
            assignments,
            filter,
            done: false,
        }
    }
}

impl PhysicalOperator for UpdateOperator {
    fn open(&mut self) -> Result<()> {
        self.done = false;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.done {
            return Ok(None);
        }
        self.done = true;
        Ok(None) // Executor handles the actual update
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        vec!["affected".to_string()]
    }
}

pub struct DeleteOperator {
    table: String,
    filter: Option<Expr>,
    done: bool,
}

impl DeleteOperator {
    pub fn new(table: String, filter: Option<Expr>) -> Self {
        Self {
            table,
            filter,
            done: false,
        }
    }
}

impl PhysicalOperator for DeleteOperator {
    fn open(&mut self) -> Result<()> {
        self.done = false;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.done {
            return Ok(None);
        }
        self.done = true;
        Ok(None) // Executor handles the actual delete
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        vec!["affected".to_string()]
    }
}

// Expression evaluation

pub struct EvalContext<'a> {
    pub columns: &'a [ColumnDef],
    pub table_aliases: &'a [String],
    pub current_row: &'a Row,
}

pub fn eval_expr(expr: &Expr, ctx: &EvalContext) -> Result<Value> {
    match expr {
        Expr::Literal(v) => Ok(v.clone()),
        Expr::ColumnRef { table, name } => {
            // Find column index by name, respecting table qualifier if present
            let mut best_match: Option<usize> = None;
            for (i, col) in ctx.columns.iter().enumerate() {
                if col.name == *name {
                    if let Some(tbl) = table {
                        // If we have a meaningful table alias at this position, check it
                        if let Some(alias) = ctx.table_aliases.get(i) {
                            if !alias.is_empty() && alias != tbl {
                                continue;
                            }
                        }
                    }
                    best_match = Some(i);
                    break;
                }
            }
            if let Some(i) = best_match {
                return Ok(ctx
                    .current_row
                    .values
                    .get(i)
                    .cloned()
                    .unwrap_or(Value::Null));
            }
            Err(MiniDbError::Runtime(format!("column '{}' not found", name)))
        }
        Expr::BinaryOp { left, op, right } => {
            let l = eval_expr(left, ctx)?;
            let r = eval_expr(right, ctx)?;
            match op {
                BinaryOp::Eq => Ok(Value::Bool(l == r)),
                BinaryOp::Ne => Ok(Value::Bool(l != r)),
                BinaryOp::Lt => Ok(Value::Bool(l.partial_cmp(&r) == Some(Ordering::Less))),
                BinaryOp::Le => Ok(Value::Bool(
                    l.partial_cmp(&r).map_or(false, |o| o != Ordering::Greater),
                )),
                BinaryOp::Gt => Ok(Value::Bool(l.partial_cmp(&r) == Some(Ordering::Greater))),
                BinaryOp::Ge => Ok(Value::Bool(
                    l.partial_cmp(&r).map_or(false, |o| o != Ordering::Less),
                )),
                BinaryOp::Add => Ok(l.add(&r)),
                BinaryOp::Sub => Ok(l.sub(&r)),
                BinaryOp::Mul => Ok(l.mul(&r)),
                BinaryOp::Div => Ok(l.div(&r)),
                BinaryOp::Concat => Ok(l.concat(&r)),
                BinaryOp::And => match (&l, &r) {
                    (Value::Bool(false), _) | (_, Value::Bool(false)) => Ok(Value::Bool(false)),
                    (Value::Bool(true), Value::Bool(true)) => Ok(Value::Bool(true)),
                    _ => Ok(Value::Null),
                },
                BinaryOp::Or => match (&l, &r) {
                    (Value::Bool(true), _) | (_, Value::Bool(true)) => Ok(Value::Bool(true)),
                    (Value::Bool(false), Value::Bool(false)) => Ok(Value::Bool(false)),
                    _ => Ok(Value::Null),
                },
            }
        }
        Expr::UnaryOp { op, expr } => {
            let val = eval_expr(expr, ctx)?;
            match op {
                UnaryOp::Neg => Ok(val.negate()),
                UnaryOp::Not => match val {
                    Value::Bool(b) => Ok(Value::Bool(!b)),
                    Value::Null => Ok(Value::Null),
                    _ => Ok(Value::Null),
                },
            }
        }
        Expr::Function { name, args } => {
            let arg_values: Vec<Value> = args
                .iter()
                .map(|a| eval_expr(a, ctx))
                .collect::<Result<Vec<_>>>()?;
            eval_function(name, &arg_values)
        }
        Expr::IsNull(expr) => {
            let val = eval_expr(expr, ctx)?;
            Ok(Value::Bool(val.is_null()))
        }
        Expr::IsNotNull(expr) => {
            let val = eval_expr(expr, ctx)?;
            Ok(Value::Bool(!val.is_null()))
        }
    }
}

fn eval_function(name: &str, args: &[Value]) -> Result<Value> {
    match name.to_uppercase().as_str() {
        "LENGTH" => {
            if args.len() != 1 {
                return Err(MiniDbError::Runtime("LENGTH takes 1 argument".to_string()));
            }
            match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Int(s.len() as i64)),
                v => Err(MiniDbError::Type(format!(
                    "LENGTH expects TEXT, got {}",
                    v.type_name()
                ))),
            }
        }
        "LOWER" => {
            if args.len() != 1 {
                return Err(MiniDbError::Runtime("LOWER takes 1 argument".to_string()));
            }
            match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Text(s.to_lowercase())),
                v => Err(MiniDbError::Type(format!(
                    "LOWER expects TEXT, got {}",
                    v.type_name()
                ))),
            }
        }
        "UPPER" => {
            if args.len() != 1 {
                return Err(MiniDbError::Runtime("UPPER takes 1 argument".to_string()));
            }
            match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Text(s.to_uppercase())),
                v => Err(MiniDbError::Type(format!(
                    "UPPER expects TEXT, got {}",
                    v.type_name()
                ))),
            }
        }
        "SUBSTR" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(MiniDbError::Runtime(
                    "SUBSTR takes 2 or 3 arguments".to_string(),
                ));
            }
            match (&args[0], &args[1]) {
                (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                (Value::Text(s), Value::Int(start)) => {
                    let start = (*start).max(0) as usize;
                    let len = if args.len() == 3 {
                        match &args[2] {
                            Value::Int(l) => (*l).max(0) as usize,
                            _ => s.len(),
                        }
                    } else {
                        s.len()
                    };
                    let substr: String = s.chars().skip(start).take(len).collect();
                    Ok(Value::Text(substr))
                }
                _ => Err(MiniDbError::Type(
                    "SUBSTR expects (TEXT, INT[, INT])".to_string(),
                )),
            }
        }
        "ABS" => {
            if args.len() != 1 {
                return Err(MiniDbError::Runtime("ABS takes 1 argument".to_string()));
            }
            match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Int(i) => Ok(Value::Int(i.abs())),
                Value::Real(r) => Ok(Value::Real(r.abs())),
                v => Err(MiniDbError::Type(format!(
                    "ABS expects numeric, got {}",
                    v.type_name()
                ))),
            }
        }
        "COALESCE" => {
            for arg in args {
                if !arg.is_null() {
                    return Ok(arg.clone());
                }
            }
            Ok(Value::Null)
        }
        _ => Err(MiniDbError::Runtime(format!("unknown function: {name}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_literal() {
        let columns = vec![];
        let aliases = vec![];
        let row = Row::new(vec![]);
        let ctx = EvalContext {
            columns: &columns,
            table_aliases: &aliases,
            current_row: &row,
        };
        assert_eq!(
            eval_expr(&Expr::Literal(Value::Int(42)), &ctx).unwrap(),
            Value::Int(42)
        );
    }

    #[test]
    fn test_eval_arithmetic() {
        let columns = vec![];
        let aliases = vec![];
        let row = Row::new(vec![]);
        let ctx = EvalContext {
            columns: &columns,
            table_aliases: &aliases,
            current_row: &row,
        };
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Literal(Value::Int(2))),
            op: BinaryOp::Add,
            right: Box::new(Expr::Literal(Value::Int(3))),
        };
        assert_eq!(eval_expr(&expr, &ctx).unwrap(), Value::Int(5));
    }

    #[test]
    fn test_eval_function() {
        assert_eq!(
            eval_function("LENGTH", &[Value::Text("hello".to_string())]).unwrap(),
            Value::Int(5)
        );
        assert_eq!(
            eval_function("LOWER", &[Value::Text("HELLO".to_string())]).unwrap(),
            Value::Text("hello".to_string())
        );
        assert_eq!(
            eval_function("ABS", &[Value::Int(-42)]).unwrap(),
            Value::Int(42)
        );
        assert_eq!(
            eval_function("COALESCE", &[Value::Null, Value::Int(42)]).unwrap(),
            Value::Int(42)
        );
    }

    #[test]
    fn test_eval_column_ref() {
        let columns = vec![ColumnDef {
            name: "id".to_string(),
            col_type: ColumnType::Int,
            primary_key: false,
            not_null: false,
            unique: false,
            default: None,
        }];
        let aliases = vec!["t".to_string()];
        let row = Row::new(vec![Value::Int(42)]);
        let ctx = EvalContext {
            columns: &columns,
            table_aliases: &aliases,
            current_row: &row,
        };
        let expr = Expr::ColumnRef {
            table: None,
            name: "id".to_string(),
        };
        assert_eq!(eval_expr(&expr, &ctx).unwrap(), Value::Int(42));
    }
}
