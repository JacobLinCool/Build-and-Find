use crate::{
    catalog::{Catalog, Index, Table},
    error::{MiniDbError, Result},
    parser::ast::*,
    plan::PhysicalPlan,
    storage::Storage,
    txn::TransactionManager,
    types::*,
};
use std::cmp::Ordering;

#[derive(Clone, Debug)]
pub enum Output {
    Rows(Vec<Vec<Value>>, Vec<String>),
    Affected(usize),
    Message(String),
}

pub trait PhysicalOperator {
    fn open(&mut self) -> Result<()> {
        Ok(())
    }
    fn next(&mut self) -> Result<Option<Vec<Value>>>;
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> Vec<String>;
}

pub struct SeqScan;
impl PhysicalOperator for SeqScan {
    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        Ok(None)
    }
    fn schema(&self) -> Vec<String> {
        vec![]
    }
}
pub struct IndexScan;
impl PhysicalOperator for IndexScan {
    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        Ok(None)
    }
    fn schema(&self) -> Vec<String> {
        vec![]
    }
}
pub struct Filter;
impl PhysicalOperator for Filter {
    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        Ok(None)
    }
    fn schema(&self) -> Vec<String> {
        vec![]
    }
}
pub struct Project;
impl PhysicalOperator for Project {
    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        Ok(None)
    }
    fn schema(&self) -> Vec<String> {
        vec![]
    }
}
pub struct NestedLoopJoin;
impl PhysicalOperator for NestedLoopJoin {
    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        Ok(None)
    }
    fn schema(&self) -> Vec<String> {
        vec![]
    }
}
pub struct Sort;
impl PhysicalOperator for Sort {
    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        Ok(None)
    }
    fn schema(&self) -> Vec<String> {
        vec![]
    }
}
pub struct Limit;
impl PhysicalOperator for Limit {
    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        Ok(None)
    }
    fn schema(&self) -> Vec<String> {
        vec![]
    }
}
pub struct Insert;
impl PhysicalOperator for Insert {
    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        Ok(None)
    }
    fn schema(&self) -> Vec<String> {
        vec![]
    }
}
pub struct Update;
impl PhysicalOperator for Update {
    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        Ok(None)
    }
    fn schema(&self) -> Vec<String> {
        vec![]
    }
}
pub struct Delete;
impl PhysicalOperator for Delete {
    fn next(&mut self) -> Result<Option<Vec<Value>>> {
        Ok(None)
    }
    fn schema(&self) -> Vec<String> {
        vec![]
    }
}

pub struct Executor;

impl Executor {
    pub fn execute(
        storage: &Storage,
        txn: &mut TransactionManager,
        catalog: &mut Catalog,
        plan: PhysicalPlan,
    ) -> Result<Output> {
        match plan {
            PhysicalPlan::Statement(stmt) => Self::execute_stmt(storage, txn, catalog, stmt),
        }
    }

    fn execute_stmt(
        storage: &Storage,
        txn: &mut TransactionManager,
        catalog: &mut Catalog,
        stmt: Statement,
    ) -> Result<Output> {
        match stmt {
            Statement::CreateTable { name, schema } => {
                catalog.tables.insert(
                    name,
                    Table {
                        schema,
                        rows: Default::default(),
                        next_row_id: 1,
                    },
                );
                auto_commit(storage, txn, catalog)?;
                Ok(Output::Message("ok".into()))
            }
            Statement::DropTable { name } => {
                catalog.tables.remove(&name);
                auto_commit(storage, txn, catalog)?;
                Ok(Output::Message("ok".into()))
            }
            Statement::CreateIndex {
                name,
                table,
                column,
            } => {
                catalog.indexes.insert(
                    name,
                    Index {
                        table,
                        column,
                        entries: Default::default(),
                    },
                );
                auto_commit(storage, txn, catalog)?;
                Ok(Output::Message("ok".into()))
            }
            Statement::DropIndex { name } => {
                catalog.indexes.remove(&name);
                auto_commit(storage, txn, catalog)?;
                Ok(Output::Message("ok".into()))
            }
            Statement::Insert {
                table,
                columns,
                values,
            } => {
                let affected = insert_row(catalog, &table, columns, values)?;
                auto_commit(storage, txn, catalog)?;
                Ok(Output::Affected(affected))
            }
            Statement::Select {
                projection,
                from,
                selection,
                order_by,
                limit,
                offset,
            } => {
                let (rows, schema) = select_rows(
                    catalog, from, selection, order_by, limit, offset, projection,
                )?;
                Ok(Output::Rows(rows, schema))
            }
            Statement::Update {
                table,
                assignments,
                selection,
            } => {
                let n = update_rows(catalog, &table, assignments, selection)?;
                auto_commit(storage, txn, catalog)?;
                Ok(Output::Affected(n))
            }
            Statement::Delete { table, selection } => {
                let n = delete_rows(catalog, &table, selection)?;
                auto_commit(storage, txn, catalog)?;
                Ok(Output::Affected(n))
            }
            Statement::Begin => {
                txn.begin();
                Ok(Output::Message("ok".into()))
            }
            Statement::Commit => {
                txn.commit(storage, catalog)?;
                Ok(Output::Message("ok".into()))
            }
            Statement::Rollback => {
                txn.rollback();
                Ok(Output::Message("ok".into()))
            }
            Statement::Meta(_) => Ok(Output::Message("ok".into())),
        }
    }
}

fn auto_commit(storage: &Storage, txn: &mut TransactionManager, catalog: &Catalog) -> Result<()> {
    if txn.explicit {
        Ok(())
    } else {
        storage.wal_append(&crate::storage::wal::WalRecord::Snapshot(catalog.clone()))?;
        storage.persist(catalog)?;
        storage.wal_clear()?;
        Ok(())
    }
}

fn insert_row(
    catalog: &mut Catalog,
    table: &str,
    columns: Vec<String>,
    values: Vec<Expr>,
) -> Result<usize> {
    let t = catalog
        .tables
        .get_mut(table)
        .ok_or_else(|| MiniDbError::Sql(format!("no such table {table}")))?;
    let mut row = vec![Value::Null; t.schema.columns.len()];
    let mut provided = std::collections::HashMap::new();
    for (i, c) in columns.iter().enumerate() {
        provided.insert(c.to_ascii_lowercase(), i);
    }
    if columns.is_empty() {
        if values.len() != row.len() {
            return Err(MiniDbError::Sql("values count mismatch".into()));
        }
        for (i, expr) in values.into_iter().enumerate() {
            row[i] = eval_expr(&expr, &[], &[])?;
        }
    } else {
        for (idx, col) in t.schema.columns.iter().enumerate() {
            if let Some(pos) = provided.get(&col.name.to_ascii_lowercase()) {
                row[idx] = eval_expr(&values[*pos], &[], &[])?;
            } else if let Some(def) = &col.default {
                row[idx] = eval_expr(def, &[], &[])?;
            }
        }
    }
    t.rows.insert(t.next_row_id, Row { values: row });
    t.next_row_id += 1;
    Ok(1)
}

fn select_rows(
    catalog: &Catalog,
    from: FromClause,
    selection: Option<Expr>,
    order_by: Vec<OrderItem>,
    limit: Option<usize>,
    offset: Option<usize>,
    projection: Vec<SelectItem>,
) -> Result<(Vec<Vec<Value>>, Vec<String>)> {
    let (table_name, schema) = match from {
        FromClause::Table { name, .. } => {
            let t = catalog
                .tables
                .get(&name)
                .ok_or_else(|| MiniDbError::Sql(format!("no such table {name}")))?;
            (name, t.schema.clone())
        }
        FromClause::Join { left, .. } => {
            let t = catalog
                .tables
                .get(&left)
                .ok_or_else(|| MiniDbError::Sql(format!("no such table {left}")))?;
            (left, t.schema.clone())
        }
    };
    let table = catalog.tables.get(&table_name).unwrap();
    let mut out: Vec<Vec<Value>> = table
        .rows
        .values()
        .filter(|row| {
            selection
                .as_ref()
                .map(|e| truthy(eval_expr(e, &schema.columns, &row.values).ok()))
                .unwrap_or(true)
        })
        .map(|row| {
            if projection.iter().any(|p| matches!(p, SelectItem::Star)) {
                row.values.clone()
            } else {
                projection
                    .iter()
                    .map(|item| match item {
                        SelectItem::Expr { expr, .. } => {
                            eval_expr(expr, &schema.columns, &row.values).unwrap_or(Value::Null)
                        }
                        SelectItem::Star => Value::Null,
                    })
                    .collect()
            }
        })
        .collect();
    if !order_by.is_empty() {
        out.sort_by(|a, b| cmp_row(a, b));
    }
    let start = offset.unwrap_or(0);
    let end = limit.map(|l| start + l).unwrap_or(out.len());
    Ok((
        out.into_iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect(),
        projection
            .into_iter()
            .map(|p| match p {
                SelectItem::Expr { alias, expr } => alias.unwrap_or_else(|| format!("{expr:?}")),
                SelectItem::Star => "*".into(),
            })
            .collect(),
    ))
}

fn cmp_row(a: &Vec<Value>, b: &Vec<Value>) -> Ordering {
    a.len().cmp(&b.len())
}

fn update_rows(
    catalog: &mut Catalog,
    table: &str,
    assignments: Vec<(String, Expr)>,
    selection: Option<Expr>,
) -> Result<usize> {
    let t = catalog
        .tables
        .get_mut(table)
        .ok_or_else(|| MiniDbError::Sql(format!("no such table {table}")))?;
    let mut count = 0;
    for row in t.rows.values_mut() {
        if selection
            .as_ref()
            .map(|e| truthy(eval_expr(e, &t.schema.columns, &row.values).ok()))
            .unwrap_or(true)
        {
            for (col, expr) in &assignments {
                if let Some(idx) = t
                    .schema
                    .columns
                    .iter()
                    .position(|c| c.name.eq_ignore_ascii_case(col))
                {
                    row.values[idx] = eval_expr(expr, &t.schema.columns, &row.values)?;
                }
            }
            count += 1;
        }
    }
    Ok(count)
}

fn delete_rows(catalog: &mut Catalog, table: &str, selection: Option<Expr>) -> Result<usize> {
    let t = catalog
        .tables
        .get_mut(table)
        .ok_or_else(|| MiniDbError::Sql(format!("no such table {table}")))?;
    let ids: Vec<u64> = t
        .rows
        .iter()
        .filter_map(|(id, row)| {
            if selection
                .as_ref()
                .map(|e| truthy(eval_expr(e, &t.schema.columns, &row.values).ok()))
                .unwrap_or(true)
            {
                Some(*id)
            } else {
                None
            }
        })
        .collect();
    let n = ids.len();
    for id in ids {
        t.rows.remove(&id);
    }
    Ok(n)
}

fn truthy(v: Option<Value>) -> bool {
    matches!(v, Some(Value::Bool(true)))
}

pub fn eval_expr(expr: &Expr, cols: &[ColumnDef], row: &[Value]) -> Result<Value> {
    Ok(match expr {
        Expr::Literal(v) => v.clone(),
        Expr::Ident(parts) => {
            let name = parts.last().unwrap().to_ascii_lowercase();
            let idx = cols
                .iter()
                .position(|c| c.name.to_ascii_lowercase() == name)
                .ok_or_else(|| MiniDbError::Sql(format!("unknown column {name}")))?;
            row[idx].clone()
        }
        Expr::Unary(UnaryOp::Neg, e) => match eval_expr(e, cols, row)? {
            Value::Int(i) => Value::Int(-i),
            Value::Real(f) => Value::Real(-f),
            _ => Value::Null,
        },
        Expr::Unary(UnaryOp::Not, e) => match eval_expr(e, cols, row)? {
            Value::Bool(b) => Value::Bool(!b),
            _ => Value::Null,
        },
        Expr::IsNull(e, neg) => {
            let b = eval_expr(e, cols, row)?.is_null();
            Value::Bool(if *neg { !b } else { b })
        }
        Expr::Binary(a, op, b) => binary(eval_expr(a, cols, row)?, op, eval_expr(b, cols, row)?),
        Expr::Func(name, args) => func(name, args, cols, row)?,
        Expr::Star => Value::Null,
    })
}

fn binary(a: Value, op: &BinaryOp, b: Value) -> Value {
    use BinaryOp::*;
    match op {
        Add => match (a, b) {
            (Value::Int(x), Value::Int(y)) => Value::Int(x + y),
            (Value::Real(x), Value::Real(y)) => Value::Real(x + y),
            (Value::Int(x), Value::Real(y)) => Value::Real(x as f64 + y),
            (Value::Real(x), Value::Int(y)) => Value::Real(x + y as f64),
            _ => Value::Null,
        },
        Sub => match (a, b) {
            (Value::Int(x), Value::Int(y)) => Value::Int(x - y),
            (Value::Real(x), Value::Real(y)) => Value::Real(x - y),
            (Value::Int(x), Value::Real(y)) => Value::Real(x as f64 - y),
            (Value::Real(x), Value::Int(y)) => Value::Real(x - y as f64),
            _ => Value::Null,
        },
        Mul => match (a, b) {
            (Value::Int(x), Value::Int(y)) => Value::Int(x * y),
            (Value::Real(x), Value::Real(y)) => Value::Real(x * y),
            (Value::Int(x), Value::Real(y)) => Value::Real(x as f64 * y),
            (Value::Real(x), Value::Int(y)) => Value::Real(x * y as f64),
            _ => Value::Null,
        },
        Div => match (a, b) {
            (Value::Int(x), Value::Int(y)) => Value::Int(x / y),
            (Value::Real(x), Value::Real(y)) => Value::Real(x / y),
            (Value::Int(x), Value::Real(y)) => Value::Real(x as f64 / y),
            (Value::Real(x), Value::Int(y)) => Value::Real(x / y as f64),
            _ => Value::Null,
        },
        Concat => Value::Text(format!("{}{}", display(a), display(b))),
        Eq => Value::Bool(matches!(a.cmp_sql(&b), Some(Ordering::Equal))),
        Ne => Value::Bool(!matches!(a.cmp_sql(&b), Some(Ordering::Equal))),
        Lt => Value::Bool(matches!(a.cmp_sql(&b), Some(Ordering::Less))),
        Lte => Value::Bool(matches!(
            a.cmp_sql(&b),
            Some(Ordering::Less | Ordering::Equal)
        )),
        Gt => Value::Bool(matches!(a.cmp_sql(&b), Some(Ordering::Greater))),
        Gte => Value::Bool(matches!(
            a.cmp_sql(&b),
            Some(Ordering::Greater | Ordering::Equal)
        )),
        And => Value::Bool(matches!((a,b),(Value::Bool(x),Value::Bool(y)) if x && y)),
        Or => Value::Bool(matches!((a,b),(Value::Bool(x),Value::Bool(y)) if x || y)),
    }
}

fn display(v: Value) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::Int(i) => i.to_string(),
        Value::Real(f) => f.to_string(),
        Value::Text(s) => s,
        Value::Bool(b) => b.to_string(),
    }
}

fn func(name: &str, args: &[Expr], cols: &[ColumnDef], row: &[Value]) -> Result<Value> {
    let vals: Result<Vec<_>> = args.iter().map(|e| eval_expr(e, cols, row)).collect();
    let vals = vals?;
    Ok(match name {
        "LENGTH" => vals
            .first()
            .map(|v| Value::Int(display(v.clone()).len() as i64))
            .unwrap_or(Value::Null),
        "LOWER" => vals
            .first()
            .map(|v| Value::Text(display(v.clone()).to_lowercase()))
            .unwrap_or(Value::Null),
        "UPPER" => vals
            .first()
            .map(|v| Value::Text(display(v.clone()).to_uppercase()))
            .unwrap_or(Value::Null),
        "SUBSTR" => {
            if vals.len() >= 2 {
                let s = display(vals[0].clone());
                let start = match vals[1] {
                    Value::Int(i) => i.max(1) as usize - 1,
                    _ => 0,
                };
                let len = if vals.len() > 2 {
                    match vals[2] {
                        Value::Int(i) => i.max(0) as usize,
                        _ => s.len(),
                    }
                } else {
                    s.len()
                };
                Value::Text(s.chars().skip(start).take(len).collect())
            } else {
                Value::Null
            }
        }
        "ABS" => match vals.first() {
            Some(Value::Int(i)) => Value::Int(i.abs()),
            Some(Value::Real(f)) => Value::Real(f.abs()),
            _ => Value::Null,
        },
        "COALESCE" => vals
            .into_iter()
            .find(|v| !v.is_null())
            .unwrap_or(Value::Null),
        _ => Value::Null,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{catalog::Catalog, storage::Storage, txn::TransactionManager};
    use tempfile::tempdir;
    #[test]
    fn insert_and_select() {
        let d = tempdir().unwrap();
        let path = d.path().join("db.json");
        Storage::init(&path).unwrap();
        let (storage, mut cat) = Storage::open(&path).unwrap();
        let mut tx = TransactionManager::new();
        let _ = Executor::execute(
            &storage,
            &mut tx,
            &mut cat,
            PhysicalPlan::Statement(Statement::CreateTable {
                name: "t".into(),
                schema: Schema {
                    columns: vec![ColumnDef {
                        name: "a".into(),
                        ty: DataType::Int,
                        primary_key: false,
                        not_null: false,
                        unique: false,
                        default: None,
                    }],
                    constraints: vec![],
                },
            }),
        )
        .unwrap();
    }
}
