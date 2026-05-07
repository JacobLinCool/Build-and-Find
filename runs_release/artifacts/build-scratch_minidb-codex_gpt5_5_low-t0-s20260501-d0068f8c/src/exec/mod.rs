use crate::{
    catalog::TableSchema,
    error::{MiniDbError, Result},
    parser::ast::*,
    storage::{self, RowId, StorageEngine},
    types::{Row, Truth, Value},
};
use std::cmp::Ordering;

#[derive(Debug, Clone)]
pub struct ExecRow {
    pub row_id: Option<RowId>,
    pub values: Row,
}

pub trait PhysicalOperator {
    fn open(&mut self) -> Result<()>;
    fn next(&mut self) -> Result<Option<ExecRow>>;
    fn close(&mut self) -> Result<()>;
    fn schema(&self) -> Vec<String>;
}

pub struct SeqScan {
    rows: Vec<(RowId, Row)>,
    schema: Vec<String>,
    pos: usize,
}
impl SeqScan {
    pub fn new(schema: Vec<String>, rows: Vec<(RowId, Row)>) -> Self {
        Self {
            rows,
            schema,
            pos: 0,
        }
    }
}
impl PhysicalOperator for SeqScan {
    fn open(&mut self) -> Result<()> {
        self.pos = 0;
        Ok(())
    }
    fn next(&mut self) -> Result<Option<ExecRow>> {
        let Some((id, row)) = self.rows.get(self.pos).cloned() else {
            return Ok(None);
        };
        self.pos += 1;
        Ok(Some(ExecRow {
            row_id: Some(id),
            values: row,
        }))
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> Vec<String> {
        self.schema.clone()
    }
}

pub struct IndexScan {
    inner: SeqScan,
}
impl IndexScan {
    pub fn new(schema: Vec<String>, rows: Vec<(RowId, Row)>) -> Self {
        Self {
            inner: SeqScan::new(schema, rows),
        }
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
    fn schema(&self) -> Vec<String> {
        self.inner.schema()
    }
}

pub struct Filter {
    input: Box<dyn PhysicalOperator>,
    expr: Expr,
}
impl Filter {
    pub fn new(input: Box<dyn PhysicalOperator>, expr: Expr) -> Self {
        Self { input, expr }
    }
}
impl PhysicalOperator for Filter {
    fn open(&mut self) -> Result<()> {
        self.input.open()
    }
    fn next(&mut self) -> Result<Option<ExecRow>> {
        let names = self.input.schema();
        while let Some(r) = self.input.next()? {
            if eval(&self.expr, &names, &r.values)?.as_truth().is_true() {
                return Ok(Some(r));
            }
        }
        Ok(None)
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
    items: Vec<SelectItem>,
    names: Vec<String>,
}
impl Project {
    pub fn new(input: Box<dyn PhysicalOperator>, items: Vec<SelectItem>) -> Self {
        let names = derive_names(&items, &input.schema());
        Self {
            input,
            items,
            names,
        }
    }
}
impl PhysicalOperator for Project {
    fn open(&mut self) -> Result<()> {
        self.input.open()
    }
    fn next(&mut self) -> Result<Option<ExecRow>> {
        let Some(r) = self.input.next()? else {
            return Ok(None);
        };
        let in_names = self.input.schema();
        let mut values = Vec::new();
        for item in &self.items {
            match item {
                SelectItem::Star => values.extend(r.values.clone()),
                SelectItem::Expr { expr, .. } => values.push(eval(expr, &in_names, &r.values)?),
            }
        }
        Ok(Some(ExecRow {
            row_id: r.row_id,
            values,
        }))
    }
    fn close(&mut self) -> Result<()> {
        self.input.close()
    }
    fn schema(&self) -> Vec<String> {
        self.names.clone()
    }
}

pub struct NestedLoopJoin {
    left: Box<dyn PhysicalOperator>,
    right_rows: Vec<ExecRow>,
    on: Expr,
    li: Option<ExecRow>,
    ri: usize,
    schema: Vec<String>,
}
impl NestedLoopJoin {
    pub fn new(
        left: Box<dyn PhysicalOperator>,
        mut right: Box<dyn PhysicalOperator>,
        on: Expr,
    ) -> Result<Self> {
        right.open()?;
        let mut rows = Vec::new();
        while let Some(r) = right.next()? {
            rows.push(r);
        }
        let right_schema = right.schema();
        let mut schema = left.schema();
        schema.extend(right_schema);
        Ok(Self {
            left,
            right_rows: rows,
            on,
            li: None,
            ri: 0,
            schema,
        })
    }
}
impl PhysicalOperator for NestedLoopJoin {
    fn open(&mut self) -> Result<()> {
        self.left.open()?;
        self.li = None;
        self.ri = 0;
        Ok(())
    }
    fn next(&mut self) -> Result<Option<ExecRow>> {
        loop {
            if self.li.is_none() {
                self.li = self.left.next()?;
                self.ri = 0;
                if self.li.is_none() {
                    return Ok(None);
                }
            }
            while self.ri < self.right_rows.len() {
                let rr = self.right_rows[self.ri].clone();
                self.ri += 1;
                let mut vals = self.li.as_ref().map_or_else(Vec::new, |r| r.values.clone());
                vals.extend(rr.values);
                if eval(&self.on, &self.schema, &vals)?.as_truth().is_true() {
                    return Ok(Some(ExecRow {
                        row_id: None,
                        values: vals,
                    }));
                }
            }
            self.li = None;
        }
    }
    fn close(&mut self) -> Result<()> {
        self.left.close()
    }
    fn schema(&self) -> Vec<String> {
        self.schema.clone()
    }
}

pub struct Sort {
    input: Box<dyn PhysicalOperator>,
    expr: Expr,
    asc: bool,
    rows: Vec<ExecRow>,
    pos: usize,
}
impl Sort {
    pub fn new(input: Box<dyn PhysicalOperator>, expr: Expr, asc: bool) -> Self {
        Self {
            input,
            expr,
            asc,
            rows: Vec::new(),
            pos: 0,
        }
    }
}
impl PhysicalOperator for Sort {
    fn open(&mut self) -> Result<()> {
        self.input.open()?;
        self.rows.clear();
        self.pos = 0;
        let names = self.input.schema();
        while let Some(r) = self.input.next()? {
            self.rows.push(r);
        }
        let expr = self.expr.clone();
        let asc = self.asc;
        self.rows.sort_by(|a, b| {
            let av = eval(&expr, &names, &a.values).map_or(Value::Null, |v| v);
            let bv = eval(&expr, &names, &b.values).map_or(Value::Null, |v| v);
            let o = av.compare(&bv).map_or(Ordering::Equal, |v| v);
            if asc { o } else { o.reverse() }
        });
        Ok(())
    }
    fn next(&mut self) -> Result<Option<ExecRow>> {
        let r = self.rows.get(self.pos).cloned();
        self.pos += usize::from(r.is_some());
        Ok(r)
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
    limit: Option<usize>,
    offset: usize,
    seen: usize,
    yielded: usize,
}
impl Limit {
    pub fn new(input: Box<dyn PhysicalOperator>, limit: Option<usize>, offset: usize) -> Self {
        Self {
            input,
            limit,
            offset,
            seen: 0,
            yielded: 0,
        }
    }
}
impl PhysicalOperator for Limit {
    fn open(&mut self) -> Result<()> {
        self.seen = 0;
        self.yielded = 0;
        self.input.open()
    }
    fn next(&mut self) -> Result<Option<ExecRow>> {
        while self.seen < self.offset {
            if self.input.next()?.is_none() {
                return Ok(None);
            }
            self.seen += 1;
        }
        if self.limit.is_some_and(|l| self.yielded >= l) {
            return Ok(None);
        }
        let r = self.input.next()?;
        if r.is_some() {
            self.yielded += 1;
        }
        Ok(r)
    }
    fn close(&mut self) -> Result<()> {
        self.input.close()
    }
    fn schema(&self) -> Vec<String> {
        self.input.schema()
    }
}

pub struct Insert;
pub struct Update;
pub struct Delete;

pub fn eval(expr: &Expr, names: &[String], row: &Row) -> Result<Value> {
    match expr {
        Expr::Literal(v) => Ok(v.clone()),
        Expr::Column(c) => {
            let lc = c.to_ascii_lowercase();
            let pos = names
                .iter()
                .position(|n| {
                    n.eq_ignore_ascii_case(c)
                        || n.rsplit('.')
                            .next()
                            .is_some_and(|s| s.eq_ignore_ascii_case(&lc))
                })
                .ok_or_else(|| MiniDbError::Sql(format!("unknown column {c}")))?;
            Ok(row.get(pos).cloned().map_or(Value::Null, |v| v))
        }
        Expr::Unary { op, expr } => match op {
            UnaryOp::Not => Ok(Value::Bool(
                eval(expr, names, row)?.as_truth().not().is_true(),
            )),
            UnaryOp::Neg => match eval(expr, names, row)? {
                Value::Int(v) => Ok(Value::Int(-v)),
                Value::Real(v) => Ok(Value::Real(-v)),
                Value::Null => Ok(Value::Null),
                v => Err(MiniDbError::Type(format!("cannot negate {}", v.sql_type()))),
            },
        },
        Expr::IsNull { expr, not } => Ok(Value::Bool(
            matches!(eval(expr, names, row)?, Value::Null) ^ *not,
        )),
        Expr::Binary { left, op, right } => {
            binary(eval(left, names, row)?, *op, eval(right, names, row)?)
        }
        Expr::Func { name, args } => func(name, args, names, row),
    }
}

fn binary(a: Value, op: BinaryOp, b: Value) -> Result<Value> {
    use BinaryOp::*;
    if matches!(op, And | Or) {
        let t = if op == And {
            a.as_truth().and(b.as_truth())
        } else {
            a.as_truth().or(b.as_truth())
        };
        return Ok(match t {
            Truth::True => Value::Bool(true),
            Truth::False => Value::Bool(false),
            Truth::Unknown => Value::Null,
        });
    }
    if matches!(a, Value::Null) || matches!(b, Value::Null) {
        return Ok(Value::Null);
    }
    match op {
        Add | Sub | Mul | Div => num(a, b, op),
        Concat => Ok(Value::Text(format!("{a}{b}"))),
        Eq | Ne | Lt | Le | Gt | Ge => {
            let ord = a.compare(&b);
            let ok = match (op, ord) {
                (_, None) => return Ok(Value::Null),
                (Eq, Some(Ordering::Equal))
                | (Ne, Some(Ordering::Less | Ordering::Greater))
                | (Lt, Some(Ordering::Less))
                | (Le, Some(Ordering::Less | Ordering::Equal))
                | (Gt, Some(Ordering::Greater))
                | (Ge, Some(Ordering::Greater | Ordering::Equal)) => true,
                _ => false,
            };
            Ok(Value::Bool(ok))
        }
        And | Or => unreachable!(),
    }
}
fn num(a: Value, b: Value, op: BinaryOp) -> Result<Value> {
    let real = matches!(a, Value::Real(_)) || matches!(b, Value::Real(_));
    let af = match a {
        Value::Int(v) => v as f64,
        Value::Real(v) => v,
        _ => return Err(MiniDbError::Type("numeric operand required".into())),
    };
    let bf = match b {
        Value::Int(v) => v as f64,
        Value::Real(v) => v,
        _ => return Err(MiniDbError::Type("numeric operand required".into())),
    };
    let r = match op {
        BinaryOp::Add => af + bf,
        BinaryOp::Sub => af - bf,
        BinaryOp::Mul => af * bf,
        BinaryOp::Div => af / bf,
        _ => af,
    };
    if real || matches!(op, BinaryOp::Div) {
        Ok(Value::Real(r))
    } else {
        Ok(Value::Int(r as i64))
    }
}
fn func(name: &str, args: &[Expr], names: &[String], row: &Row) -> Result<Value> {
    let vals = args
        .iter()
        .map(|e| eval(e, names, row))
        .collect::<Result<Vec<_>>>()?;
    match name.to_ascii_uppercase().as_str() {
        "LENGTH" => Ok(match vals.first() {
            Some(Value::Text(s)) => Value::Int(s.chars().count() as i64),
            Some(Value::Null) | None => Value::Null,
            _ => return Err(MiniDbError::Type("LENGTH expects text".into())),
        }),
        "LOWER" => Ok(match vals.first() {
            Some(Value::Text(s)) => Value::Text(s.to_lowercase()),
            Some(Value::Null) | None => Value::Null,
            _ => return Err(MiniDbError::Type("LOWER expects text".into())),
        }),
        "UPPER" => Ok(match vals.first() {
            Some(Value::Text(s)) => Value::Text(s.to_uppercase()),
            Some(Value::Null) | None => Value::Null,
            _ => return Err(MiniDbError::Type("UPPER expects text".into())),
        }),
        "ABS" => Ok(match vals.first() {
            Some(Value::Int(v)) => Value::Int(v.abs()),
            Some(Value::Real(v)) => Value::Real(v.abs()),
            Some(Value::Null) | None => Value::Null,
            _ => return Err(MiniDbError::Type("ABS expects number".into())),
        }),
        "COALESCE" => Ok(vals
            .into_iter()
            .find(|v| !matches!(v, Value::Null))
            .map_or(Value::Null, |v| v)),
        "SUBSTR" => {
            let s = match vals.first() {
                Some(Value::Text(s)) => s,
                Some(Value::Null) | None => return Ok(Value::Null),
                _ => return Err(MiniDbError::Type("SUBSTR expects text".into())),
            };
            let start = match vals.get(1) {
                Some(Value::Int(i)) => *i.max(&1) as usize - 1,
                _ => 0,
            };
            let len = match vals.get(2) {
                Some(Value::Int(i)) => *i as usize,
                _ => s.len(),
            };
            Ok(Value::Text(s.chars().skip(start).take(len).collect()))
        }
        _ => Err(MiniDbError::Sql(format!("unknown function {name}"))),
    }
}
fn derive_names(items: &[SelectItem], input: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        match item {
            SelectItem::Star => out.extend(input.iter().cloned()),
            SelectItem::Expr { expr, alias } => out.push(alias.clone().map_or_else(
                || match expr {
                    Expr::Column(c) => c.clone(),
                    _ => "expr".into(),
                },
                |v| v,
            )),
        }
    }
    out
}
pub fn names_for(schema: &TableSchema) -> Vec<String> {
    schema
        .columns
        .iter()
        .map(|c| format!("{}.{}", schema.name, c.name))
        .collect()
}

pub fn scan_for_select(
    db: &StorageEngine,
    table: &str,
    pred: &Option<Expr>,
) -> Result<Box<dyn PhysicalOperator>> {
    let schema = db.catalog().table(table)?;
    let names = names_for(schema);
    let all = db.table_rows(table)?;
    if let Some((col, val, op)) = simple_index_pred(pred) {
        if db.catalog().index_on(table, &col).is_some() {
            let idx = storage::indexed_rows(db.image(), table, &col)?;
            let ids = if op == BinaryOp::Eq {
                idx.eq(&val)
            } else {
                idx.range(
                    if matches!(op, BinaryOp::Gt | BinaryOp::Ge) {
                        Some(&val)
                    } else {
                        None
                    },
                    if matches!(op, BinaryOp::Lt | BinaryOp::Le) {
                        Some(&val)
                    } else {
                        None
                    },
                )
            };
            let rows = all.into_iter().filter(|(id, _)| ids.contains(id)).collect();
            return Ok(Box::new(IndexScan::new(names, rows)));
        }
    }
    Ok(Box::new(SeqScan::new(names, all)))
}
fn simple_index_pred(pred: &Option<Expr>) -> Option<(String, Value, BinaryOp)> {
    if let Some(Expr::Binary { left, op, right }) = pred {
        if matches!(
            op,
            BinaryOp::Eq | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
        ) {
            if let (Expr::Column(c), Expr::Literal(v)) = (&**left, &**right) {
                return Some((
                    c.rsplit('.').next().map_or(c.as_str(), |s| s).to_string(),
                    v.clone(),
                    *op,
                ));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arithmetic() {
        assert_eq!(
            eval(
                &Expr::Binary {
                    left: Box::new(Expr::Literal(Value::Int(2))),
                    op: BinaryOp::Add,
                    right: Box::new(Expr::Literal(Value::Int(3)))
                },
                &[],
                &vec![]
            )
            .unwrap(),
            Value::Int(5)
        );
    }
}
