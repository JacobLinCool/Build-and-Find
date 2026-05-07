use crate::catalog::{normalize, TableSchema};
use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;
use crate::plan::{PhysicalPlan, Planner};
use crate::storage::heap::StoredRow;
use crate::storage::wal::WalOp;
use crate::storage::{next_row_id, Database, DatabaseState};
use crate::types::{coerce_numeric, Truth, Value};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct ColumnRef {
    pub table: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ExecRow {
    pub values: Vec<Value>,
    pub columns: Vec<ColumnRef>,
    pub row_ids: BTreeMap<String, u64>,
}

pub trait PhysicalOperator {
    fn open(&mut self) -> Result<()>;
    fn next(&mut self) -> Result<Option<ExecRow>>;
    fn close(&mut self) -> Result<()>;
    fn schema(&self) -> Vec<ColumnRef>;
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub affected: Option<usize>,
}

impl QueryResult {
    pub fn ok() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            affected: Some(0),
        }
    }

    pub fn affected(n: usize) -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            affected: Some(n),
        }
    }
}

pub struct Engine {
    pub db: Database,
}

impl Engine {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        Ok(Self {
            db: Database::open(path)?,
        })
    }

    pub fn execute(&mut self, stmt: Statement) -> Result<QueryResult> {
        match stmt {
            Statement::Begin => {
                self.db.begin()?;
                Ok(QueryResult::ok())
            }
            Statement::Commit => {
                self.db.commit()?;
                Ok(QueryResult::ok())
            }
            Statement::Rollback => {
                self.db.rollback()?;
                Ok(QueryResult::ok())
            }
            Statement::CreateTable {
                name,
                columns,
                checks,
            } => self.with_auto_tx(|db| {
                db.append_and_apply(WalOp::CreateTable {
                    name,
                    columns,
                    checks,
                })?;
                Ok(QueryResult::ok())
            }),
            Statement::DropTable { name } => self.with_auto_tx(|db| {
                db.append_and_apply(WalOp::DropTable { name })?;
                Ok(QueryResult::ok())
            }),
            Statement::CreateIndex {
                name,
                table,
                column,
            } => self.with_auto_tx(|db| {
                db.append_and_apply(WalOp::CreateIndex {
                    name,
                    table,
                    column,
                })?;
                Ok(QueryResult::ok())
            }),
            Statement::DropIndex { name } => self.with_auto_tx(|db| {
                db.append_and_apply(WalOp::DropIndex { name })?;
                Ok(QueryResult::ok())
            }),
            Statement::Insert {
                table,
                columns,
                values,
            } => self.with_auto_tx(|db| Insert::new(table, columns, values).execute(db)),
            Statement::Update {
                table,
                assignments,
                where_clause,
            } => self.with_auto_tx(|db| Update::new(table, assignments, where_clause).execute(db)),
            Statement::Delete {
                table,
                where_clause,
            } => self.with_auto_tx(|db| Delete::new(table, where_clause).execute(db)),
            Statement::Select(select) => {
                let plan = Planner::plan_select(&self.db.state, &select);
                execute_select_plan(&self.db.state, plan)
            }
            Statement::Meta(_) => Err(MiniDbError::Sql("meta-command handled by CLI".into())),
        }
    }

    fn with_auto_tx<F>(&mut self, f: F) -> Result<QueryResult>
    where
        F: FnOnce(&mut Database) -> Result<QueryResult>,
    {
        let auto = !self.db.in_transaction();
        if auto {
            self.db.begin()?;
        }
        let before = self.db.state.clone();
        let result = f(&mut self.db);
        match result {
            Ok(value) => {
                if auto {
                    self.db.commit()?;
                }
                Ok(value)
            }
            Err(err) => {
                self.db.state = before;
                if auto && self.db.in_transaction() {
                    let _ = self.db.rollback();
                }
                Err(err)
            }
        }
    }
}

fn execute_select_plan(state: &DatabaseState, plan: PhysicalPlan) -> Result<QueryResult> {
    let mut op = build_operator(state, plan)?;
    op.open()?;
    let columns = op.schema().into_iter().map(|c| c.name).collect::<Vec<_>>();
    let mut rows = Vec::new();
    while let Some(row) = op.next()? {
        rows.push(row.values);
    }
    op.close()?;
    Ok(QueryResult {
        columns,
        rows,
        affected: None,
    })
}

fn build_operator(state: &DatabaseState, plan: PhysicalPlan) -> Result<Box<dyn PhysicalOperator>> {
    match plan {
        PhysicalPlan::SeqScan { table } => Ok(Box::new(SeqScan::new(state, table)?)),
        PhysicalPlan::IndexScan {
            table,
            index,
            column,
            op,
            value,
        } => Ok(Box::new(IndexScan::new(
            state, table, index, column, op, value,
        )?)),
        PhysicalPlan::Filter { input, predicate } => Ok(Box::new(Filter::new(
            build_operator(state, *input)?,
            predicate,
        ))),
        PhysicalPlan::Project { input, select } => Ok(Box::new(Project::new(
            build_operator(state, *input)?,
            select,
        ))),
        PhysicalPlan::NestedLoopJoin { left, right, on } => Ok(Box::new(NestedLoopJoin::new(
            build_operator(state, *left)?,
            build_operator(state, *right)?,
            on,
        ))),
        PhysicalPlan::Sort { input, select } => {
            Ok(Box::new(Sort::new(build_operator(state, *input)?, select)))
        }
        PhysicalPlan::Limit {
            input,
            limit,
            offset,
        } => Ok(Box::new(Limit::new(
            build_operator(state, *input)?,
            limit,
            offset,
        ))),
    }
}

pub struct SeqScan {
    table: String,
    rows: Vec<StoredRow>,
    columns: Vec<ColumnRef>,
    pos: usize,
}

impl SeqScan {
    pub fn new(state: &DatabaseState, table: String) -> Result<Self> {
        let schema = state.catalog.table(&table)?;
        let rows = state.table_rows(&table)?.clone();
        let columns = schema
            .columns
            .iter()
            .map(|c| ColumnRef {
                table: schema.name.clone(),
                name: c.name.clone(),
            })
            .collect();
        Ok(Self {
            table,
            rows,
            columns,
            pos: 0,
        })
    }
}

impl PhysicalOperator for SeqScan {
    fn open(&mut self) -> Result<()> {
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<ExecRow>> {
        let row = match self.rows.get(self.pos) {
            Some(row) => row.clone(),
            None => return Ok(None),
        };
        self.pos += 1;
        let mut row_ids = BTreeMap::new();
        row_ids.insert(normalize(&self.table), row.id);
        Ok(Some(ExecRow {
            values: row.values,
            columns: self.columns.clone(),
            row_ids,
        }))
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<ColumnRef> {
        self.columns.clone()
    }
}

pub struct IndexScan {
    inner: SeqScan,
}

impl IndexScan {
    pub fn new(
        state: &DatabaseState,
        table: String,
        index: String,
        _column: String,
        op: BinaryOp,
        value: Value,
    ) -> Result<Self> {
        let tree = state
            .indexes
            .get(&normalize(&index))
            .ok_or_else(|| MiniDbError::Sql(format!("unknown index {index}")))?;
        let ids = match op {
            BinaryOp::Eq => tree.equals(&value),
            BinaryOp::Lt => tree.range("<", &value),
            BinaryOp::Le => tree.range("<=", &value),
            BinaryOp::Gt => tree.range(">", &value),
            BinaryOp::Ge => tree.range(">=", &value),
            _ => Vec::new(),
        }
        .into_iter()
        .collect::<BTreeSet<_>>();
        let schema = state.catalog.table(&table)?;
        let rows = state
            .table_rows(&table)?
            .iter()
            .filter(|r| ids.contains(&r.id))
            .cloned()
            .collect::<Vec<_>>();
        let columns = schema
            .columns
            .iter()
            .map(|c| ColumnRef {
                table: schema.name.clone(),
                name: c.name.clone(),
            })
            .collect();
        Ok(Self {
            inner: SeqScan {
                table,
                rows,
                columns,
                pos: 0,
            },
        })
    }
}

impl PhysicalOperator for IndexScan {
    fn open(&mut self) -> Result<()> {
        self.inner.open()
    }
    fn next(&mut self) -> Result<Option<ExecRow>> {
        self.inner.next()
    }
    fn close(&mut self) -> Result<()> {
        self.inner.close()
    }
    fn schema(&self) -> Vec<ColumnRef> {
        self.inner.schema()
    }
}

pub struct Filter {
    input: Box<dyn PhysicalOperator>,
    predicate: Expr,
}

impl Filter {
    pub fn new(input: Box<dyn PhysicalOperator>, predicate: Expr) -> Self {
        Self { input, predicate }
    }
}

impl PhysicalOperator for Filter {
    fn open(&mut self) -> Result<()> {
        self.input.open()
    }
    fn next(&mut self) -> Result<Option<ExecRow>> {
        while let Some(row) = self.input.next()? {
            if eval_expr(&self.predicate, &row)?.as_truth().passes_where() {
                return Ok(Some(row));
            }
        }
        Ok(None)
    }
    fn close(&mut self) -> Result<()> {
        self.input.close()
    }
    fn schema(&self) -> Vec<ColumnRef> {
        self.input.schema()
    }
}

pub struct Project {
    input: Box<dyn PhysicalOperator>,
    select: SelectStmt,
    columns: Vec<ColumnRef>,
}

impl Project {
    pub fn new(input: Box<dyn PhysicalOperator>, select: SelectStmt) -> Self {
        let columns = project_schema(&input.schema(), &select);
        Self {
            input,
            select,
            columns,
        }
    }
}

impl PhysicalOperator for Project {
    fn open(&mut self) -> Result<()> {
        self.input.open()
    }
    fn next(&mut self) -> Result<Option<ExecRow>> {
        let row = match self.input.next()? {
            Some(row) => row,
            None => return Ok(None),
        };
        let mut values = Vec::new();
        for item in &self.select.projection {
            match item {
                SelectItem::Wildcard => values.extend(row.values.clone()),
                SelectItem::Expr { expr, .. } => values.push(eval_expr(expr, &row)?),
            }
        }
        Ok(Some(ExecRow {
            values,
            columns: self.columns.clone(),
            row_ids: row.row_ids,
        }))
    }
    fn close(&mut self) -> Result<()> {
        self.input.close()
    }
    fn schema(&self) -> Vec<ColumnRef> {
        self.columns.clone()
    }
}

pub struct NestedLoopJoin {
    left: Box<dyn PhysicalOperator>,
    right: Box<dyn PhysicalOperator>,
    on: Expr,
    rows: Vec<ExecRow>,
    pos: usize,
    columns: Vec<ColumnRef>,
}

impl NestedLoopJoin {
    pub fn new(
        left: Box<dyn PhysicalOperator>,
        right: Box<dyn PhysicalOperator>,
        on: Expr,
    ) -> Self {
        let mut columns = left.schema();
        columns.extend(right.schema());
        Self {
            left,
            right,
            on,
            rows: Vec::new(),
            pos: 0,
            columns,
        }
    }
}

impl PhysicalOperator for NestedLoopJoin {
    fn open(&mut self) -> Result<()> {
        self.rows.clear();
        self.pos = 0;
        self.left.open()?;
        let mut left_rows = Vec::new();
        while let Some(row) = self.left.next()? {
            left_rows.push(row);
        }
        self.left.close()?;
        self.right.open()?;
        let mut right_rows = Vec::new();
        while let Some(row) = self.right.next()? {
            right_rows.push(row);
        }
        self.right.close()?;
        for l in &left_rows {
            for r in &right_rows {
                let mut row = l.clone();
                row.values.extend(r.values.clone());
                row.columns.extend(r.columns.clone());
                row.row_ids.extend(r.row_ids.clone());
                if eval_expr(&self.on, &row)?.as_truth().passes_where() {
                    self.rows.push(row);
                }
            }
        }
        Ok(())
    }
    fn next(&mut self) -> Result<Option<ExecRow>> {
        let row = self.rows.get(self.pos).cloned();
        if row.is_some() {
            self.pos += 1;
        }
        Ok(row)
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> Vec<ColumnRef> {
        self.columns.clone()
    }
}

pub struct Sort {
    input: Box<dyn PhysicalOperator>,
    select: SelectStmt,
    rows: Vec<ExecRow>,
    pos: usize,
}

impl Sort {
    pub fn new(input: Box<dyn PhysicalOperator>, select: SelectStmt) -> Self {
        Self {
            input,
            select,
            rows: Vec::new(),
            pos: 0,
        }
    }
}

impl PhysicalOperator for Sort {
    fn open(&mut self) -> Result<()> {
        self.rows.clear();
        self.pos = 0;
        self.input.open()?;
        while let Some(row) = self.input.next()? {
            self.rows.push(row);
        }
        let order = self.select.order_by.clone();
        self.rows.sort_by(|a, b| {
            for item in &order {
                let av = eval_expr(&item.expr, a).unwrap_or(Value::Null);
                let bv = eval_expr(&item.expr, b).unwrap_or(Value::Null);
                let ord = av.sql_cmp(&bv).unwrap_or(Ordering::Equal);
                if ord != Ordering::Equal {
                    return if item.asc { ord } else { ord.reverse() };
                }
            }
            Ordering::Equal
        });
        Ok(())
    }
    fn next(&mut self) -> Result<Option<ExecRow>> {
        let row = self.rows.get(self.pos).cloned();
        if row.is_some() {
            self.pos += 1;
        }
        Ok(row)
    }
    fn close(&mut self) -> Result<()> {
        self.input.close()
    }
    fn schema(&self) -> Vec<ColumnRef> {
        self.input.schema()
    }
}

pub struct Limit {
    input: Box<dyn PhysicalOperator>,
    limit: Option<usize>,
    offset: usize,
    emitted: usize,
    skipped: bool,
}

impl Limit {
    pub fn new(
        input: Box<dyn PhysicalOperator>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Self {
        Self {
            input,
            limit,
            offset: offset.unwrap_or(0),
            emitted: 0,
            skipped: false,
        }
    }
}

impl PhysicalOperator for Limit {
    fn open(&mut self) -> Result<()> {
        self.emitted = 0;
        self.skipped = false;
        self.input.open()
    }
    fn next(&mut self) -> Result<Option<ExecRow>> {
        if !self.skipped {
            for _ in 0..self.offset {
                if self.input.next()?.is_none() {
                    return Ok(None);
                }
            }
            self.skipped = true;
        }
        if self.limit.is_some_and(|limit| self.emitted >= limit) {
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
    fn schema(&self) -> Vec<ColumnRef> {
        self.input.schema()
    }
}

pub struct Insert {
    table: String,
    columns: Option<Vec<String>>,
    values: Vec<Expr>,
}

impl Insert {
    pub fn new(table: String, columns: Option<Vec<String>>, values: Vec<Expr>) -> Self {
        Self {
            table,
            columns,
            values,
        }
    }

    pub fn execute(self, db: &mut Database) -> Result<QueryResult> {
        let schema = db.state.catalog.table(&self.table)?.clone();
        let row = build_insert_row(&schema, self.columns, self.values)?;
        validate_candidate_rows(
            &schema,
            db.state.table_rows(&self.table)?,
            &[row.clone()],
            None,
        )?;
        let row_id = next_row_id(&mut db.state);
        db.append_and_apply(WalOp::Insert {
            table: self.table,
            row_id,
            values: row,
        })?;
        Ok(QueryResult::affected(1))
    }
}

pub struct Update {
    table: String,
    assignments: Vec<(String, Expr)>,
    where_clause: Option<Expr>,
}

impl Update {
    pub fn new(
        table: String,
        assignments: Vec<(String, Expr)>,
        where_clause: Option<Expr>,
    ) -> Self {
        Self {
            table,
            assignments,
            where_clause,
        }
    }

    pub fn execute(self, db: &mut Database) -> Result<QueryResult> {
        let schema = db.state.catalog.table(&self.table)?.clone();
        let rows = db.state.table_rows(&self.table)?.clone();
        let columns = scan_columns(&schema);
        let assignment_indexes = self
            .assignments
            .iter()
            .map(|(name, expr)| Ok((schema.column_index(name)?, expr.clone())))
            .collect::<Result<Vec<_>>>()?;
        let mut updates = Vec::new();
        let mut final_rows = Vec::new();
        for row in rows {
            let exec_row = single_table_exec_row(&schema, &row, columns.clone());
            let matches = match &self.where_clause {
                Some(expr) => eval_expr(expr, &exec_row)?.as_truth().passes_where(),
                None => true,
            };
            if matches {
                let mut values = row.values.clone();
                for (idx, expr) in &assignment_indexes {
                    values[*idx] =
                        eval_expr(expr, &exec_row)?.cast_to(schema.columns[*idx].data_type)?;
                }
                updates.push((row.id, values.clone()));
                final_rows.push(StoredRow { id: row.id, values });
            } else {
                final_rows.push(row);
            }
        }
        validate_all_rows(&schema, &final_rows)?;
        for (row_id, values) in &updates {
            db.append_and_apply(WalOp::Update {
                table: self.table.clone(),
                row_id: *row_id,
                values: values.clone(),
            })?;
        }
        Ok(QueryResult::affected(updates.len()))
    }
}

pub struct Delete {
    table: String,
    where_clause: Option<Expr>,
}

impl Delete {
    pub fn new(table: String, where_clause: Option<Expr>) -> Self {
        Self {
            table,
            where_clause,
        }
    }

    pub fn execute(self, db: &mut Database) -> Result<QueryResult> {
        let schema = db.state.catalog.table(&self.table)?.clone();
        let rows = db.state.table_rows(&self.table)?.clone();
        let columns = scan_columns(&schema);
        let mut ids = Vec::new();
        for row in rows {
            let exec_row = single_table_exec_row(&schema, &row, columns.clone());
            let matches = match &self.where_clause {
                Some(expr) => eval_expr(expr, &exec_row)?.as_truth().passes_where(),
                None => true,
            };
            if matches {
                ids.push(row.id);
            }
        }
        for row_id in &ids {
            db.append_and_apply(WalOp::Delete {
                table: self.table.clone(),
                row_id: *row_id,
            })?;
        }
        Ok(QueryResult::affected(ids.len()))
    }
}

impl PhysicalOperator for Insert {
    fn open(&mut self) -> Result<()> {
        Ok(())
    }
    fn next(&mut self) -> Result<Option<ExecRow>> {
        Ok(None)
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> Vec<ColumnRef> {
        Vec::new()
    }
}

impl PhysicalOperator for Update {
    fn open(&mut self) -> Result<()> {
        Ok(())
    }
    fn next(&mut self) -> Result<Option<ExecRow>> {
        Ok(None)
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> Vec<ColumnRef> {
        Vec::new()
    }
}

impl PhysicalOperator for Delete {
    fn open(&mut self) -> Result<()> {
        Ok(())
    }
    fn next(&mut self) -> Result<Option<ExecRow>> {
        Ok(None)
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> Vec<ColumnRef> {
        Vec::new()
    }
}

pub fn eval_expr(expr: &Expr, row: &ExecRow) -> Result<Value> {
    match expr {
        Expr::Literal(value) => Ok(value.clone()),
        Expr::Identifier(parts) => resolve_identifier(parts, row),
        Expr::Unary { op, expr } => {
            let value = eval_expr(expr, row)?;
            match op {
                UnaryOp::Not => Ok(truth_value(value.as_truth().negate())),
                UnaryOp::Neg => match value {
                    Value::Null => Ok(Value::Null),
                    Value::Int(v) => Ok(Value::Int(-v)),
                    Value::Real(v) => Ok(Value::Real(-v)),
                    _ => Err(MiniDbError::Type(
                        "unary minus expects numeric value".into(),
                    )),
                },
                UnaryOp::Pos => Ok(value),
            }
        }
        Expr::Binary { left, op, right } => {
            let l = eval_expr(left, row)?;
            let r = eval_expr(right, row)?;
            eval_binary(l, *op, r)
        }
        Expr::Function { name, args } => eval_function(name, args, row),
        Expr::IsNull { expr, negated } => {
            let is_null = eval_expr(expr, row)?.is_null();
            Ok(Value::Bool(if *negated { !is_null } else { is_null }))
        }
    }
}

fn eval_binary(left: Value, op: BinaryOp, right: Value) -> Result<Value> {
    match op {
        BinaryOp::And => Ok(truth_value(left.as_truth().and(right.as_truth()))),
        BinaryOp::Or => Ok(truth_value(left.as_truth().or(right.as_truth()))),
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            let truth = match left.sql_cmp(&right) {
                Some(ord) => match op {
                    BinaryOp::Eq => ord == Ordering::Equal,
                    BinaryOp::Ne => ord != Ordering::Equal,
                    BinaryOp::Lt => ord == Ordering::Less,
                    BinaryOp::Le => ord != Ordering::Greater,
                    BinaryOp::Gt => ord == Ordering::Greater,
                    BinaryOp::Ge => ord != Ordering::Less,
                    _ => false,
                },
                None => return Ok(Value::Null),
            };
            Ok(Value::Bool(truth))
        }
        BinaryOp::Concat => {
            if left.is_null() || right.is_null() {
                Ok(Value::Null)
            } else {
                Ok(Value::Text(format!("{left}{right}")))
            }
        }
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
            let (l, r) = coerce_numeric(&left, &right)?;
            match (l, r) {
                (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                (Value::Int(a), Value::Int(b)) => match op {
                    BinaryOp::Add => Ok(Value::Int(a + b)),
                    BinaryOp::Sub => Ok(Value::Int(a - b)),
                    BinaryOp::Mul => Ok(Value::Int(a * b)),
                    BinaryOp::Div if b != 0 => Ok(Value::Int(a / b)),
                    BinaryOp::Div => Err(MiniDbError::Sql("division by zero".into())),
                    _ => Err(MiniDbError::Type("expected arithmetic operator".into())),
                },
                (Value::Real(a), Value::Real(b)) => match op {
                    BinaryOp::Add => Ok(Value::Real(a + b)),
                    BinaryOp::Sub => Ok(Value::Real(a - b)),
                    BinaryOp::Mul => Ok(Value::Real(a * b)),
                    BinaryOp::Div if b != 0.0 => Ok(Value::Real(a / b)),
                    BinaryOp::Div => Err(MiniDbError::Sql("division by zero".into())),
                    _ => Err(MiniDbError::Type("expected arithmetic operator".into())),
                },
                _ => Err(MiniDbError::Type("expected numeric operands".into())),
            }
        }
    }
}

fn eval_function(name: &str, args: &[Expr], row: &ExecRow) -> Result<Value> {
    let values = args
        .iter()
        .map(|arg| eval_expr(arg, row))
        .collect::<Result<Vec<_>>>()?;
    match name {
        "LENGTH" if values.len() == 1 => match &values[0] {
            Value::Null => Ok(Value::Null),
            Value::Text(s) => Ok(Value::Int(s.chars().count() as i64)),
            _ => Err(MiniDbError::Type("LENGTH expects TEXT".into())),
        },
        "LOWER" if values.len() == 1 => string_fn(&values[0], |s| s.to_ascii_lowercase()),
        "UPPER" if values.len() == 1 => string_fn(&values[0], |s| s.to_ascii_uppercase()),
        "SUBSTR" if values.len() == 2 || values.len() == 3 => substr_fn(&values),
        "ABS" if values.len() == 1 => match &values[0] {
            Value::Null => Ok(Value::Null),
            Value::Int(v) => Ok(Value::Int(v.abs())),
            Value::Real(v) => Ok(Value::Real(v.abs())),
            _ => Err(MiniDbError::Type("ABS expects numeric value".into())),
        },
        "COALESCE" => Ok(values
            .into_iter()
            .find(|v| !v.is_null())
            .unwrap_or(Value::Null)),
        _ => Err(MiniDbError::Sql(format!("unknown function {name}"))),
    }
}

fn string_fn(value: &Value, f: impl Fn(&str) -> String) -> Result<Value> {
    match value {
        Value::Null => Ok(Value::Null),
        Value::Text(s) => Ok(Value::Text(f(s))),
        _ => Err(MiniDbError::Type("function expects TEXT".into())),
    }
}

fn substr_fn(values: &[Value]) -> Result<Value> {
    let text = match &values[0] {
        Value::Null => return Ok(Value::Null),
        Value::Text(s) => s,
        _ => {
            return Err(MiniDbError::Type(
                "SUBSTR expects TEXT first argument".into(),
            ))
        }
    };
    let start = match values.get(1) {
        Some(Value::Int(v)) => (*v).max(1) as usize - 1,
        Some(Value::Null) => return Ok(Value::Null),
        _ => return Err(MiniDbError::Type("SUBSTR start expects INT".into())),
    };
    let len = match values.get(2) {
        Some(Value::Int(v)) => Some((*v).max(0) as usize),
        Some(Value::Null) => return Ok(Value::Null),
        None => None,
        _ => return Err(MiniDbError::Type("SUBSTR length expects INT".into())),
    };
    let chars = text.chars().skip(start);
    let out = match len {
        Some(n) => chars.take(n).collect(),
        None => chars.collect(),
    };
    Ok(Value::Text(out))
}

fn truth_value(truth: Truth) -> Value {
    match truth {
        Truth::True => Value::Bool(true),
        Truth::False => Value::Bool(false),
        Truth::Unknown => Value::Null,
    }
}

fn resolve_identifier(parts: &[String], row: &ExecRow) -> Result<Value> {
    let mut matches = Vec::new();
    for (idx, col) in row.columns.iter().enumerate() {
        let is_match = match parts {
            [name] => normalize(&col.name) == normalize(name),
            [table, name] => {
                normalize(&col.table) == normalize(table) && normalize(&col.name) == normalize(name)
            }
            _ => false,
        };
        if is_match {
            if let Some(value) = row.values.get(idx) {
                matches.push(value.clone());
            }
        }
    }
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(MiniDbError::Sql(format!(
            "unknown column {}",
            parts.join(".")
        ))),
        _ => Err(MiniDbError::Sql(format!(
            "ambiguous column {}",
            parts.join(".")
        ))),
    }
}

fn build_insert_row(
    schema: &TableSchema,
    columns: Option<Vec<String>>,
    values: Vec<Expr>,
) -> Result<Vec<Value>> {
    let empty = ExecRow {
        values: Vec::new(),
        columns: Vec::new(),
        row_ids: BTreeMap::new(),
    };
    let mut row = vec![Value::Null; schema.columns.len()];
    if let Some(cols) = columns {
        if cols.len() != values.len() {
            return Err(MiniDbError::Sql(
                "INSERT column/value count mismatch".into(),
            ));
        }
        for (name, expr) in cols.into_iter().zip(values.into_iter()) {
            let idx = schema.column_index(&name)?;
            row[idx] = eval_expr(&expr, &empty)?.cast_to(schema.columns[idx].data_type)?;
        }
        fill_defaults(schema, &mut row, &empty)?;
    } else {
        if values.len() != schema.columns.len() {
            return Err(MiniDbError::Sql("INSERT value count mismatch".into()));
        }
        for (idx, expr) in values.into_iter().enumerate() {
            row[idx] = eval_expr(&expr, &empty)?.cast_to(schema.columns[idx].data_type)?;
        }
    }
    for (idx, col) in schema.columns.iter().enumerate() {
        if col.not_null && row[idx].is_null() {
            return Err(MiniDbError::Sql(format!(
                "column {} may not be NULL",
                col.name
            )));
        }
    }
    Ok(row)
}

fn fill_defaults(schema: &TableSchema, row: &mut [Value], empty: &ExecRow) -> Result<()> {
    for (idx, col) in schema.columns.iter().enumerate() {
        if row[idx].is_null() {
            if let Some(expr) = &col.default {
                row[idx] = eval_expr(expr, empty)?.cast_to(col.data_type)?;
            }
        }
    }
    Ok(())
}

fn validate_candidate_rows(
    schema: &TableSchema,
    existing: &[StoredRow],
    candidates: &[Vec<Value>],
    skip_row: Option<u64>,
) -> Result<()> {
    let mut all = existing
        .iter()
        .filter(|r| Some(r.id) != skip_row)
        .cloned()
        .collect::<Vec<_>>();
    for (offset, values) in candidates.iter().enumerate() {
        all.push(StoredRow {
            id: u64::MAX - offset as u64,
            values: values.clone(),
        });
    }
    validate_all_rows(schema, &all)
}

fn validate_all_rows(schema: &TableSchema, rows: &[StoredRow]) -> Result<()> {
    for row in rows {
        for (idx, col) in schema.columns.iter().enumerate() {
            let value = row.values.get(idx).unwrap_or(&Value::Null);
            if col.not_null && value.is_null() {
                return Err(MiniDbError::Sql(format!(
                    "column {} may not be NULL",
                    col.name
                )));
            }
        }
        let exec_row = single_table_exec_row(schema, row, scan_columns(schema));
        for check in &schema.checks {
            if !eval_expr(check, &exec_row)?.as_truth().passes_where() {
                return Err(MiniDbError::Sql("CHECK constraint failed".into()));
            }
        }
    }
    for (idx, col) in schema.columns.iter().enumerate() {
        if col.unique || col.primary_key {
            let mut seen = BTreeSet::new();
            for row in rows {
                let value = row.values.get(idx).unwrap_or(&Value::Null);
                if value.is_null() {
                    continue;
                }
                if !seen.insert(value.key()) {
                    return Err(MiniDbError::Sql(format!(
                        "UNIQUE constraint failed: {}.{}",
                        schema.name, col.name
                    )));
                }
            }
        }
    }
    Ok(())
}

fn single_table_exec_row(
    schema: &TableSchema,
    row: &StoredRow,
    columns: Vec<ColumnRef>,
) -> ExecRow {
    let mut row_ids = BTreeMap::new();
    row_ids.insert(normalize(&schema.name), row.id);
    ExecRow {
        values: row.values.clone(),
        columns,
        row_ids,
    }
}

fn scan_columns(schema: &TableSchema) -> Vec<ColumnRef> {
    schema
        .columns
        .iter()
        .map(|c| ColumnRef {
            table: schema.name.clone(),
            name: c.name.clone(),
        })
        .collect()
}

fn project_schema(input: &[ColumnRef], select: &SelectStmt) -> Vec<ColumnRef> {
    let mut columns = Vec::new();
    for item in &select.projection {
        match item {
            SelectItem::Wildcard => columns.extend(input.iter().cloned()),
            SelectItem::Expr { expr, alias } => columns.push(ColumnRef {
                table: String::new(),
                name: alias.clone().unwrap_or_else(|| expr_label(expr)),
            }),
        }
    }
    columns
}

fn expr_label(expr: &Expr) -> String {
    match expr {
        Expr::Identifier(parts) => parts.join("."),
        Expr::Function { name, .. } => name.to_ascii_lowercase(),
        _ => "expr".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_sql;
    use tempfile::tempdir;

    #[test]
    fn evaluates_basic_expression() {
        let row = ExecRow {
            values: vec![Value::Int(2)],
            columns: vec![ColumnRef {
                table: "t".into(),
                name: "x".into(),
            }],
            row_ids: BTreeMap::new(),
        };
        let stmt = parse_sql("SELECT x + 3 FROM t;").unwrap().remove(0);
        let expr = match stmt {
            Statement::Select(s) => match &s.projection[0] {
                SelectItem::Expr { expr, .. } => expr.clone(),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };
        assert_eq!(eval_expr(&expr, &row).unwrap(), Value::Int(5));
    }

    #[test]
    fn engine_inserts_and_selects() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("db");
        let mut engine = Engine::open(&path).unwrap();
        for stmt in parse_sql(
            "CREATE TABLE t (id INT PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'a');",
        )
        .unwrap()
        {
            engine.execute(stmt).unwrap();
        }
        let result = engine
            .execute(
                parse_sql("SELECT name FROM t WHERE id = 1;")
                    .unwrap()
                    .remove(0),
            )
            .unwrap();
        assert_eq!(result.rows, vec![vec![Value::Text("a".into())]]);
    }
}
