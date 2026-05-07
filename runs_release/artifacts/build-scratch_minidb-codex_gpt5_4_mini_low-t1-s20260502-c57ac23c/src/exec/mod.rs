use std::cmp::Ordering;

use crate::catalog::{IndexMetadata, TableMetadata};
use crate::error::MiniDbError;
use crate::parser::ast::*;
use crate::storage::{DbSnapshot, Storage};
use crate::types::{compare_values, Column, Row, TruthValue, Value};

pub trait PhysicalOperator {
    fn open(&mut self) -> Result<(), MiniDbError>;
    fn next(&mut self) -> Result<Option<Row>, MiniDbError>;
    fn close(&mut self) -> Result<(), MiniDbError>;
    fn schema(&self) -> &[Column];
}

pub struct SeqScan<'a> {
    pub table: &'a str,
    pub snapshot: &'a DbSnapshot,
    schema: Vec<Column>,
    rows: Vec<Row>,
    idx: usize,
}
impl<'a> SeqScan<'a> {
    pub fn new(table: &'a str, snapshot: &'a DbSnapshot) -> Self {
        let schema = snapshot
            .catalog
            .tables
            .get(table)
            .map(|t| t.columns.clone())
            .unwrap_or_default();
        let rows = snapshot.rows.get(table).cloned().unwrap_or_default();
        Self {
            table,
            snapshot,
            schema,
            rows,
            idx: 0,
        }
    }
}
impl<'a> PhysicalOperator for SeqScan<'a> {
    fn open(&mut self) -> Result<(), MiniDbError> {
        Ok(())
    }
    fn next(&mut self) -> Result<Option<Row>, MiniDbError> {
        let r = self.rows.get(self.idx).cloned();
        self.idx += usize::from(r.is_some());
        Ok(r)
    }
    fn close(&mut self) -> Result<(), MiniDbError> {
        Ok(())
    }
    fn schema(&self) -> &[Column] {
        &self.schema
    }
}

pub struct IndexScan {
    pub table: String,
    pub column: String,
    pub value: Value,
    schema: Vec<Column>,
    rows: Vec<Row>,
    idx: usize,
}
impl IndexScan {
    pub fn new(table: String, column: String, value: Value, snapshot: &DbSnapshot) -> Self {
        let schema = snapshot
            .catalog
            .tables
            .get(&table)
            .map(|t| t.columns.clone())
            .unwrap_or_default();
        let rows = snapshot
            .rows
            .get(&table)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| {
                schema
                    .iter()
                    .position(|c| c.name == column)
                    .and_then(|i| r.get(i))
                    .map(|v| v == &value)
                    .unwrap_or(false)
            })
            .collect();
        Self {
            table,
            column,
            value,
            schema,
            rows,
            idx: 0,
        }
    }
}
impl PhysicalOperator for IndexScan {
    fn open(&mut self) -> Result<(), MiniDbError> {
        Ok(())
    }
    fn next(&mut self) -> Result<Option<Row>, MiniDbError> {
        let r = self.rows.get(self.idx).cloned();
        self.idx += usize::from(r.is_some());
        Ok(r)
    }
    fn close(&mut self) -> Result<(), MiniDbError> {
        Ok(())
    }
    fn schema(&self) -> &[Column] {
        &self.schema
    }
}

pub struct Filter<O: PhysicalOperator> {
    input: O,
    pred: Box<dyn Fn(&Row) -> Result<bool, MiniDbError>>,
    schema: Vec<Column>,
}
impl<O: PhysicalOperator> Filter<O> {
    pub fn new(input: O, pred: Box<dyn Fn(&Row) -> Result<bool, MiniDbError>>) -> Self {
        let schema = input.schema().to_vec();
        Self {
            input,
            pred,
            schema,
        }
    }
}
impl<O: PhysicalOperator> PhysicalOperator for Filter<O> {
    fn open(&mut self) -> Result<(), MiniDbError> {
        self.input.open()
    }
    fn next(&mut self) -> Result<Option<Row>, MiniDbError> {
        while let Some(r) = self.input.next()? {
            if (self.pred)(&r)? {
                return Ok(Some(r));
            }
        }
        Ok(None)
    }
    fn close(&mut self) -> Result<(), MiniDbError> {
        self.input.close()
    }
    fn schema(&self) -> &[Column] {
        &self.schema
    }
}

pub struct Project<O: PhysicalOperator> {
    input: O,
    exprs: Vec<Box<dyn Fn(&Row) -> Result<Value, MiniDbError>>>,
    schema: Vec<Column>,
}
impl<O: PhysicalOperator> Project<O> {
    pub fn new(
        input: O,
        exprs: Vec<Box<dyn Fn(&Row) -> Result<Value, MiniDbError>>>,
        schema: Vec<Column>,
    ) -> Self {
        Self {
            input,
            exprs,
            schema,
        }
    }
}
impl<O: PhysicalOperator> PhysicalOperator for Project<O> {
    fn open(&mut self) -> Result<(), MiniDbError> {
        self.input.open()
    }
    fn next(&mut self) -> Result<Option<Row>, MiniDbError> {
        match self.input.next()? {
            Some(r) => Ok(Some(Row(self
                .exprs
                .iter()
                .map(|e| e(&r))
                .collect::<Result<Vec<_>, _>>()?))),
            None => Ok(None),
        }
    }
    fn close(&mut self) -> Result<(), MiniDbError> {
        self.input.close()
    }
    fn schema(&self) -> &[Column] {
        &self.schema
    }
}

pub struct NestedLoopJoin<L: PhysicalOperator, R: PhysicalOperator> {
    rows: Vec<Row>,
    idx: usize,
    schema: Vec<Column>,
    _marker: std::marker::PhantomData<(L, R)>,
}
impl<L: PhysicalOperator, R: PhysicalOperator> NestedLoopJoin<L, R> {
    pub fn new(mut left: L, mut right: R) -> Result<Self, MiniDbError> {
        let mut rows = Vec::new();
        left.open()?;
        right.open()?;
        let mut lrows = Vec::new();
        while let Some(r) = left.next()? {
            lrows.push(r);
        }
        let mut rrows = Vec::new();
        while let Some(r) = right.next()? {
            rrows.push(r);
        }
        for l in &lrows {
            for r in &rrows {
                let mut v = l.0.clone();
                v.extend(r.0.clone());
                rows.push(Row(v));
            }
        }
        let mut schema = left.schema().to_vec();
        schema.extend_from_slice(right.schema());
        let _ = (left, right);
        Ok(Self {
            rows,
            idx: 0,
            schema,
            _marker: std::marker::PhantomData,
        })
    }
}
impl<L: PhysicalOperator, R: PhysicalOperator> PhysicalOperator for NestedLoopJoin<L, R> {
    fn open(&mut self) -> Result<(), MiniDbError> {
        Ok(())
    }
    fn next(&mut self) -> Result<Option<Row>, MiniDbError> {
        let r = self.rows.get(self.idx).cloned();
        self.idx += usize::from(r.is_some());
        Ok(r)
    }
    fn close(&mut self) -> Result<(), MiniDbError> {
        Ok(())
    }
    fn schema(&self) -> &[Column] {
        &self.schema
    }
}

pub struct Sort<O: PhysicalOperator> {
    input: O,
    rows: Vec<Row>,
    idx: usize,
    schema: Vec<Column>,
    cmp: Box<dyn Fn(&Row, &Row) -> Ordering>,
}
impl<O: PhysicalOperator> Sort<O> {
    pub fn new(input: O, cmp: Box<dyn Fn(&Row, &Row) -> Ordering>) -> Self {
        let schema = input.schema().to_vec();
        Self {
            input,
            rows: vec![],
            idx: 0,
            schema,
            cmp,
        }
    }
}
impl<O: PhysicalOperator> PhysicalOperator for Sort<O> {
    fn open(&mut self) -> Result<(), MiniDbError> {
        self.input.open()?;
        while let Some(r) = self.input.next()? {
            self.rows.push(r);
        }
        self.rows.sort_by(|a, b| (self.cmp)(a, b));
        Ok(())
    }
    fn next(&mut self) -> Result<Option<Row>, MiniDbError> {
        let r = self.rows.get(self.idx).cloned();
        self.idx += usize::from(r.is_some());
        Ok(r)
    }
    fn close(&mut self) -> Result<(), MiniDbError> {
        self.input.close()
    }
    fn schema(&self) -> &[Column] {
        &self.schema
    }
}

pub struct Limit<O: PhysicalOperator> {
    input: O,
    limit: usize,
    seen: usize,
    schema: Vec<Column>,
}
impl<O: PhysicalOperator> Limit<O> {
    pub fn new(input: O, limit: usize) -> Self {
        let schema = input.schema().to_vec();
        Self {
            input,
            limit,
            seen: 0,
            schema,
        }
    }
}
impl<O: PhysicalOperator> PhysicalOperator for Limit<O> {
    fn open(&mut self) -> Result<(), MiniDbError> {
        self.input.open()
    }
    fn next(&mut self) -> Result<Option<Row>, MiniDbError> {
        if self.seen >= self.limit {
            return Ok(None);
        }
        let r = self.input.next()?;
        self.seen += usize::from(r.is_some());
        Ok(r)
    }
    fn close(&mut self) -> Result<(), MiniDbError> {
        self.input.close()
    }
    fn schema(&self) -> &[Column] {
        &self.schema
    }
}

pub struct Insert;
pub struct Update;
pub struct Delete;

pub struct Executor {
    pub storage: Storage,
}

impl Executor {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }
    pub fn execute(&mut self, stmt: Statement) -> Result<ExecResult, MiniDbError> {
        match stmt {
            Statement::CreateTable { name, elements } => {
                let mut cols = Vec::new();
                let mut checks = Vec::new();
                for el in elements {
                    match el {
                        TableElement::Column(c) => cols.push(Column {
                            table: Some(name.clone()),
                            name: c.name,
                            col_type: c.col_type,
                            primary_key: c.primary_key,
                            not_null: c.not_null,
                            unique: c.unique,
                            default: c.default,
                        }),
                        TableElement::Check(e) => checks.push(e),
                    }
                }
                self.storage.snapshot_mut().catalog.tables.insert(
                    name.clone(),
                    TableMetadata {
                        name,
                        columns: cols,
                        checks,
                    },
                );
                self.persist()?;
                Ok(ExecResult::Message("table created".into(), 0))
            }
            Statement::DropTable { name } => {
                self.storage.snapshot_mut().catalog.tables.remove(&name);
                self.storage.snapshot_mut().rows.remove(&name);
                self.persist()?;
                Ok(ExecResult::Message("table dropped".into(), 0))
            }
            Statement::CreateIndex {
                name,
                table,
                column,
            } => {
                self.storage.snapshot_mut().catalog.indexes.insert(
                    name.clone(),
                    IndexMetadata {
                        name,
                        table,
                        column,
                    },
                );
                self.persist()?;
                Ok(ExecResult::Message("index created".into(), 0))
            }
            Statement::DropIndex { name } => {
                self.storage.snapshot_mut().catalog.indexes.remove(&name);
                self.persist()?;
                Ok(ExecResult::Message("index dropped".into(), 0))
            }
            Statement::Insert {
                table,
                columns,
                values,
            } => self.insert(&table, columns, values),
            Statement::Select {
                items,
                from,
                join,
                filter,
                order_by,
                limit,
            } => self.select(items, from, join, filter, order_by, limit),
            Statement::Update {
                table,
                assignments,
                filter,
            } => self.update(table, assignments, filter),
            Statement::Delete { table, filter } => self.delete(table, filter),
            Statement::Begin => Ok(ExecResult::Message("transaction started".into(), 0)),
            Statement::Commit => {
                self.persist()?;
                Ok(ExecResult::Message("committed".into(), 0))
            }
            Statement::Rollback => Ok(ExecResult::Message("rolled back".into(), 0)),
        }
    }
    fn persist(&mut self) -> Result<(), MiniDbError> {
        let bytes = serde_json::to_vec(&self.storage.snapshot().clone())
            .map_err(|e| MiniDbError::storage(e.to_string()))?;
        self.storage
            .wal_append(&crate::storage::wal::WalRecord::Snapshot(bytes.clone()))?;
        self.storage.persist()
    }
    fn insert(
        &mut self,
        table: &str,
        columns: Option<Vec<String>>,
        values: Vec<Expr>,
    ) -> Result<ExecResult, MiniDbError> {
        let meta = self
            .storage
            .snapshot()
            .catalog
            .tables
            .get(table)
            .cloned()
            .ok_or_else(|| MiniDbError::sql("unknown table"))?;
        let mut row = vec![Value::Null; meta.columns.len()];
        let vals = values
            .into_iter()
            .map(|e| eval_expr(&e, None, &meta.columns))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(cols) = columns {
            for (c, v) in cols.into_iter().zip(vals) {
                let i = meta
                    .column_index(&c)
                    .ok_or_else(|| MiniDbError::sql("unknown column"))?;
                row[i] = v;
            }
        } else {
            row = vals;
        }
        for (i, col) in meta.columns.iter().enumerate() {
            if row[i].is_null() {
                if let Some(d) = &col.default {
                    row[i] = d.clone();
                }
            }
            if col.not_null && row[i].is_null() {
                return Err(MiniDbError::sql(format!("{} cannot be null", col.name)));
            }
        }
        self.storage
            .snapshot_mut()
            .rows
            .entry(table.to_string())
            .or_default()
            .push(Row(row));
        self.persist()?;
        Ok(ExecResult::Message("1 row inserted".into(), 1))
    }
    fn select(
        &mut self,
        items: Vec<SelectItem>,
        from: String,
        join: Option<Join>,
        filter: Option<Expr>,
        order_by: Option<OrderBy>,
        limit: Option<(usize, usize)>,
    ) -> Result<ExecResult, MiniDbError> {
        let meta = self
            .storage
            .snapshot()
            .catalog
            .tables
            .get(&from)
            .cloned()
            .ok_or_else(|| MiniDbError::sql("unknown table"))?;
        let mut rows = self
            .storage
            .snapshot()
            .rows
            .get(&from)
            .cloned()
            .unwrap_or_default();
        let schema = meta.columns;
        if let Some(join) = join {
            let right_meta = self
                .storage
                .snapshot()
                .catalog
                .tables
                .get(&join.right)
                .cloned()
                .ok_or_else(|| MiniDbError::sql("unknown table"))?;
            let right_rows = self
                .storage
                .snapshot()
                .rows
                .get(&join.right)
                .cloned()
                .unwrap_or_default();
            let mut joined = Vec::new();
            for l in rows {
                for r in &right_rows {
                    let mut v = l.0.clone();
                    v.extend(r.0.clone());
                    let row = Row(v);
                    if eval_truth(&join.on, Some(&row), &schema, Some(&right_meta.columns))?
                        == TruthValue::True
                    {
                        joined.push(row);
                    }
                }
            }
            rows = joined;
        }
        if let Some(expr) = filter {
            rows = rows
                .into_iter()
                .filter(|r| {
                    eval_truth(&expr, Some(r), &schema, None).unwrap_or(TruthValue::False)
                        == TruthValue::True
                })
                .collect();
        }
        if let Some(ord) = order_by {
            rows.sort_by(|a, b| {
                compare_values(
                    &eval_expr(&ord.expr, Some(a), &schema).unwrap_or(Value::Null),
                    &eval_expr(&ord.expr, Some(b), &schema).unwrap_or(Value::Null),
                )
                .unwrap_or(Ordering::Equal)
            });
            if ord.desc {
                rows.reverse();
            }
        }
        if let Some((lim, off)) = limit {
            rows = rows.into_iter().skip(off).take(lim).collect();
        }
        let out_rows: Vec<Vec<Value>> = rows
            .iter()
            .map(|r| {
                if items.len() == 1 && matches!(items[0], SelectItem::Star) {
                    r.0.clone()
                } else {
                    items
                        .iter()
                        .map(|it| match it {
                            SelectItem::Star => Value::Null,
                            SelectItem::Expr(e) => {
                                eval_expr(e, Some(r), &schema).unwrap_or(Value::Null)
                            }
                        })
                        .collect()
                }
            })
            .collect();
        Ok(ExecResult::Rows {
            columns: if items.len() == 1 && matches!(items[0], SelectItem::Star) {
                schema.into_iter().map(|c| c.name).collect()
            } else {
                items
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("c{}", i + 1))
                    .collect()
            },
            rows: out_rows,
        })
    }
    fn update(
        &mut self,
        table: String,
        assignments: Vec<(String, Expr)>,
        filter: Option<Expr>,
    ) -> Result<ExecResult, MiniDbError> {
        let meta = self
            .storage
            .snapshot()
            .catalog
            .tables
            .get(&table)
            .cloned()
            .ok_or_else(|| MiniDbError::sql("unknown table"))?;
        let rows = self
            .storage
            .snapshot_mut()
            .rows
            .entry(table.clone())
            .or_default();
        let mut count = 0;
        for row in rows.iter_mut() {
            if filter
                .as_ref()
                .map(|e| {
                    eval_truth(e, Some(row), &meta.columns, None).unwrap_or(TruthValue::False)
                        == TruthValue::True
                })
                .unwrap_or(true)
            {
                for (c, e) in &assignments {
                    let i = meta
                        .column_index(c)
                        .ok_or_else(|| MiniDbError::sql("unknown column"))?;
                    row.0[i] = eval_expr(e, Some(row), &meta.columns)?;
                }
                count += 1;
            }
        }
        self.persist()?;
        Ok(ExecResult::Message(
            format!("ok {count} rows affected"),
            count,
        ))
    }
    fn delete(&mut self, table: String, filter: Option<Expr>) -> Result<ExecResult, MiniDbError> {
        let meta = self
            .storage
            .snapshot()
            .catalog
            .tables
            .get(&table)
            .cloned()
            .ok_or_else(|| MiniDbError::sql("unknown table"))?;
        let rows = self
            .storage
            .snapshot_mut()
            .rows
            .entry(table.clone())
            .or_default();
        let before = rows.len();
        rows.retain(|row| {
            filter
                .as_ref()
                .map(|e| {
                    eval_truth(e, Some(row), &meta.columns, None).unwrap_or(TruthValue::False)
                        != TruthValue::True
                })
                .unwrap_or(true)
        });
        let count = before - rows.len();
        self.persist()?;
        Ok(ExecResult::Message(
            format!("ok {count} rows affected"),
            count,
        ))
    }
}

pub enum ExecResult {
    Rows {
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
    },
    Message(String, usize),
}

fn eval_truth(
    expr: &Expr,
    row: Option<&Row>,
    schema: &[Column],
    _right_schema: Option<&[Column]>,
) -> Result<TruthValue, MiniDbError> {
    match eval_expr(expr, row, schema)? {
        Value::Bool(v) => Ok(if v {
            TruthValue::True
        } else {
            TruthValue::False
        }),
        Value::Null => Ok(TruthValue::Unknown),
        _ => Err(MiniDbError::sql("expected boolean")),
    }
}

fn eval_expr(expr: &Expr, row: Option<&Row>, schema: &[Column]) -> Result<Value, MiniDbError> {
    eval_expr_inner(expr, row, schema)
}
fn eval_expr_inner(
    expr: &Expr,
    row: Option<&Row>,
    schema: &[Column],
) -> Result<Value, MiniDbError> {
    match expr {
        Expr::Literal(v) => Ok(v.clone()),
        Expr::Column { table: _, name } => {
            let i = schema
                .iter()
                .position(|c| c.name == *name)
                .ok_or_else(|| MiniDbError::sql("unknown column"))?;
            Ok(row.and_then(|r| r.get(i)).cloned().unwrap_or(Value::Null))
        }
        Expr::Unary { op, expr } => match op {
            UnaryOp::Neg => match eval_expr_inner(expr, row, schema)? {
                Value::Int(i) => Ok(Value::Int(-i)),
                Value::Real(f) => Ok(Value::Real(-f)),
                Value::Null => Ok(Value::Null),
                _ => Err(MiniDbError::sql("bad unary op")),
            },
            UnaryOp::Not => Ok(Value::Bool(matches!(
                eval_truth(expr, row, schema, None)?,
                TruthValue::False
            ))),
        },
        Expr::Binary { left, op, right } => {
            let l = eval_expr_inner(left, row, schema)?;
            let r = eval_expr_inner(right, row, schema)?;
            match op {
                BinaryOp::Add => num_bin(l, r, |a, b| a + b, |a, b| a + b),
                BinaryOp::Sub => num_bin(l, r, |a, b| a - b, |a, b| a - b),
                BinaryOp::Mul => num_bin(l, r, |a, b| a * b, |a, b| a * b),
                BinaryOp::Div => num_bin(l, r, |a, b| a / b, |a, b| a / b),
                BinaryOp::Concat => Ok(Value::Text(format!("{}{}", l, r))),
                BinaryOp::Eq => cmp_bin(l, r, |o| o == Ordering::Equal),
                BinaryOp::Neq => cmp_bin(l, r, |o| o != Ordering::Equal),
                BinaryOp::Lt => cmp_bin(l, r, |o| o == Ordering::Less),
                BinaryOp::Lte => cmp_bin(l, r, |o| o != Ordering::Greater),
                BinaryOp::Gt => cmp_bin(l, r, |o| o == Ordering::Greater),
                BinaryOp::Gte => cmp_bin(l, r, |o| o != Ordering::Less),
                BinaryOp::And => Ok(truth_to_value(tv(l).and(tv(r)))),
                BinaryOp::Or => Ok(truth_to_value(tv(l).or(tv(r)))),
            }
        }
        Expr::IsNull { expr, negated } => {
            let v = eval_expr_inner(expr, row, schema)?;
            Ok(Value::Bool(if *negated {
                !v.is_null()
            } else {
                v.is_null()
            }))
        }
        Expr::Func { name, args } => eval_func(name, args, row, schema),
        Expr::Wildcard => Ok(Value::Null),
    }
}
fn tv(v: Value) -> TruthValue {
    match v {
        Value::Bool(true) => TruthValue::True,
        Value::Bool(false) => TruthValue::False,
        Value::Null => TruthValue::Unknown,
        _ => TruthValue::Unknown,
    }
}
fn truth_to_value(v: TruthValue) -> Value {
    Value::Bool(matches!(v, TruthValue::True))
}
fn num_bin(
    l: Value,
    r: Value,
    iop: fn(i64, i64) -> i64,
    fop: fn(f64, f64) -> f64,
) -> Result<Value, MiniDbError> {
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(iop(a, b))),
        (Value::Real(a), Value::Real(b)) => Ok(Value::Real(fop(a, b))),
        (Value::Int(a), Value::Real(b)) => Ok(Value::Real(fop(a as f64, b))),
        (Value::Real(a), Value::Int(b)) => Ok(Value::Real(fop(a, b as f64))),
        (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
        _ => Err(MiniDbError::sql("bad arithmetic")),
    }
}
fn cmp_bin(l: Value, r: Value, f: fn(Ordering) -> bool) -> Result<Value, MiniDbError> {
    Ok(Value::Bool(compare_values(&l, &r).map(f).unwrap_or(false)))
}
fn eval_func(
    name: &str,
    args: &[Expr],
    row: Option<&Row>,
    schema: &[Column],
) -> Result<Value, MiniDbError> {
    let vals: Vec<Value> = args
        .iter()
        .map(|e| eval_expr_inner(e, row, schema))
        .collect::<Result<_, _>>()?;
    Ok(match name {
        "LENGTH" => match vals.get(0) {
            Some(Value::Text(s)) => Value::Int(s.len() as i64),
            _ => Value::Null,
        },
        "LOWER" => match vals.get(0) {
            Some(Value::Text(s)) => Value::Text(s.to_lowercase()),
            _ => Value::Null,
        },
        "UPPER" => match vals.get(0) {
            Some(Value::Text(s)) => Value::Text(s.to_uppercase()),
            _ => Value::Null,
        },
        "ABS" => match vals.get(0) {
            Some(Value::Int(i)) => Value::Int(i.abs()),
            Some(Value::Real(f)) => Value::Real(f.abs()),
            _ => Value::Null,
        },
        "SUBSTR" => match (vals.get(0), vals.get(1), vals.get(2)) {
            (Some(Value::Text(s)), Some(Value::Int(start)), Some(Value::Int(len))) => {
                let start = (*start).max(1) as usize - 1;
                Value::Text(s.chars().skip(start).take(*len as usize).collect())
            }
            _ => Value::Null,
        },
        "COALESCE" => vals
            .into_iter()
            .find(|v| !v.is_null())
            .unwrap_or(Value::Null),
        _ => return Err(MiniDbError::sql("unknown function")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arithmetic_works() {
        assert_eq!(
            num_bin(Value::Int(2), Value::Int(3), |a, b| a + b, |a, b| a + b).unwrap(),
            Value::Int(5)
        );
    }
}
