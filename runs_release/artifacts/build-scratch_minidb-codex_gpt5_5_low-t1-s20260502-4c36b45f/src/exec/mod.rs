use std::cmp::Ordering;
use std::collections::BTreeSet;

use crate::{
    catalog::{IndexMeta, TableSchema},
    error::{MiniDbError, Result},
    parser::ast::*,
    storage::{Storage, StoredRow},
    types::{Row, RowId, Truth, Value, compare},
};

#[derive(Clone, Debug)]
pub struct Schema {
    pub columns: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    pub affected: Option<usize>,
    pub message: String,
}

pub trait PhysicalOperator {
    fn open(&mut self, storage: &mut Storage) -> Result<()>;
    fn next(&mut self, storage: &mut Storage) -> Result<Option<Row>>;
    fn close(&mut self, _storage: &mut Storage) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> Schema;
}

pub struct SeqScan {
    pub table: String,
    rows: Vec<Row>,
    pos: usize,
    schema: Schema,
}
pub struct IndexScan {
    pub table: String,
    pub index: String,
    pub key: Value,
    rows: Vec<Row>,
    pos: usize,
    schema: Schema,
}
#[allow(dead_code)]
pub struct Filter {
    pub input: Box<dyn PhysicalOperator>,
    pub predicate: Expr,
    rows: Vec<Row>,
    pos: usize,
    schema: Schema,
}
#[allow(dead_code)]
pub struct Project {
    pub input: Box<dyn PhysicalOperator>,
    pub projection: Vec<SelectItem>,
    rows: Vec<Row>,
    pos: usize,
    schema: Schema,
}
#[allow(dead_code)]
pub struct NestedLoopJoin {
    pub left: Box<dyn PhysicalOperator>,
    pub right: Box<dyn PhysicalOperator>,
    rows: Vec<Row>,
    pos: usize,
    schema: Schema,
}
#[allow(dead_code)]
pub struct Sort {
    pub input: Box<dyn PhysicalOperator>,
    rows: Vec<Row>,
    pos: usize,
    schema: Schema,
}
#[allow(dead_code)]
pub struct Limit {
    pub input: Box<dyn PhysicalOperator>,
    pub limit: usize,
    rows: Vec<Row>,
    pos: usize,
    schema: Schema,
}
pub struct Insert;
pub struct Update;
pub struct Delete;

impl SeqScan {
    pub fn new(table: String) -> Self {
        Self {
            table,
            rows: vec![],
            pos: 0,
            schema: Schema { columns: vec![] },
        }
    }
}
impl PhysicalOperator for SeqScan {
    fn open(&mut self, storage: &mut Storage) -> Result<()> {
        let table = self.table.to_ascii_lowercase();
        let schema = storage.image.catalog.table(&table)?;
        self.schema = Schema {
            columns: schema.columns.iter().map(|c| c.name.clone()).collect(),
        };
        self.rows = storage
            .image
            .tables
            .get(&table)
            .map(|t| {
                t.rows
                    .iter()
                    .filter(|r| !r.deleted)
                    .map(|r| r.values.clone())
                    .collect()
            })
            .map_or_else(Vec::new, |v| v);
        self.pos = 0;
        Ok(())
    }
    fn next(&mut self, _storage: &mut Storage) -> Result<Option<Row>> {
        let row = self.rows.get(self.pos).cloned();
        self.pos += usize::from(row.is_some());
        Ok(row)
    }
    fn schema(&self) -> Schema {
        self.schema.clone()
    }
}

impl PhysicalOperator for IndexScan {
    fn open(&mut self, storage: &mut Storage) -> Result<()> {
        let schema = storage.image.catalog.table(&self.table)?;
        self.schema = Schema {
            columns: schema.columns.iter().map(|c| c.name.clone()).collect(),
        };
        let ids = storage
            .image
            .indexes
            .get(&self.index)
            .map(|idx| idx.eq(&self.key))
            .map_or_else(Vec::new, |v| v);
        let idset: BTreeSet<RowId> = ids.into_iter().collect();
        self.rows = storage
            .image
            .tables
            .get(&self.table)
            .map(|t| {
                t.rows
                    .iter()
                    .filter(|r| !r.deleted && idset.contains(&r.id))
                    .map(|r| r.values.clone())
                    .collect()
            })
            .map_or_else(Vec::new, |v| v);
        self.pos = 0;
        Ok(())
    }
    fn next(&mut self, _storage: &mut Storage) -> Result<Option<Row>> {
        let row = self.rows.get(self.pos).cloned();
        self.pos += usize::from(row.is_some());
        Ok(row)
    }
    fn schema(&self) -> Schema {
        self.schema.clone()
    }
}

macro_rules! empty_op {
    ($t:ty) => {
        impl PhysicalOperator for $t {
            fn open(&mut self, _storage: &mut Storage) -> Result<()> {
                Ok(())
            }
            fn next(&mut self, _storage: &mut Storage) -> Result<Option<Row>> {
                Ok(None)
            }
            fn schema(&self) -> Schema {
                Schema { columns: vec![] }
            }
        }
    };
}
empty_op!(Insert);
empty_op!(Update);
empty_op!(Delete);
empty_op!(Filter);
empty_op!(Project);
empty_op!(NestedLoopJoin);
empty_op!(Sort);
empty_op!(Limit);

pub struct Executor<'a> {
    pub storage: &'a mut Storage,
}

impl<'a> Executor<'a> {
    pub fn execute(&mut self, stmt: &Statement) -> Result<QueryResult> {
        match stmt {
            Statement::CreateTable(schema) => {
                self.storage.image.catalog.create_table(schema.clone())?;
                self.storage
                    .image
                    .tables
                    .entry(schema.name.clone())
                    .or_default();
                Ok(summary("ok", 0))
            }
            Statement::DropTable(name) => {
                self.storage.image.catalog.drop_table(name)?;
                self.storage.image.tables.remove(&name.to_ascii_lowercase());
                self.storage.rebuild_indexes()?;
                Ok(summary("ok", 0))
            }
            Statement::CreateIndex {
                name,
                table,
                column,
            } => {
                let table = table.to_ascii_lowercase();
                let column = column.to_ascii_lowercase();
                self.storage.image.catalog.table(&table)?;
                self.storage.image.catalog.indexes.insert(
                    name.to_ascii_lowercase(),
                    IndexMeta {
                        name: name.to_ascii_lowercase(),
                        table,
                        column,
                    },
                );
                self.storage.rebuild_indexes()?;
                Ok(summary("ok", 0))
            }
            Statement::DropIndex(name) => {
                self.storage
                    .image
                    .catalog
                    .indexes
                    .remove(&name.to_ascii_lowercase());
                self.storage.rebuild_indexes()?;
                Ok(summary("ok", 0))
            }
            Statement::Insert {
                table,
                columns,
                values,
            } => self.insert(table, columns.as_deref(), values),
            Statement::Select(select) => self.select(select),
            Statement::Update {
                table,
                assigns,
                where_clause,
            } => self.update(table, assigns, where_clause.as_ref()),
            Statement::Delete {
                table,
                where_clause,
            } => self.delete(table, where_clause.as_ref()),
            Statement::Meta(_) | Statement::Begin | Statement::Commit | Statement::Rollback => {
                Ok(summary("ok", 0))
            }
        }
    }

    fn insert(
        &mut self,
        table: &str,
        columns: Option<&[String]>,
        values: &[Expr],
    ) -> Result<QueryResult> {
        let table = table.to_ascii_lowercase();
        let schema = self.storage.image.catalog.table(&table)?.clone();
        if columns.is_none() && values.len() != schema.columns.len() {
            return Err(MiniDbError::Sql(
                "INSERT value count does not match table".into(),
            ));
        }
        let mut row = schema
            .columns
            .iter()
            .map(|c| c.default.clone().map_or(Value::Null, |v| v))
            .collect::<Row>();
        let targets = if let Some(cols) = columns {
            cols.to_vec()
        } else {
            schema.columns.iter().map(|c| c.name.clone()).collect()
        };
        if targets.len() != values.len() {
            return Err(MiniDbError::Sql(
                "INSERT column count does not match values".into(),
            ));
        }
        let empty_row = Vec::new();
        let empty_schema = Schema { columns: vec![] };
        for (col, expr) in targets.iter().zip(values) {
            let idx = column_index(&schema, col)?;
            row[idx] = eval(expr, &empty_row, &empty_schema)?;
        }
        self.validate_row(&schema, &row, None)?;
        let t = self.storage.image.tables.entry(table).or_default();
        let id = t.next_row_id;
        t.next_row_id += 1;
        t.rows.push(StoredRow {
            id,
            values: row,
            deleted: false,
        });
        self.storage.rebuild_indexes()?;
        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected: Some(1),
            message: "ok 1 row affected".into(),
        })
    }

    fn select(&mut self, select: &Select) -> Result<QueryResult> {
        let (mut rows, schema) = self.source_rows(&select.from)?;
        if let Some(w) = &select.where_clause {
            rows.retain(|r| {
                matches!(
                    eval(w, r, &schema).and_then(|v| v.as_truth()),
                    Ok(Truth::True)
                )
            });
        }
        if !select.order_by.is_empty() {
            rows.sort_by(|a, b| {
                for ob in &select.order_by {
                    let av = eval(&ob.expr, a, &schema).map_or(Value::Null, |v| v);
                    let bv = eval(&ob.expr, b, &schema).map_or(Value::Null, |v| v);
                    let ord = compare(&av, &bv).map_or(Ordering::Equal, |o| o);
                    if ord != Ordering::Equal {
                        return if ob.desc { ord.reverse() } else { ord };
                    }
                }
                Ordering::Equal
            });
        }
        let offset = select.offset.map_or(0, |v| v);
        let limit = select.limit.map_or(usize::MAX, |v| v);
        rows = rows.into_iter().skip(offset).take(limit).collect();
        let mut out_cols = Vec::new();
        let mut out_rows = Vec::new();
        for row in &rows {
            let mut out = Vec::new();
            for item in &select.projection {
                match item {
                    SelectItem::Star => {
                        if out_cols.is_empty() {
                            out_cols.extend(schema.columns.clone());
                        }
                        out.extend(row.clone());
                    }
                    SelectItem::Expr { expr, alias } => {
                        if out_cols.len() < select.projection.len() {
                            out_cols.push(alias.clone().map_or_else(|| expr_name(expr), |a| a));
                        }
                        out.push(eval(expr, row, &schema)?);
                    }
                }
            }
            out_rows.push(out);
        }
        if rows.is_empty() {
            for item in &select.projection {
                match item {
                    SelectItem::Star => out_cols.extend(schema.columns.clone()),
                    SelectItem::Expr { expr, alias } => {
                        out_cols.push(alias.clone().map_or_else(|| expr_name(expr), |a| a))
                    }
                }
            }
        }
        let n = out_rows.len();
        Ok(QueryResult {
            columns: out_cols,
            rows: out_rows,
            affected: None,
            message: format!("ok {n} {}", if n == 1 { "row" } else { "rows" }),
        })
    }

    fn source_rows(&self, from: &From) -> Result<(Vec<Row>, Schema)> {
        match from {
            From::Table(t) => {
                let schema = self.storage.image.catalog.table(t)?;
                let cols = schema.columns.iter().map(|c| c.name.clone()).collect();
                let rows = self
                    .storage
                    .image
                    .tables
                    .get(t)
                    .map(|d| {
                        d.rows
                            .iter()
                            .filter(|r| !r.deleted)
                            .map(|r| r.values.clone())
                            .collect()
                    })
                    .map_or_else(Vec::new, |v| v);
                Ok((rows, Schema { columns: cols }))
            }
            From::Join { left, right, on } => {
                let ls = self.storage.image.catalog.table(left)?;
                let rs = self.storage.image.catalog.table(right)?;
                let mut cols: Vec<String> = ls
                    .columns
                    .iter()
                    .map(|c| format!("{left}.{}", c.name))
                    .collect();
                cols.extend(rs.columns.iter().map(|c| format!("{right}.{}", c.name)));
                let schema = Schema { columns: cols };
                let lrows = self
                    .storage
                    .image
                    .tables
                    .get(left)
                    .map(|d| {
                        d.rows
                            .iter()
                            .filter(|r| !r.deleted)
                            .map(|r| r.values.clone())
                            .collect::<Vec<_>>()
                    })
                    .map_or_else(Vec::new, |v| v);
                let rrows = self
                    .storage
                    .image
                    .tables
                    .get(right)
                    .map(|d| {
                        d.rows
                            .iter()
                            .filter(|r| !r.deleted)
                            .map(|r| r.values.clone())
                            .collect::<Vec<_>>()
                    })
                    .map_or_else(Vec::new, |v| v);
                let mut rows = Vec::new();
                for l in &lrows {
                    for r in &rrows {
                        let mut joined = l.clone();
                        joined.extend(r.clone());
                        if matches!(eval(on, &joined, &schema)?.as_truth()?, Truth::True) {
                            rows.push(joined);
                        }
                    }
                }
                Ok((rows, schema))
            }
        }
    }

    fn update(
        &mut self,
        table: &str,
        assigns: &[(String, Expr)],
        where_clause: Option<&Expr>,
    ) -> Result<QueryResult> {
        let table = table.to_ascii_lowercase();
        let schema = self.storage.image.catalog.table(&table)?.clone();
        let scan_schema = Schema {
            columns: schema.columns.iter().map(|c| c.name.clone()).collect(),
        };
        let mut count = 0;
        let rows_snapshot = self
            .storage
            .image
            .tables
            .get(&table)
            .map(|t| t.rows.clone())
            .map_or_else(Vec::new, |v| v);
        let mut new_values = Vec::new();
        for stored in rows_snapshot.iter().filter(|r| !r.deleted) {
            let matches = where_clause
                .map(|w| {
                    matches!(
                        eval(w, &stored.values, &scan_schema).and_then(|v| v.as_truth()),
                        Ok(Truth::True)
                    )
                })
                .map_or(true, |v| v);
            if matches {
                let mut row = stored.values.clone();
                for (col, expr) in assigns {
                    let idx = column_index(&schema, col)?;
                    row[idx] = eval(expr, &stored.values, &scan_schema)?;
                }
                self.validate_row(&schema, &row, Some(stored.id))?;
                new_values.push((stored.id, row));
            }
        }
        if let Some(t) = self.storage.image.tables.get_mut(&table) {
            for (id, row) in new_values {
                if let Some(stored) = t.rows.iter_mut().find(|r| r.id == id) {
                    stored.values = row;
                    count += 1;
                }
            }
        }
        self.storage.rebuild_indexes()?;
        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected: Some(count),
            message: format!(
                "ok {count} {}",
                if count == 1 {
                    "row affected"
                } else {
                    "rows affected"
                }
            ),
        })
    }

    fn delete(&mut self, table: &str, where_clause: Option<&Expr>) -> Result<QueryResult> {
        let table = table.to_ascii_lowercase();
        let schema = self.storage.image.catalog.table(&table)?.clone();
        let scan_schema = Schema {
            columns: schema.columns.iter().map(|c| c.name.clone()).collect(),
        };
        let mut count = 0;
        if let Some(t) = self.storage.image.tables.get_mut(&table) {
            for stored in t.rows.iter_mut().filter(|r| !r.deleted) {
                let matches = where_clause
                    .map(|w| {
                        matches!(
                            eval(w, &stored.values, &scan_schema).and_then(|v| v.as_truth()),
                            Ok(Truth::True)
                        )
                    })
                    .map_or(true, |v| v);
                if matches {
                    stored.deleted = true;
                    count += 1;
                }
            }
        }
        self.storage.rebuild_indexes()?;
        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected: Some(count),
            message: format!(
                "ok {count} {}",
                if count == 1 {
                    "row affected"
                } else {
                    "rows affected"
                }
            ),
        })
    }

    fn validate_row(&self, schema: &TableSchema, row: &Row, current: Option<RowId>) -> Result<()> {
        for (i, c) in schema.columns.iter().enumerate() {
            if c.not_null && row[i].is_null() {
                return Err(MiniDbError::Sql(format!("{} may not be NULL", c.name)));
            }
            if c.unique || c.primary_key {
                if let Some(t) = self.storage.image.tables.get(&schema.name) {
                    for r in t
                        .rows
                        .iter()
                        .filter(|r| !r.deleted && Some(r.id) != current)
                    {
                        if compare(&r.values[i], &row[i]) == Some(Ordering::Equal) {
                            return Err(MiniDbError::Sql(format!(
                                "unique constraint failed: {}.{}",
                                schema.name, c.name
                            )));
                        }
                    }
                }
            }
        }
        let scan_schema = Schema {
            columns: schema.columns.iter().map(|c| c.name.clone()).collect(),
        };
        for check in &schema.checks {
            if !matches!(eval(check, row, &scan_schema)?.as_truth()?, Truth::True) {
                return Err(MiniDbError::Sql("check constraint failed".into()));
            }
        }
        Ok(())
    }
}

pub fn eval(expr: &Expr, row: &Row, schema: &Schema) -> Result<Value> {
    match expr {
        Expr::Literal(v) => Ok(v.clone()),
        Expr::Column { table, name } => {
            let key = table
                .as_ref()
                .map(|t| format!("{t}.{name}"))
                .map_or_else(|| name.clone(), |t| format!("{t}.{name}"));
            let idx = schema
                .columns
                .iter()
                .position(|c| c == &key || c.rsplit('.').next() == Some(name.as_str()))
                .ok_or_else(|| MiniDbError::Sql(format!("unknown column '{name}'")))?;
            Ok(row.get(idx).cloned().map_or(Value::Null, |v| v))
        }
        Expr::Unary { op, expr } => {
            let v = eval(expr, row, schema)?;
            match op {
                UnaryOp::Not => Ok(Value::Bool(matches!(!v.as_truth()?, Truth::True))),
                UnaryOp::Neg => match v {
                    Value::Int(x) => Ok(Value::Int(-x)),
                    Value::Real(x) => Ok(Value::Real(-x)),
                    Value::Null => Ok(Value::Null),
                    _ => Err(MiniDbError::Type("cannot negate value".into())),
                },
            }
        }
        Expr::Binary { left, op, right } => {
            binary(eval(left, row, schema)?, op, eval(right, row, schema)?)
        }
        Expr::Function { name, args } => call(name, args, row, schema),
        Expr::IsNull { expr, negated } => {
            Ok(Value::Bool(eval(expr, row, schema)?.is_null() ^ *negated))
        }
    }
}

fn binary(a: Value, op: &BinaryOp, b: Value) -> Result<Value> {
    if matches!(op, BinaryOp::And | BinaryOp::Or) {
        let x = a.as_truth()?;
        let y = b.as_truth()?;
        return Ok(Value::Bool(matches!(
            if matches!(op, BinaryOp::And) {
                x.and(y)
            } else {
                x.or(y)
            },
            Truth::True
        )));
    }
    if a.is_null() || b.is_null() {
        return Ok(Value::Null);
    }
    match op {
        BinaryOp::Add => num(a, b, |x, y| x + y, |x, y| x + y),
        BinaryOp::Sub => num(a, b, |x, y| x - y, |x, y| x - y),
        BinaryOp::Mul => num(a, b, |x, y| x * y, |x, y| x * y),
        BinaryOp::Div => Ok(Value::Real(to_f64(&a)? / to_f64(&b)?)),
        BinaryOp::Concat => Ok(Value::Text(format!("{}{}", a.display(), b.display()))),
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            let c =
                compare(&a, &b).ok_or_else(|| MiniDbError::Type("incomparable values".into()))?;
            Ok(Value::Bool(match op {
                BinaryOp::Eq => c == Ordering::Equal,
                BinaryOp::Ne => c != Ordering::Equal,
                BinaryOp::Lt => c == Ordering::Less,
                BinaryOp::Le => c != Ordering::Greater,
                BinaryOp::Gt => c == Ordering::Greater,
                BinaryOp::Ge => c != Ordering::Less,
                _ => false,
            }))
        }
        _ => Err(MiniDbError::Type("invalid binary operator".into())),
    }
}

fn num(a: Value, b: Value, intf: fn(i64, i64) -> i64, realf: fn(f64, f64) -> f64) -> Result<Value> {
    match (&a, &b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(intf(*x, *y))),
        _ => Ok(Value::Real(realf(to_f64(&a)?, to_f64(&b)?))),
    }
}
fn to_f64(v: &Value) -> Result<f64> {
    match v {
        Value::Int(x) => Ok(*x as f64),
        Value::Real(x) => Ok(*x),
        _ => Err(MiniDbError::Type("expected number".into())),
    }
}

fn call(name: &str, args: &[Expr], row: &Row, schema: &Schema) -> Result<Value> {
    let vals = args
        .iter()
        .map(|e| eval(e, row, schema))
        .collect::<Result<Vec<_>>>()?;
    match name {
        "length" => Ok(Value::Int(
            vals.first()
                .map_or(0, |v| v.display().chars().count() as i64),
        )),
        "lower" => Ok(Value::Text(
            vals.first()
                .map_or_else(String::new, Value::display)
                .to_lowercase(),
        )),
        "upper" => Ok(Value::Text(
            vals.first()
                .map_or_else(String::new, Value::display)
                .to_uppercase(),
        )),
        "substr" => {
            let s = vals.first().map_or_else(String::new, Value::display);
            let start = match vals.get(1) {
                Some(Value::Int(i)) => (*i).max(1) as usize - 1,
                _ => 0,
            };
            let len = match vals.get(2) {
                Some(Value::Int(i)) => *i as usize,
                _ => s.len(),
            };
            Ok(Value::Text(s.chars().skip(start).take(len).collect()))
        }
        "abs" => match vals.first().cloned().map_or(Value::Null, |v| v) {
            Value::Int(i) => Ok(Value::Int(i.abs())),
            Value::Real(f) => Ok(Value::Real(f.abs())),
            Value::Null => Ok(Value::Null),
            _ => Err(MiniDbError::Type("ABS expects number".into())),
        },
        "coalesce" => Ok(vals
            .into_iter()
            .find(|v| !v.is_null())
            .map_or(Value::Null, |v| v)),
        _ => Err(MiniDbError::Sql(format!("unknown function '{name}'"))),
    }
}

fn column_index(schema: &TableSchema, col: &str) -> Result<usize> {
    schema
        .columns
        .iter()
        .position(|c| c.name == col.to_ascii_lowercase())
        .ok_or_else(|| MiniDbError::Sql(format!("unknown column '{col}'")))
}
fn expr_name(expr: &Expr) -> String {
    match expr {
        Expr::Column { name, .. } => name.clone(),
        Expr::Function { name, .. } => name.clone(),
        _ => "expr".into(),
    }
}
fn summary(msg: &str, n: usize) -> QueryResult {
    QueryResult {
        columns: vec![],
        rows: vec![],
        affected: Some(n),
        message: msg.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn evals_arithmetic() {
        let e = Expr::Binary {
            left: Box::new(Expr::Literal(Value::Int(2))),
            op: BinaryOp::Add,
            right: Box::new(Expr::Literal(Value::Int(3))),
        };
        assert_eq!(
            eval(&e, &vec![], &Schema { columns: vec![] }).unwrap(),
            Value::Int(5)
        );
    }
}
