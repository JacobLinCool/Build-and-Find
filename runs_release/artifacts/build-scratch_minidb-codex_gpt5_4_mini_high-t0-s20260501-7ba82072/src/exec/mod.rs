use std::cmp::Ordering;

use log::debug;

use crate::error::{MiniDbError, Result};
use crate::parser::ast::{BinaryOp, Expr, OrderBy, SelectItem, TableRef, UnaryOp};
use crate::txn::{DatabaseState, TableData};
use crate::types::{ColumnType, ResultSet, Row, SchemaColumn, TriBool, Value};

pub trait PhysicalOperator {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()>;
    fn next(&mut self) -> Result<Option<Row>>;
    fn close(&mut self) -> Result<()>;
    fn schema(&self) -> &[SchemaColumn];
}

#[derive(Debug, Clone, Default)]
pub struct QueryExecutor;

impl QueryExecutor {
    pub fn run(
        &self,
        op: &mut dyn PhysicalOperator,
        state: &mut DatabaseState,
    ) -> Result<ResultSet> {
        op.open(state)?;
        let mut rows = Vec::new();
        while let Some(row) = op.next()? {
            rows.push(row);
        }
        let columns = op.schema().to_vec();
        op.close()?;
        Ok(ResultSet { columns, rows })
    }
}

#[derive(Debug, Clone)]
pub struct RowContext {
    pub columns: Vec<SchemaColumn>,
    pub values: Row,
}

impl RowContext {
    pub fn lookup(&self, table: Option<&str>, column: &str) -> Result<Value> {
        let mut matches = Vec::new();
        for (idx, schema) in self.columns.iter().enumerate() {
            if schema.name.eq_ignore_ascii_case(column) {
                if let Some(table_name) = table {
                    if schema.table.as_deref() == Some(table_name) {
                        return Ok(self.values[idx].clone());
                    }
                } else {
                    matches.push(idx);
                }
            }
        }
        if let Some(table_name) = table {
            Err(MiniDbError::Runtime(format!(
                "unknown column {}.{}",
                table_name, column
            )))
        } else if matches.len() == 1 {
            Ok(self.values[matches[0]].clone())
        } else if matches.is_empty() {
            Err(MiniDbError::Runtime(format!("unknown column {column}")))
        } else {
            Err(MiniDbError::Runtime(format!("ambiguous column {column}")))
        }
    }
}

pub fn evaluate(expr: &Expr, row: Option<&RowContext>) -> Result<Value> {
    match expr {
        Expr::Literal(literal) => Ok(Value::from_literal(literal)),
        Expr::ColumnRef { table, column } => {
            let ctx =
                row.ok_or_else(|| MiniDbError::Runtime("column reference outside row".into()))?;
            ctx.lookup(table.as_deref(), column)
        }
        Expr::Star | Expr::QualifiedStar { .. } => Err(MiniDbError::Runtime(
            "star expression cannot be evaluated directly".into(),
        )),
        Expr::Unary { op, expr } => {
            let v = evaluate(expr, row)?;
            match op {
                UnaryOp::Neg => match v {
                    Value::Int(v) => Ok(Value::Int(-v)),
                    Value::Real(v) => Ok(Value::Real(-v)),
                    Value::Null => Ok(Value::Null),
                    _ => Err(MiniDbError::Type("numeric value required".into())),
                },
                UnaryOp::Not => Ok(TriBool::from_value(&v).not().to_value()),
                UnaryOp::IsNull => Ok(Value::Bool(v.is_null())),
                UnaryOp::IsNotNull => Ok(Value::Bool(!v.is_null())),
            }
        }
        Expr::Binary { left, op, right } => {
            use BinaryOp::*;
            match op {
                And | Or => {
                    let a = TriBool::from_value(&evaluate(left, row)?);
                    let b = TriBool::from_value(&evaluate(right, row)?);
                    Ok(match op {
                        And => a.and(b).to_value(),
                        Or => a.or(b).to_value(),
                        _ => unreachable!(),
                    })
                }
                Eq | Neq | Lt | Lte | Gt | Gte => {
                    let a = evaluate(left, row)?;
                    let b = evaluate(right, row)?;
                    if a.is_null() || b.is_null() {
                        return Ok(Value::Null);
                    }
                    let ord = a.total_cmp(&b);
                    let result = match op {
                        Eq => ord == Ordering::Equal,
                        Neq => ord != Ordering::Equal,
                        Lt => ord == Ordering::Less,
                        Lte => ord != Ordering::Greater,
                        Gt => ord == Ordering::Greater,
                        Gte => ord != Ordering::Less,
                        _ => unreachable!(),
                    };
                    Ok(Value::Bool(result))
                }
                Add | Sub | Mul | Div => {
                    let a = evaluate(left, row)?;
                    let b = evaluate(right, row)?;
                    arithmetic(op, a, b)
                }
                Concat => {
                    let a = evaluate(left, row)?;
                    let b = evaluate(right, row)?;
                    match (a, b) {
                        (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                        (Value::Text(a), Value::Text(b)) => Ok(Value::Text(format!("{a}{b}"))),
                        (Value::Text(a), b) => Ok(Value::Text(format!("{a}{b}"))),
                        (a, Value::Text(b)) => Ok(Value::Text(format!("{a}{b}"))),
                        _ => Err(MiniDbError::Type("string value required".into())),
                    }
                }
            }
        }
        Expr::Function { name, args } => evaluate_function(name, args, row),
    }
}

fn arithmetic(op: &BinaryOp, a: Value, b: Value) -> Result<Value> {
    if a.is_null() || b.is_null() {
        return Ok(Value::Null);
    }
    let lhs = a
        .as_f64()
        .ok_or_else(|| MiniDbError::Type("numeric value required".into()))?;
    let rhs = b
        .as_f64()
        .ok_or_else(|| MiniDbError::Type("numeric value required".into()))?;
    let result = match op {
        BinaryOp::Add => lhs + rhs,
        BinaryOp::Sub => lhs - rhs,
        BinaryOp::Mul => lhs * rhs,
        BinaryOp::Div => lhs / rhs,
        _ => unreachable!(),
    };
    if matches!(a, Value::Int(_)) && matches!(b, Value::Int(_)) && !matches!(op, BinaryOp::Div) {
        Ok(Value::Int(result as i64))
    } else {
        Ok(Value::Real(result))
    }
}

fn evaluate_function(name: &str, args: &[Expr], row: Option<&RowContext>) -> Result<Value> {
    let upper = name.to_ascii_uppercase();
    let mut evaluated = Vec::new();
    for arg in args {
        evaluated.push(evaluate(arg, row)?);
    }
    match upper.as_str() {
        "LENGTH" => match evaluated.as_slice() {
            [Value::Text(s)] => Ok(Value::Int(s.chars().count() as i64)),
            [Value::Null] => Ok(Value::Null),
            _ => Err(MiniDbError::Type("LENGTH expects one TEXT argument".into())),
        },
        "LOWER" => match evaluated.as_slice() {
            [Value::Text(s)] => Ok(Value::Text(s.to_ascii_lowercase())),
            [Value::Null] => Ok(Value::Null),
            _ => Err(MiniDbError::Type("LOWER expects one TEXT argument".into())),
        },
        "UPPER" => match evaluated.as_slice() {
            [Value::Text(s)] => Ok(Value::Text(s.to_ascii_uppercase())),
            [Value::Null] => Ok(Value::Null),
            _ => Err(MiniDbError::Type("UPPER expects one TEXT argument".into())),
        },
        "SUBSTR" => {
            if evaluated.len() < 2 || evaluated.len() > 3 {
                return Err(MiniDbError::Type("SUBSTR expects 2 or 3 arguments".into()));
            }
            match (&evaluated[0], &evaluated[1], evaluated.get(2)) {
                (Value::Text(text), start, len) => {
                    let start = start.as_i64().unwrap_or(0);
                    let start = if start <= 0 { 0 } else { (start - 1) as usize };
                    let chars: Vec<char> = text.chars().collect();
                    if start >= chars.len() {
                        return Ok(Value::Text(String::new()));
                    }
                    let end = match len {
                        Some(Value::Int(n)) => (start + (*n as usize)).min(chars.len()),
                        Some(Value::Real(n)) => (start + (*n as usize)).min(chars.len()),
                        Some(Value::Null) => return Ok(Value::Null),
                        None => chars.len(),
                        _ => return Err(MiniDbError::Type("SUBSTR length must be numeric".into())),
                    };
                    Ok(Value::Text(chars[start..end].iter().collect()))
                }
                (Value::Null, _, _) => Ok(Value::Null),
                _ => Err(MiniDbError::Type(
                    "SUBSTR expects TEXT as first argument".into(),
                )),
            }
        }
        "ABS" => match evaluated.as_slice() {
            [Value::Int(v)] => Ok(Value::Int(v.abs())),
            [Value::Real(v)] => Ok(Value::Real(v.abs())),
            [Value::Null] => Ok(Value::Null),
            _ => Err(MiniDbError::Type("ABS expects one numeric argument".into())),
        },
        "COALESCE" => {
            for value in evaluated {
                if !value.is_null() {
                    return Ok(value);
                }
            }
            Ok(Value::Null)
        }
        other => Err(MiniDbError::Unsupported(format!(
            "unknown function {other}"
        ))),
    }
}

#[derive(Debug, Clone)]
pub struct SeqScan {
    pub table: TableRef,
    schema: Vec<SchemaColumn>,
    rows: Vec<Row>,
    pos: usize,
}

impl SeqScan {
    pub fn new(table: TableRef) -> Self {
        Self {
            table,
            schema: Vec::new(),
            rows: Vec::new(),
            pos: 0,
        }
    }
}

impl PhysicalOperator for SeqScan {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        let table = state.table(&self.table.name)?.clone();
        self.schema = table
            .schema
            .columns
            .iter()
            .map(|col| SchemaColumn {
                table: Some(
                    self.table
                        .alias
                        .clone()
                        .unwrap_or_else(|| self.table.name.clone()),
                ),
                name: col.name.clone(),
                col_type: col.col_type,
            })
            .collect();
        self.rows = table.heap.iter().map(|(_, row)| row.clone()).collect();
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos >= self.rows.len() {
            return Ok(None);
        }
        let row = self.rows[self.pos].clone();
        self.pos += 1;
        Ok(Some(row))
    }

    fn close(&mut self) -> Result<()> {
        debug!("closing SeqScan");
        Ok(())
    }

    fn schema(&self) -> &[SchemaColumn] {
        &self.schema
    }
}

#[derive(Debug, Clone)]
pub enum IndexPredicate {
    Eq(Value),
    Lt(Value),
    Lte(Value),
    Gt(Value),
    Gte(Value),
}

#[derive(Debug, Clone)]
pub struct IndexScan {
    pub table: TableRef,
    pub column: String,
    pub predicate: IndexPredicate,
    schema: Vec<SchemaColumn>,
    rows: Vec<Row>,
    pos: usize,
}

impl IndexScan {
    pub fn new(table: TableRef, column: String, predicate: IndexPredicate) -> Self {
        Self {
            table,
            column,
            predicate,
            schema: Vec::new(),
            rows: Vec::new(),
            pos: 0,
        }
    }
}

impl PhysicalOperator for IndexScan {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        let table_name = &self.table.name;
        let table = state.table(table_name)?.clone();
        let alias = self
            .table
            .alias
            .clone()
            .unwrap_or_else(|| table_name.clone());
        self.schema = table
            .schema
            .columns
            .iter()
            .map(|col| SchemaColumn {
                table: Some(alias.clone()),
                name: col.name.clone(),
                col_type: col.col_type,
            })
            .collect();
        let col_idx = table
            .schema
            .columns
            .iter()
            .position(|c| c.name == self.column)
            .ok_or_else(|| MiniDbError::Runtime(format!("unknown column {}", self.column)))?;
        let index = table
            .indexes
            .get(&self.column)
            .ok_or_else(|| MiniDbError::Runtime(format!("no index on {}", self.column)))?;
        let row_ids = match &self.predicate {
            IndexPredicate::Eq(v) => index.exact(v),
            IndexPredicate::Lt(v) => index.range(None, Some((v, false))),
            IndexPredicate::Lte(v) => index.range(None, Some((v, true))),
            IndexPredicate::Gt(v) => index.range(Some((v, false)), None),
            IndexPredicate::Gte(v) => index.range(Some((v, true)), None),
        };
        self.rows = row_ids
            .into_iter()
            .filter_map(|row_id| table.heap.get(row_id).cloned())
            .collect();
        self.pos = 0;
        debug!(
            "index scan on {} column {} at {}",
            table_name, self.column, col_idx
        );
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos >= self.rows.len() {
            return Ok(None);
        }
        let row = self.rows[self.pos].clone();
        self.pos += 1;
        Ok(Some(row))
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &[SchemaColumn] {
        &self.schema
    }
}

#[derive(Debug, Clone)]
pub struct Filter {
    pub child: Box<PlanNode>,
    pub predicate: Expr,
    pub(crate) schema: Vec<SchemaColumn>,
    pub(crate) rows: Vec<Row>,
    pub(crate) pos: usize,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub child: Box<PlanNode>,
    pub items: Vec<SelectItem>,
    pub(crate) schema: Vec<SchemaColumn>,
    pub(crate) rows: Vec<Row>,
    pub(crate) pos: usize,
}

#[derive(Debug, Clone)]
pub struct NestedLoopJoin {
    pub left: Box<PlanNode>,
    pub right: Box<PlanNode>,
    pub on: Expr,
    pub(crate) schema: Vec<SchemaColumn>,
    pub(crate) rows: Vec<Row>,
    pub(crate) pos: usize,
}

#[derive(Debug, Clone)]
pub struct Sort {
    pub child: Box<PlanNode>,
    pub order_by: Vec<OrderBy>,
    pub(crate) schema: Vec<SchemaColumn>,
    pub(crate) rows: Vec<Row>,
    pub(crate) pos: usize,
}

#[derive(Debug, Clone)]
pub struct Limit {
    pub child: Box<PlanNode>,
    pub limit: usize,
    pub offset: usize,
    pub(crate) schema: Vec<SchemaColumn>,
    pub(crate) rows: Vec<Row>,
    pub(crate) pos: usize,
}

#[derive(Debug, Clone)]
pub enum PlanNode {
    SeqScan(SeqScan),
    IndexScan(IndexScan),
    Filter(Filter),
    Project(Project),
    NestedLoopJoin(NestedLoopJoin),
    Sort(Sort),
    Limit(Limit),
}

impl PhysicalOperator for PlanNode {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        match self {
            PlanNode::SeqScan(op) => op.open(state),
            PlanNode::IndexScan(op) => op.open(state),
            PlanNode::Filter(op) => op.open(state),
            PlanNode::Project(op) => op.open(state),
            PlanNode::NestedLoopJoin(op) => op.open(state),
            PlanNode::Sort(op) => op.open(state),
            PlanNode::Limit(op) => op.open(state),
        }
    }

    fn next(&mut self) -> Result<Option<Row>> {
        match self {
            PlanNode::SeqScan(op) => op.next(),
            PlanNode::IndexScan(op) => op.next(),
            PlanNode::Filter(op) => op.next(),
            PlanNode::Project(op) => op.next(),
            PlanNode::NestedLoopJoin(op) => op.next(),
            PlanNode::Sort(op) => op.next(),
            PlanNode::Limit(op) => op.next(),
        }
    }

    fn close(&mut self) -> Result<()> {
        match self {
            PlanNode::SeqScan(op) => op.close(),
            PlanNode::IndexScan(op) => op.close(),
            PlanNode::Filter(op) => op.close(),
            PlanNode::Project(op) => op.close(),
            PlanNode::NestedLoopJoin(op) => op.close(),
            PlanNode::Sort(op) => op.close(),
            PlanNode::Limit(op) => op.close(),
        }
    }

    fn schema(&self) -> &[SchemaColumn] {
        match self {
            PlanNode::SeqScan(op) => op.schema(),
            PlanNode::IndexScan(op) => op.schema(),
            PlanNode::Filter(op) => op.schema(),
            PlanNode::Project(op) => op.schema(),
            PlanNode::NestedLoopJoin(op) => op.schema(),
            PlanNode::Sort(op) => op.schema(),
            PlanNode::Limit(op) => op.schema(),
        }
    }
}

fn materialize(
    child: &mut PlanNode,
    state: &mut DatabaseState,
) -> Result<(Vec<SchemaColumn>, Vec<Row>)> {
    child.open(state)?;
    let schema = child.schema().to_vec();
    let mut rows = Vec::new();
    while let Some(row) = child.next()? {
        rows.push(row);
    }
    child.close()?;
    Ok((schema, rows))
}

fn row_context(schema: &[SchemaColumn], row: &Row) -> RowContext {
    RowContext {
        columns: schema.to_vec(),
        values: row.clone(),
    }
}

impl PhysicalOperator for Filter {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        let (schema, rows) = materialize(&mut self.child, state)?;
        let mut filtered = Vec::new();
        for row in rows {
            let ctx = row_context(&schema, &row);
            if matches!(evaluate(&self.predicate, Some(&ctx))?, Value::Bool(true)) {
                filtered.push(row);
            }
        }
        self.schema = schema;
        self.rows = filtered;
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos >= self.rows.len() {
            return Ok(None);
        }
        let row = self.rows[self.pos].clone();
        self.pos += 1;
        Ok(Some(row))
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &[SchemaColumn] {
        &self.schema
    }
}

impl PhysicalOperator for Project {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        let (schema, rows) = materialize(&mut self.child, state)?;
        let mut projected_rows = Vec::new();
        let mut projected_schema = Vec::new();
        for item in &self.items {
            match item {
                SelectItem::Wildcard => projected_schema.extend(schema.iter().cloned()),
                SelectItem::Expr { expr, alias } => {
                    let name = alias.clone().unwrap_or_else(|| match expr {
                        Expr::ColumnRef { column, .. } => column.clone(),
                        _ => "expr".to_string(),
                    });
                    projected_schema.push(SchemaColumn {
                        table: None,
                        name,
                        col_type: infer_expr_type(expr, &schema),
                    });
                }
            }
        }
        for row in rows {
            let ctx = row_context(&schema, &row);
            let mut projected = Vec::new();
            for item in &self.items {
                match item {
                    SelectItem::Wildcard => projected.extend(row.clone()),
                    SelectItem::Expr { expr, .. } => projected.push(evaluate(expr, Some(&ctx))?),
                }
            }
            projected_rows.push(projected);
        }
        self.schema = projected_schema;
        self.rows = projected_rows;
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos >= self.rows.len() {
            return Ok(None);
        }
        let row = self.rows[self.pos].clone();
        self.pos += 1;
        Ok(Some(row))
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &[SchemaColumn] {
        &self.schema
    }
}

fn infer_expr_type(expr: &Expr, schema: &[SchemaColumn]) -> ColumnType {
    match expr {
        Expr::Literal(literal) => match literal {
            crate::types::Literal::Null => ColumnType::Text,
            crate::types::Literal::Int(_) => ColumnType::Int,
            crate::types::Literal::Real(_) => ColumnType::Real,
            crate::types::Literal::Text(_) => ColumnType::Text,
            crate::types::Literal::Bool(_) => ColumnType::Bool,
        },
        Expr::ColumnRef { table, column } => schema
            .iter()
            .find(|col| {
                col.name == *column
                    && table
                        .as_deref()
                        .map(|t| col.table.as_deref() == Some(t))
                        .unwrap_or(true)
            })
            .map(|col| col.col_type)
            .unwrap_or(ColumnType::Text),
        Expr::Unary { expr, .. } => infer_expr_type(expr, schema),
        Expr::Binary { left, op, right } => match op {
            BinaryOp::Concat => ColumnType::Text,
            BinaryOp::And
            | BinaryOp::Or
            | BinaryOp::Eq
            | BinaryOp::Neq
            | BinaryOp::Lt
            | BinaryOp::Lte
            | BinaryOp::Gt
            | BinaryOp::Gte => ColumnType::Bool,
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                let left_ty = infer_expr_type(left, schema);
                let right_ty = infer_expr_type(right, schema);
                if matches!(left_ty, ColumnType::Real)
                    || matches!(right_ty, ColumnType::Real)
                    || matches!(op, BinaryOp::Div)
                {
                    ColumnType::Real
                } else {
                    ColumnType::Int
                }
            }
        },
        Expr::Function { name, .. } => match name.to_ascii_uppercase().as_str() {
            "LENGTH" | "ABS" => ColumnType::Int,
            "LOWER" | "UPPER" | "SUBSTR" | "COALESCE" => ColumnType::Text,
            _ => ColumnType::Text,
        },
        Expr::Star | Expr::QualifiedStar { .. } => ColumnType::Text,
    }
}

impl PhysicalOperator for NestedLoopJoin {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        let (left_schema, left_rows) = materialize(&mut self.left, state)?;
        let (right_schema, right_rows) = materialize(&mut self.right, state)?;
        let mut schema = left_schema.clone();
        schema.extend(right_schema.clone());
        let mut rows = Vec::new();
        for lrow in &left_rows {
            for rrow in &right_rows {
                let mut combined = lrow.clone();
                combined.extend(rrow.clone());
                let ctx = row_context(&schema, &combined);
                if matches!(evaluate(&self.on, Some(&ctx))?, Value::Bool(true)) {
                    rows.push(combined);
                }
            }
        }
        self.schema = schema;
        self.rows = rows;
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos >= self.rows.len() {
            return Ok(None);
        }
        let row = self.rows[self.pos].clone();
        self.pos += 1;
        Ok(Some(row))
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &[SchemaColumn] {
        &self.schema
    }
}

impl PhysicalOperator for Sort {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        let (schema, mut rows) = materialize(&mut self.child, state)?;
        rows.sort_by(|a, b| {
            let ctx_a = row_context(&schema, a);
            let ctx_b = row_context(&schema, b);
            for order in &self.order_by {
                let va = evaluate(&order.expr, Some(&ctx_a));
                let vb = evaluate(&order.expr, Some(&ctx_b));
                let cmp = match (va, vb) {
                    (Ok(va), Ok(vb)) => va.total_cmp(&vb),
                    _ => Ordering::Equal,
                };
                if cmp != Ordering::Equal {
                    return if order.ascending { cmp } else { cmp.reverse() };
                }
            }
            Ordering::Equal
        });
        self.schema = schema;
        self.rows = rows;
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos >= self.rows.len() {
            return Ok(None);
        }
        let row = self.rows[self.pos].clone();
        self.pos += 1;
        Ok(Some(row))
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &[SchemaColumn] {
        &self.schema
    }
}

impl PhysicalOperator for Limit {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        let (schema, rows) = materialize(&mut self.child, state)?;
        self.schema = schema;
        self.rows = rows
            .into_iter()
            .skip(self.offset)
            .take(self.limit)
            .collect();
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos >= self.rows.len() {
            return Ok(None);
        }
        let row = self.rows[self.pos].clone();
        self.pos += 1;
        Ok(Some(row))
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &[SchemaColumn] {
        &self.schema
    }
}

#[derive(Debug, Clone, Default)]
pub struct Insert {
    schema: Vec<SchemaColumn>,
    affected: usize,
    yielded: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Update {
    schema: Vec<SchemaColumn>,
    affected: usize,
    yielded: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Delete {
    schema: Vec<SchemaColumn>,
    affected: usize,
    yielded: bool,
}

macro_rules! impl_mutation_operator {
    ($ty:ty) => {
        impl PhysicalOperator for $ty {
            fn open(&mut self, _state: &mut DatabaseState) -> Result<()> {
                self.yielded = false;
                Ok(())
            }

            fn next(&mut self) -> Result<Option<Row>> {
                if self.yielded {
                    return Ok(None);
                }
                self.yielded = true;
                Ok(Some(vec![Value::Int(self.affected as i64)]))
            }

            fn close(&mut self) -> Result<()> {
                Ok(())
            }

            fn schema(&self) -> &[SchemaColumn] {
                &self.schema
            }
        }
    };
}

impl_mutation_operator!(Insert);
impl_mutation_operator!(Update);
impl_mutation_operator!(Delete);

pub fn table_schema(table: &TableRef, data: &TableData) -> Vec<SchemaColumn> {
    let alias = table.alias.clone().unwrap_or_else(|| table.name.clone());
    data.schema
        .columns
        .iter()
        .map(|col| SchemaColumn {
            table: Some(alias.clone()),
            name: col.name.clone(),
            col_type: col.col_type,
        })
        .collect()
}
