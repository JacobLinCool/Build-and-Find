use crate::catalog::{IndexSchema, TableSchema};
use crate::error::{MiniDbError, Result};
use crate::parser::ast::{
    BinaryOp, Delete as DeleteStmt, Expr, Insert as InsertStmt, Literal, OrderByExpr, Select,
    SelectItem, Statement, UnaryOp, Update as UpdateStmt,
};
use crate::storage::btree::{IndexKey, RowId};
use crate::storage::heap::HeapTable;
use crate::storage::DatabaseState;
use crate::types::{ColumnType, Row, Schema, SchemaColumn, Value};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tuple {
    pub schema: Schema,
    pub values: Vec<Value>,
}

impl Tuple {
    pub fn new(schema: Schema, values: Vec<Value>) -> Self {
        Self { schema, values }
    }

    pub fn value(&self, table: Option<&str>, column: &str) -> Option<&Value> {
        let idx = self.schema.column_index(table, column)?;
        self.values.get(idx)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultSet {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExecutionResult {
    Query(ResultSet),
    AffectedRows(usize),
    Message(String),
    Empty,
}

#[derive(Debug)]
pub struct ExecContext<'a> {
    pub state: &'a mut DatabaseState,
}

pub trait PhysicalOperator {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()>;
    fn next(&mut self, ctx: &mut ExecContext<'_>) -> Result<Option<Tuple>>;
    fn close(&mut self, ctx: &mut ExecContext<'_>) -> Result<()>;
    fn schema(&self) -> Schema;
}

#[derive(Debug, Clone)]
pub struct SeqScan {
    pub table: String,
    pub alias: Option<String>,
    schema: Schema,
    rows: Vec<(RowId, Row)>,
    cursor: usize,
}

impl SeqScan {
    pub fn new(table: impl Into<String>, alias: Option<String>, schema: Schema) -> Self {
        Self {
            table: table.into(),
            alias,
            schema,
            rows: Vec::new(),
            cursor: 0,
        }
    }
}

impl PhysicalOperator for SeqScan {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        let table = ctx
            .state
            .tables
            .get(&self.table)
            .ok_or_else(|| MiniDbError::runtime(format!("unknown table {}", self.table)))?;
        self.rows = table
            .rows
            .iter()
            .map(|(row_id, row)| (*row_id, row.clone()))
            .collect();
        self.cursor = 0;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecContext<'_>) -> Result<Option<Tuple>> {
        if self.cursor >= self.rows.len() {
            return Ok(None);
        }
        let (_, row) = self.rows[self.cursor].clone();
        self.cursor += 1;
        Ok(Some(Tuple::new(self.schema.clone(), row.values)))
    }

    fn close(&mut self, _ctx: &mut ExecContext<'_>) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Schema {
        self.schema.clone()
    }
}

#[derive(Debug, Clone)]
pub enum IndexPredicate {
    Eq(Value),
    Range {
        lower: Option<(Value, bool)>,
        upper: Option<(Value, bool)>,
    },
}

#[derive(Debug, Clone)]
pub struct IndexScan {
    pub table: String,
    pub alias: Option<String>,
    pub index: String,
    pub schema: Schema,
    pub predicate: IndexPredicate,
    rows: Vec<(RowId, Row)>,
    cursor: usize,
}

impl IndexScan {
    pub fn new(
        table: impl Into<String>,
        alias: Option<String>,
        index: impl Into<String>,
        schema: Schema,
        predicate: IndexPredicate,
    ) -> Self {
        Self {
            table: table.into(),
            alias,
            index: index.into(),
            schema,
            predicate,
            rows: Vec::new(),
            cursor: 0,
        }
    }
}

impl PhysicalOperator for IndexScan {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        let table = ctx
            .state
            .tables
            .get(&self.table)
            .ok_or_else(|| MiniDbError::runtime(format!("unknown table {}", self.table)))?;
        let index = table
            .indexes
            .get(&self.index)
            .ok_or_else(|| MiniDbError::runtime(format!("unknown index {}", self.index)))?;
        let row_ids = match &self.predicate {
            IndexPredicate::Eq(value) => index.data.eq_search(&IndexKey::from_value(value)),
            IndexPredicate::Range { lower, upper } => {
                let lower = lower
                    .as_ref()
                    .map(|(value, inclusive)| (IndexKey::from_value(value), *inclusive));
                let upper = upper
                    .as_ref()
                    .map(|(value, inclusive)| (IndexKey::from_value(value), *inclusive));
                index.data.range_search(
                    lower.as_ref().map(|(key, inclusive)| (key, *inclusive)),
                    upper.as_ref().map(|(key, inclusive)| (key, *inclusive)),
                )
            }
        };
        self.rows = row_ids
            .into_iter()
            .filter_map(|row_id| table.rows.get(&row_id).cloned().map(|row| (row_id, row)))
            .collect();
        self.cursor = 0;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecContext<'_>) -> Result<Option<Tuple>> {
        if self.cursor >= self.rows.len() {
            return Ok(None);
        }
        let (_, row) = self.rows[self.cursor].clone();
        self.cursor += 1;
        Ok(Some(Tuple::new(self.schema.clone(), row.values)))
    }

    fn close(&mut self, _ctx: &mut ExecContext<'_>) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Schema {
        self.schema.clone()
    }
}

pub struct Filter {
    pub child: Box<dyn PhysicalOperator>,
    pub predicate: Expr,
    schema: Schema,
}

impl Filter {
    pub fn new(child: Box<dyn PhysicalOperator>, predicate: Expr) -> Self {
        let schema = child.schema();
        Self {
            child,
            predicate,
            schema,
        }
    }
}

impl PhysicalOperator for Filter {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.child.open(ctx)
    }

    fn next(&mut self, ctx: &mut ExecContext<'_>) -> Result<Option<Tuple>> {
        while let Some(tuple) = self.child.next(ctx)? {
            if eval_truthy(&self.predicate, &tuple)? {
                return Ok(Some(tuple));
            }
        }
        Ok(None)
    }

    fn close(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.child.close(ctx)
    }

    fn schema(&self) -> Schema {
        self.schema.clone()
    }
}

pub struct Project {
    pub child: Box<dyn PhysicalOperator>,
    pub items: Vec<SelectItem>,
    schema: Schema,
}

impl Project {
    pub fn new(child: Box<dyn PhysicalOperator>, items: Vec<SelectItem>) -> Self {
        let schema = project_schema(child.schema(), &items);
        Self {
            child,
            items,
            schema,
        }
    }
}

impl PhysicalOperator for Project {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.child.open(ctx)
    }

    fn next(&mut self, ctx: &mut ExecContext<'_>) -> Result<Option<Tuple>> {
        let tuple = match self.child.next(ctx)? {
            Some(tuple) => tuple,
            None => return Ok(None),
        };
        Ok(Some(project_tuple(&tuple, &self.items)))
    }

    fn close(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.child.close(ctx)
    }

    fn schema(&self) -> Schema {
        self.schema.clone()
    }
}

pub struct NestedLoopJoin {
    pub left: Box<dyn PhysicalOperator>,
    pub right: Box<dyn PhysicalOperator>,
    pub on: Expr,
    schema: Schema,
    pairs: Vec<Tuple>,
    cursor: usize,
}

impl NestedLoopJoin {
    pub fn new(
        left: Box<dyn PhysicalOperator>,
        right: Box<dyn PhysicalOperator>,
        on: Expr,
    ) -> Self {
        let mut columns = left.schema().columns;
        columns.extend(right.schema().columns);
        Self {
            left,
            right,
            on,
            schema: Schema { columns },
            pairs: Vec::new(),
            cursor: 0,
        }
    }
}

impl PhysicalOperator for NestedLoopJoin {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.left.open(ctx)?;
        self.right.open(ctx)?;
        let mut left_rows = Vec::new();
        while let Some(tuple) = self.left.next(ctx)? {
            left_rows.push(tuple);
        }
        let mut right_rows = Vec::new();
        while let Some(tuple) = self.right.next(ctx)? {
            right_rows.push(tuple);
        }
        let mut pairs = Vec::new();
        for left in &left_rows {
            for right in &right_rows {
                let tuple = join_tuples(left, right);
                if eval_truthy(&self.on, &tuple)? {
                    pairs.push(tuple);
                }
            }
        }
        self.pairs = pairs;
        self.cursor = 0;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecContext<'_>) -> Result<Option<Tuple>> {
        if self.cursor >= self.pairs.len() {
            return Ok(None);
        }
        let tuple = self.pairs[self.cursor].clone();
        self.cursor += 1;
        Ok(Some(tuple))
    }

    fn close(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.left.close(ctx)?;
        self.right.close(ctx)
    }

    fn schema(&self) -> Schema {
        self.schema.clone()
    }
}

pub struct Sort {
    pub child: Box<dyn PhysicalOperator>,
    pub order_by: Vec<OrderByExpr>,
    schema: Schema,
    rows: Vec<Tuple>,
    cursor: usize,
}

impl Sort {
    pub fn new(child: Box<dyn PhysicalOperator>, order_by: Vec<OrderByExpr>) -> Self {
        let schema = child.schema();
        Self {
            child,
            order_by,
            schema,
            rows: Vec::new(),
            cursor: 0,
        }
    }
}

impl PhysicalOperator for Sort {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.child.open(ctx)?;
        let mut rows = Vec::new();
        while let Some(tuple) = self.child.next(ctx)? {
            rows.push(tuple);
        }
        rows.sort_by(|a, b| compare_order_rows(a, b, &self.order_by).unwrap_or(Ordering::Equal));
        self.rows = rows;
        self.cursor = 0;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecContext<'_>) -> Result<Option<Tuple>> {
        if self.cursor >= self.rows.len() {
            return Ok(None);
        }
        let tuple = self.rows[self.cursor].clone();
        self.cursor += 1;
        Ok(Some(tuple))
    }

    fn close(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.child.close(ctx)
    }

    fn schema(&self) -> Schema {
        self.schema.clone()
    }
}

pub struct Limit {
    pub child: Box<dyn PhysicalOperator>,
    pub limit: usize,
    pub offset: usize,
    schema: Schema,
    seen: usize,
    emitted: usize,
}

impl Limit {
    pub fn new(child: Box<dyn PhysicalOperator>, limit: usize, offset: usize) -> Self {
        let schema = child.schema();
        Self {
            child,
            limit,
            offset,
            schema,
            seen: 0,
            emitted: 0,
        }
    }
}

impl PhysicalOperator for Limit {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.child.open(ctx)?;
        self.seen = 0;
        self.emitted = 0;
        Ok(())
    }

    fn next(&mut self, ctx: &mut ExecContext<'_>) -> Result<Option<Tuple>> {
        while self.seen < self.offset {
            if self.child.next(ctx)?.is_none() {
                return Ok(None);
            }
            self.seen += 1;
        }
        if self.emitted >= self.limit {
            return Ok(None);
        }
        if let Some(tuple) = self.child.next(ctx)? {
            self.emitted += 1;
            Ok(Some(tuple))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.child.close(ctx)
    }

    fn schema(&self) -> Schema {
        self.schema.clone()
    }
}

pub struct Insert {
    pub statement: InsertStmt,
    affected: usize,
    emitted: bool,
}

impl Insert {
    pub fn new(statement: InsertStmt) -> Self {
        Self {
            statement,
            affected: 0,
            emitted: false,
        }
    }
}

impl PhysicalOperator for Insert {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.affected = insert_row(ctx.state, &self.statement)?;
        self.emitted = false;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecContext<'_>) -> Result<Option<Tuple>> {
        if self.emitted {
            return Ok(None);
        }
        self.emitted = true;
        Ok(Some(Tuple::new(
            Schema {
                columns: vec![SchemaColumn {
                    table: None,
                    name: "affected".into(),
                    col_type: ColumnType::Int,
                }],
            },
            vec![Value::Int(self.affected as i64)],
        )))
    }

    fn close(&mut self, _ctx: &mut ExecContext<'_>) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Schema {
        Schema {
            columns: vec![SchemaColumn {
                table: None,
                name: "affected".into(),
                col_type: ColumnType::Int,
            }],
        }
    }
}

pub struct Update {
    pub statement: UpdateStmt,
    affected: usize,
    emitted: bool,
}

impl Update {
    pub fn new(statement: UpdateStmt) -> Self {
        Self {
            statement,
            affected: 0,
            emitted: false,
        }
    }
}

impl PhysicalOperator for Update {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.affected = update_rows(ctx.state, &self.statement)?;
        self.emitted = false;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecContext<'_>) -> Result<Option<Tuple>> {
        if self.emitted {
            return Ok(None);
        }
        self.emitted = true;
        Ok(Some(Tuple::new(
            Schema {
                columns: vec![SchemaColumn {
                    table: None,
                    name: "affected".into(),
                    col_type: ColumnType::Int,
                }],
            },
            vec![Value::Int(self.affected as i64)],
        )))
    }

    fn close(&mut self, _ctx: &mut ExecContext<'_>) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Schema {
        Schema {
            columns: vec![SchemaColumn {
                table: None,
                name: "affected".into(),
                col_type: ColumnType::Int,
            }],
        }
    }
}

pub struct Delete {
    pub statement: DeleteStmt,
    affected: usize,
    emitted: bool,
}

impl Delete {
    pub fn new(statement: DeleteStmt) -> Self {
        Self {
            statement,
            affected: 0,
            emitted: false,
        }
    }
}

impl PhysicalOperator for Delete {
    fn open(&mut self, ctx: &mut ExecContext<'_>) -> Result<()> {
        self.affected = delete_rows(ctx.state, &self.statement)?;
        self.emitted = false;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecContext<'_>) -> Result<Option<Tuple>> {
        if self.emitted {
            return Ok(None);
        }
        self.emitted = true;
        Ok(Some(Tuple::new(
            Schema {
                columns: vec![SchemaColumn {
                    table: None,
                    name: "affected".into(),
                    col_type: ColumnType::Int,
                }],
            },
            vec![Value::Int(self.affected as i64)],
        )))
    }

    fn close(&mut self, _ctx: &mut ExecContext<'_>) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Schema {
        Schema {
            columns: vec![SchemaColumn {
                table: None,
                name: "affected".into(),
                col_type: ColumnType::Int,
            }],
        }
    }
}

fn join_tuples(left: &Tuple, right: &Tuple) -> Tuple {
    let mut schema = left.schema.clone();
    schema.columns.extend(right.schema.columns.clone());
    let mut values = left.values.clone();
    values.extend(right.values.clone());
    Tuple { schema, values }
}

fn project_schema(input: Schema, items: &[SelectItem]) -> Schema {
    let mut columns = Vec::new();
    for item in items {
        match item {
            SelectItem::Wildcard => columns.extend(input.columns.clone()),
            SelectItem::QualifiedWildcard(table) => {
                columns.extend(
                    input
                        .columns
                        .iter()
                        .filter(|c| c.table.as_deref() == Some(table))
                        .cloned(),
                );
            }
            SelectItem::Expr { expr, alias } => {
                let name = alias.clone().unwrap_or_else(|| expr_display_name(expr));
                columns.push(SchemaColumn {
                    table: None,
                    name,
                    col_type: ColumnType::Text,
                });
            }
        }
    }
    Schema { columns }
}

fn project_tuple(tuple: &Tuple, items: &[SelectItem]) -> Tuple {
    let mut values = Vec::new();
    let mut columns = Vec::new();
    for item in items {
        match item {
            SelectItem::Wildcard => {
                columns.extend(tuple.schema.columns.clone());
                values.extend(tuple.values.clone());
            }
            SelectItem::QualifiedWildcard(table) => {
                for (idx, column) in tuple.schema.columns.iter().enumerate() {
                    if column.table.as_deref() == Some(table) {
                        columns.push(column.clone());
                        values.push(tuple.values[idx].clone());
                    }
                }
            }
            SelectItem::Expr { expr, alias } => {
                let name = alias.clone().unwrap_or_else(|| expr_display_name(expr));
                columns.push(SchemaColumn {
                    table: None,
                    name,
                    col_type: ColumnType::Text,
                });
                values.push(eval_expr(expr, tuple).unwrap_or(Value::Null));
            }
        }
    }
    Tuple {
        schema: Schema { columns },
        values,
    }
}

fn compare_order_rows(a: &Tuple, b: &Tuple, order_by: &[OrderByExpr]) -> Result<Ordering> {
    for order in order_by {
        let left = eval_expr(&order.expr, a)?;
        let right = eval_expr(&order.expr, b)?;
        let ord = compare_values(&left, &right).unwrap_or(Ordering::Equal);
        if ord != Ordering::Equal {
            return Ok(if order.asc { ord } else { ord.reverse() });
        }
    }
    Ok(Ordering::Equal)
}

fn expr_display_name(expr: &Expr) -> String {
    match expr {
        Expr::ColumnRef { column, .. } => column.clone(),
        Expr::Function { name, .. } => name.clone(),
        _ => "expr".into(),
    }
}

fn eval_truthy(expr: &Expr, tuple: &Tuple) -> Result<bool> {
    match eval_expr(expr, tuple)? {
        Value::Bool(value) => Ok(value),
        Value::Null => Ok(false),
        other => Err(MiniDbError::type_error(format!(
            "expected boolean expression, found {}",
            other.type_name()
        ))),
    }
}

pub fn eval_expr(expr: &Expr, tuple: &Tuple) -> Result<Value> {
    match expr {
        Expr::Literal(lit) => Ok(match lit {
            Literal::Null => Value::Null,
            Literal::Bool(v) => Value::Bool(*v),
            Literal::Int(v) => Value::Int(*v),
            Literal::Real(v) => Value::Real(*v),
            Literal::Text(v) => Value::Text(v.clone()),
        }),
        Expr::ColumnRef { table, column } => tuple
            .value(table.as_deref(), column)
            .cloned()
            .ok_or_else(|| MiniDbError::runtime(format!("unknown column {column}"))),
        Expr::Unary { op, expr } => {
            let value = eval_expr(expr, tuple)?;
            match op {
                UnaryOp::Not => match value {
                    Value::Bool(v) => Ok(Value::Bool(!v)),
                    Value::Null => Ok(Value::Null),
                    other => Err(MiniDbError::type_error(format!(
                        "NOT expects boolean, found {}",
                        other.type_name()
                    ))),
                },
                UnaryOp::Neg => match value {
                    Value::Int(v) => Ok(Value::Int(-v)),
                    Value::Real(v) => Ok(Value::Real(-v)),
                    Value::Null => Ok(Value::Null),
                    other => Err(MiniDbError::type_error(format!(
                        "unary - expects numeric, found {}",
                        other.type_name()
                    ))),
                },
            }
        }
        Expr::Binary { left, op, right } => {
            let left_value = eval_expr(left, tuple)?;
            let right_value = eval_expr(right, tuple)?;
            match op {
                BinaryOp::Or => Ok(Value::Bool(truth_or(&left_value, &right_value))),
                BinaryOp::And => Ok(Value::Bool(truth_and(&left_value, &right_value))),
                BinaryOp::Eq
                | BinaryOp::Neq
                | BinaryOp::Lt
                | BinaryOp::Lte
                | BinaryOp::Gt
                | BinaryOp::Gte => Ok(compare_binary(&left_value, &right_value, op)?),
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                    arithmetic_binary(&left_value, &right_value, op)
                }
                BinaryOp::Concat => match (left_value, right_value) {
                    (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                    (Value::Text(a), Value::Text(b)) => Ok(Value::Text(format!("{a}{b}"))),
                    (a, b) => Err(MiniDbError::type_error(format!(
                        "|| expects text operands, found {} and {}",
                        a.type_name(),
                        b.type_name()
                    ))),
                },
            }
        }
        Expr::Function { name, args } => eval_function(name, args, tuple),
        Expr::IsNull { expr, negated } => {
            let value = eval_expr(expr, tuple)?;
            Ok(Value::Bool(if *negated {
                !value.is_null()
            } else {
                value.is_null()
            }))
        }
    }
}

fn eval_function(name: &str, args: &[Expr], tuple: &Tuple) -> Result<Value> {
    let upper = name.to_ascii_uppercase();
    match upper.as_str() {
        "LENGTH" => {
            let arg = eval_arg(args.get(0), tuple)?;
            match arg {
                Value::Text(text) => Ok(Value::Int(text.chars().count() as i64)),
                Value::Null => Ok(Value::Null),
                other => Err(MiniDbError::type_error(format!(
                    "LENGTH expects text, found {}",
                    other.type_name()
                ))),
            }
        }
        "LOWER" => {
            let arg = eval_arg(args.get(0), tuple)?;
            match arg {
                Value::Text(text) => Ok(Value::Text(text.to_lowercase())),
                Value::Null => Ok(Value::Null),
                other => Err(MiniDbError::type_error(format!(
                    "LOWER expects text, found {}",
                    other.type_name()
                ))),
            }
        }
        "UPPER" => {
            let arg = eval_arg(args.get(0), tuple)?;
            match arg {
                Value::Text(text) => Ok(Value::Text(text.to_uppercase())),
                Value::Null => Ok(Value::Null),
                other => Err(MiniDbError::type_error(format!(
                    "UPPER expects text, found {}",
                    other.type_name()
                ))),
            }
        }
        "SUBSTR" => {
            let text = eval_arg(args.get(0), tuple)?;
            let start = eval_arg(args.get(1), tuple)?;
            let len = args.get(2).map(|expr| eval_expr(expr, tuple)).transpose()?;
            match (text, start) {
                (Value::Text(text), Value::Int(start)) => {
                    let start = start.max(1) as usize - 1;
                    let chars: Vec<char> = text.chars().collect();
                    if start >= chars.len() {
                        return Ok(Value::Text(String::new()));
                    }
                    let end = match len {
                        Some(Value::Int(len)) => start.saturating_add(len.max(0) as usize),
                        _ => chars.len(),
                    };
                    let slice: String = chars[start..end.min(chars.len())].iter().collect();
                    Ok(Value::Text(slice))
                }
                (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                (a, b) => Err(MiniDbError::type_error(format!(
                    "SUBSTR expects text, int, [int], found {} and {}",
                    a.type_name(),
                    b.type_name()
                ))),
            }
        }
        "ABS" => {
            let arg = eval_arg(args.get(0), tuple)?;
            match arg {
                Value::Int(v) => Ok(Value::Int(v.abs())),
                Value::Real(v) => Ok(Value::Real(v.abs())),
                Value::Null => Ok(Value::Null),
                other => Err(MiniDbError::type_error(format!(
                    "ABS expects numeric, found {}",
                    other.type_name()
                ))),
            }
        }
        "COALESCE" => {
            for arg in args {
                let value = eval_expr(arg, tuple)?;
                if !value.is_null() {
                    return Ok(value);
                }
            }
            Ok(Value::Null)
        }
        _ => Err(MiniDbError::runtime(format!("unknown function {name}"))),
    }
}

fn eval_arg(expr: Option<&Expr>, tuple: &Tuple) -> Result<Value> {
    expr.map(|expr| eval_expr(expr, tuple))
        .transpose()?
        .ok_or_else(|| MiniDbError::runtime("missing function argument"))
}

fn truth_and(left: &Value, right: &Value) -> bool {
    match (left.as_bool(), right.as_bool()) {
        (Some(false), _) | (_, Some(false)) => false,
        (Some(true), Some(true)) => true,
        _ => false,
    }
}

fn truth_or(left: &Value, right: &Value) -> bool {
    match (left.as_bool(), right.as_bool()) {
        (Some(true), _) | (_, Some(true)) => true,
        (Some(false), Some(false)) => false,
        _ => false,
    }
}

fn compare_binary(left: &Value, right: &Value, op: &BinaryOp) -> Result<Value> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    let ord = compare_values(left, right)
        .ok_or_else(|| MiniDbError::type_error("incompatible types for comparison"))?;
    let result = match op {
        BinaryOp::Eq => ord == Ordering::Equal,
        BinaryOp::Neq => ord != Ordering::Equal,
        BinaryOp::Lt => ord == Ordering::Less,
        BinaryOp::Lte => ord == Ordering::Less || ord == Ordering::Equal,
        BinaryOp::Gt => ord == Ordering::Greater,
        BinaryOp::Gte => ord == Ordering::Greater || ord == Ordering::Equal,
        _ => unreachable!(),
    };
    Ok(Value::Bool(result))
}

fn compare_values(left: &Value, right: &Value) -> Option<Ordering> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Some(a.cmp(b)),
        (Value::Real(a), Value::Real(b)) => Some(a.total_cmp(b)),
        (Value::Int(a), Value::Real(b)) => Some((*a as f64).total_cmp(b)),
        (Value::Real(a), Value::Int(b)) => Some(a.total_cmp(&(*b as f64))),
        (Value::Text(a), Value::Text(b)) => Some(a.cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

fn arithmetic_binary(left: &Value, right: &Value, op: &BinaryOp) -> Result<Value> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(match op {
            BinaryOp::Add => Value::Int(a + b),
            BinaryOp::Sub => Value::Int(a - b),
            BinaryOp::Mul => Value::Int(a * b),
            BinaryOp::Div => Value::Int(a / b),
            _ => unreachable!(),
        }),
        (Value::Int(a), Value::Real(b)) => arithmetic_real(*a as f64, *b, op),
        (Value::Real(a), Value::Int(b)) => arithmetic_real(*a, *b as f64, op),
        (Value::Real(a), Value::Real(b)) => arithmetic_real(*a, *b, op),
        (a, b) => Err(MiniDbError::type_error(format!(
            "arithmetic expects numeric operands, found {} and {}",
            a.type_name(),
            b.type_name()
        ))),
    }
}

fn arithmetic_real(left: f64, right: f64, op: &BinaryOp) -> Result<Value> {
    let value = match op {
        BinaryOp::Add => left + right,
        BinaryOp::Sub => left - right,
        BinaryOp::Mul => left * right,
        BinaryOp::Div => left / right,
        _ => unreachable!(),
    };
    Ok(Value::Real(value))
}

pub fn table_schema_for_alias(schema: &TableSchema, alias: Option<&str>) -> Schema {
    Schema {
        columns: schema
            .columns
            .iter()
            .map(|column| SchemaColumn {
                table: Some(alias.unwrap_or(&schema.name).to_string()),
                name: column.name.clone(),
                col_type: column.col_type.clone(),
            })
            .collect(),
    }
}

pub fn execute_statement(
    state: &mut DatabaseState,
    statement: &Statement,
) -> Result<ExecutionResult> {
    match statement {
        Statement::CreateTable(_)
        | Statement::DropTable { .. }
        | Statement::CreateIndex { .. }
        | Statement::DropIndex { .. }
        | Statement::Insert(_)
        | Statement::Update(_)
        | Statement::Delete(_)
        | Statement::Select(_)
        | Statement::Begin
        | Statement::Commit
        | Statement::Rollback => apply_statement_direct(state, statement),
    }
}

pub fn apply_statement_direct(
    state: &mut DatabaseState,
    statement: &Statement,
) -> Result<ExecutionResult> {
    match statement {
        Statement::CreateTable(create) => {
            create_table(state, create)?;
            Ok(ExecutionResult::Empty)
        }
        Statement::DropTable { name } => {
            state.catalog.drop_table(name);
            state.tables.remove(name);
            Ok(ExecutionResult::Empty)
        }
        Statement::CreateIndex {
            name,
            table,
            column,
        } => {
            create_index(state, name, table, column)?;
            Ok(ExecutionResult::Empty)
        }
        Statement::DropIndex { name } => {
            drop_index(state, name)?;
            Ok(ExecutionResult::Empty)
        }
        Statement::Insert(insert) => {
            let count = insert_row(state, insert)?;
            Ok(ExecutionResult::AffectedRows(count))
        }
        Statement::Update(update) => {
            let count = update_rows(state, update)?;
            Ok(ExecutionResult::AffectedRows(count))
        }
        Statement::Delete(delete) => {
            let count = delete_rows(state, delete)?;
            Ok(ExecutionResult::AffectedRows(count))
        }
        Statement::Select(select) => {
            let result = execute_select(state, select)?;
            Ok(ExecutionResult::Query(result))
        }
        Statement::Begin => Ok(ExecutionResult::Message("transaction started".into())),
        Statement::Commit => Ok(ExecutionResult::Message("transaction committed".into())),
        Statement::Rollback => Ok(ExecutionResult::Message("transaction rolled back".into())),
    }
}

fn create_table(state: &mut DatabaseState, create: &crate::parser::ast::CreateTable) -> Result<()> {
    if state.catalog.tables.contains_key(&create.name) {
        return Err(MiniDbError::runtime(format!(
            "table {} already exists",
            create.name
        )));
    }
    let mut columns = Vec::new();
    for col in &create.columns {
        let default = match &col.default {
            Some(expr) => Some(eval_constant(expr)?),
            None => None,
        };
        columns.push(col.to_column(default));
    }
    let schema = TableSchema {
        name: create.name.clone(),
        columns,
        checks: create.checks.clone(),
    };
    state.catalog.create_table(schema.clone());
    state
        .tables
        .insert(create.name.clone(), HeapTable::new(schema));
    Ok(())
}

fn create_index(state: &mut DatabaseState, name: &str, table: &str, column: &str) -> Result<()> {
    if state.catalog.indexes.contains_key(name) {
        return Err(MiniDbError::runtime(format!(
            "index {} already exists",
            name
        )));
    }
    let schema = state
        .catalog
        .table(table)
        .ok_or_else(|| MiniDbError::runtime(format!("unknown table {}", table)))?
        .clone();
    if schema.column_index(column).is_none() {
        return Err(MiniDbError::runtime(format!(
            "unknown column {} on table {}",
            column, table
        )));
    }
    let index_schema = IndexSchema {
        name: name.into(),
        table: table.into(),
        column: column.into(),
    };
    state.catalog.create_index(index_schema.clone());
    state
        .tables
        .get_mut(table)
        .ok_or_else(|| MiniDbError::runtime(format!("unknown table {}", table)))?
        .add_index(index_schema)?;
    Ok(())
}

fn drop_index(state: &mut DatabaseState, name: &str) -> Result<()> {
    let index = state
        .catalog
        .indexes
        .remove(name)
        .ok_or_else(|| MiniDbError::runtime(format!("unknown index {}", name)))?;
    if let Some(table) = state.tables.get_mut(&index.table) {
        table.remove_index(name);
    }
    Ok(())
}

fn insert_row(state: &mut DatabaseState, insert: &InsertStmt) -> Result<usize> {
    let table_schema = state
        .catalog
        .table(&insert.table)
        .ok_or_else(|| MiniDbError::runtime(format!("unknown table {}", insert.table)))?
        .clone();
    let table = state
        .tables
        .get_mut(&insert.table)
        .ok_or_else(|| MiniDbError::runtime(format!("unknown table {}", insert.table)))?;
    let row = build_insert_row(&table_schema, insert)?;
    validate_row(&table_schema, &row)?;
    let row_id = table.insert_row(row.clone())?;
    enforce_unique_constraints(table, row_id, &row)?;
    Ok(1)
}

fn build_insert_row(table_schema: &TableSchema, insert: &InsertStmt) -> Result<Row> {
    let mut values = vec![Value::Null; table_schema.columns.len()];
    if let Some(columns) = &insert.columns {
        if columns.len() != insert.values.len() {
            return Err(MiniDbError::runtime(
                "column count does not match value count",
            ));
        }
        for (column_name, expr) in columns.iter().zip(&insert.values) {
            let idx = table_schema
                .column_index(column_name)
                .ok_or_else(|| MiniDbError::runtime(format!("unknown column {}", column_name)))?;
            values[idx] = eval_constant(expr)?;
        }
    } else {
        if insert.values.len() != table_schema.columns.len() {
            return Err(MiniDbError::runtime(
                "value count does not match table column count",
            ));
        }
        for (idx, expr) in insert.values.iter().enumerate() {
            values[idx] = eval_constant(expr)?;
        }
    }
    for (idx, column) in table_schema.columns.iter().enumerate() {
        if matches!(values[idx], Value::Null) {
            if let Some(default) = &column.default {
                values[idx] = default.clone();
            }
        }
    }
    Ok(Row::new(values))
}

fn update_rows(state: &mut DatabaseState, update: &UpdateStmt) -> Result<usize> {
    let table_schema = state
        .catalog
        .table(&update.table)
        .ok_or_else(|| MiniDbError::runtime(format!("unknown table {}", update.table)))?
        .clone();
    let table = state
        .tables
        .get_mut(&update.table)
        .ok_or_else(|| MiniDbError::runtime(format!("unknown table {}", update.table)))?;
    let mut count = 0usize;
    let row_ids: Vec<RowId> = table.rows.keys().copied().collect();
    for row_id in row_ids {
        let Some(current) = table.rows.get(&row_id).cloned() else {
            continue;
        };
        let tuple = row_to_tuple(&table_schema, &current);
        if update
            .selection
            .as_ref()
            .map(|expr| eval_truthy(expr, &tuple))
            .transpose()?
            .unwrap_or(true)
        {
            let mut values = current.values.clone();
            for (column_name, expr) in &update.assignments {
                let idx = table_schema.column_index(column_name).ok_or_else(|| {
                    MiniDbError::runtime(format!("unknown column {}", column_name))
                })?;
                values[idx] = eval_expr(expr, &tuple)?;
            }
            let new_row = Row::new(values);
            validate_row(&table_schema, &new_row)?;
            table.update_row(row_id, new_row);
            count += 1;
        }
    }
    enforce_table_constraints(table, &table_schema)?;
    Ok(count)
}

fn delete_rows(state: &mut DatabaseState, delete: &DeleteStmt) -> Result<usize> {
    let table_schema = state
        .catalog
        .table(&delete.table)
        .ok_or_else(|| MiniDbError::runtime(format!("unknown table {}", delete.table)))?
        .clone();
    let table = state
        .tables
        .get_mut(&delete.table)
        .ok_or_else(|| MiniDbError::runtime(format!("unknown table {}", delete.table)))?;
    let row_ids: Vec<RowId> = table.rows.keys().copied().collect();
    let mut count = 0usize;
    for row_id in row_ids {
        let Some(current) = table.rows.get(&row_id).cloned() else {
            continue;
        };
        let tuple = row_to_tuple(&table_schema, &current);
        if delete
            .selection
            .as_ref()
            .map(|expr| eval_truthy(expr, &tuple))
            .transpose()?
            .unwrap_or(true)
        {
            table.delete_row(row_id);
            count += 1;
        }
    }
    Ok(count)
}

fn execute_select(state: &mut DatabaseState, select: &Select) -> Result<ResultSet> {
    let plan = crate::plan::physical::build_select_plan(state, select)?;
    let mut ctx = ExecContext { state };
    let mut root = plan.root;
    root.open(&mut ctx)?;
    let schema = root.schema();
    let columns = schema.columns.iter().map(|c| c.name.clone()).collect();
    let mut rows = Vec::new();
    while let Some(tuple) = root.next(&mut ctx)? {
        rows.push(tuple.values);
    }
    root.close(&mut ctx)?;
    Ok(ResultSet { columns, rows })
}

fn row_to_tuple(schema: &TableSchema, row: &Row) -> Tuple {
    Tuple::new(schema.schema(), row.values.clone())
}

fn validate_row(table_schema: &TableSchema, row: &Row) -> Result<()> {
    for (idx, column) in table_schema.columns.iter().enumerate() {
        let value = &row.values[idx];
        if column.not_null && value.is_null() {
            return Err(MiniDbError::runtime(format!(
                "column {} may not be null",
                column.name
            )));
        }
        if !value.is_null() && !value_matches_type(value, &column.col_type) {
            return Err(MiniDbError::type_error(format!(
                "column {} expects {:?}",
                column.name, column.col_type
            )));
        }
    }
    Ok(())
}

fn value_matches_type(value: &Value, ty: &ColumnType) -> bool {
    match (value, ty) {
        (Value::Null, _) => true,
        (Value::Int(_), ColumnType::Int) | (Value::Int(_), ColumnType::BigInt) => true,
        (Value::Real(_), ColumnType::Real) => true,
        (Value::Text(_), ColumnType::Text) => true,
        (Value::Bool(_), ColumnType::Bool) => true,
        _ => false,
    }
}

fn enforce_unique_constraints(table: &HeapTable, row_id: RowId, row: &Row) -> Result<()> {
    for column in &table.schema.columns {
        if column.unique || column.primary_key {
            if let Some(idx) = table.schema.column_index(&column.name) {
                let target = &row.values[idx];
                if target.is_null() && !column.primary_key {
                    continue;
                }
                for (other_row_id, other_row) in &table.rows {
                    if *other_row_id == row_id {
                        continue;
                    }
                    if &other_row.values[idx] == target {
                        return Err(MiniDbError::runtime(format!(
                            "unique constraint violation on {}",
                            column.name
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

fn enforce_table_constraints(table: &HeapTable, schema: &TableSchema) -> Result<()> {
    for (row_id, row) in &table.rows {
        enforce_unique_constraints(table, *row_id, row)?;
        validate_row(schema, row)?;
        let tuple = row_to_tuple(schema, row);
        for check in &schema.checks {
            if !eval_truthy(check, &tuple)? {
                return Err(MiniDbError::runtime("CHECK constraint failed"));
            }
        }
    }
    Ok(())
}

fn eval_constant(expr: &Expr) -> Result<Value> {
    let tuple = Tuple::new(Schema::empty(), Vec::new());
    eval_expr(expr, &tuple)
}

pub fn summarize_result(result: &ExecutionResult) -> Option<String> {
    match result {
        ExecutionResult::Query(result) => Some(format!("ok {} rows", result.rows.len())),
        ExecutionResult::AffectedRows(count) => Some(format!(
            "ok {} row{}",
            count,
            if *count == 1 { "" } else { "s" }
        )),
        ExecutionResult::Message(message) => Some(format!("ok {message}")),
        ExecutionResult::Empty => Some("ok".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::{ColumnDefinition, CreateTable, Literal, Statement};
    use crate::types::ColumnType;

    #[test]
    fn evaluates_basic_arithmetic() {
        let tuple = Tuple::new(Schema::empty(), vec![]);
        let expr = Expr::Binary {
            left: Box::new(Expr::Literal(Literal::Int(1))),
            op: BinaryOp::Add,
            right: Box::new(Expr::Literal(Literal::Int(2))),
        };
        assert_eq!(eval_expr(&expr, &tuple).unwrap(), Value::Int(3));
    }

    #[test]
    fn insert_uses_defaults() {
        let mut state = DatabaseState::default();
        let stmt = Statement::CreateTable(CreateTable {
            name: "t".into(),
            columns: vec![ColumnDefinition {
                name: "id".into(),
                col_type: ColumnType::Int,
                not_null: true,
                unique: true,
                primary_key: true,
                default: None,
            }],
            checks: vec![],
        });
        apply_statement_direct(&mut state, &stmt).unwrap();
    }
}
