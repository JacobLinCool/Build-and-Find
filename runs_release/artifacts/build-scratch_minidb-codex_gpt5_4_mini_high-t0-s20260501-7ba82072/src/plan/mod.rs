pub mod logical;
pub mod physical;

use crate::catalog::{IndexSchema, TableSchema};
use crate::error::{MiniDbError, Result};
use crate::exec::{
    evaluate, Filter, IndexPredicate, IndexScan, Limit as OpLimit, NestedLoopJoin, PlanNode,
    Project, QueryExecutor, SeqScan, Sort,
};
use crate::parser::ast::*;
use crate::txn::DatabaseState;
use crate::types::{Column, ResultSet, Row, SchemaColumn, TableConstraint, Value};

pub enum StatementOutcome {
    Query(ResultSet),
    Affected(usize),
    Message(String),
    None,
}

pub fn apply_statement_without_wal(
    state: &mut DatabaseState,
    statement: &Statement,
) -> Result<StatementOutcome> {
    match statement {
        Statement::CreateTable {
            name,
            columns,
            table_constraints,
        } => {
            let schema = TableSchema {
                name: name.clone(),
                columns: columns
                    .iter()
                    .map(|col| Column {
                        name: col.name.clone(),
                        col_type: col.col_type,
                        constraints: col.constraints.clone(),
                    })
                    .collect(),
                table_constraints: table_constraints.clone(),
            };
            state.create_table(schema)?;
            Ok(StatementOutcome::Affected(0))
        }
        Statement::DropTable { name } => {
            state.drop_table(name)?;
            Ok(StatementOutcome::Affected(0))
        }
        Statement::CreateIndex {
            name,
            table,
            column,
        } => {
            state.create_index(IndexSchema {
                name: name.clone(),
                table: table.clone(),
                column: column.clone(),
            })?;
            Ok(StatementOutcome::Affected(0))
        }
        Statement::DropIndex { name } => {
            state.drop_index(name)?;
            Ok(StatementOutcome::Affected(0))
        }
        Statement::Insert {
            table,
            columns,
            values,
        } => {
            let affected = insert_rows(state, table, columns.as_ref(), values)?;
            Ok(StatementOutcome::Affected(affected))
        }
        Statement::Select(select) => {
            let result = execute_select(state, select)?;
            Ok(StatementOutcome::Query(result))
        }
        Statement::Update {
            table,
            assignments,
            selection,
        } => {
            let affected = execute_update(state, table, assignments, selection.as_ref())?;
            Ok(StatementOutcome::Affected(affected))
        }
        Statement::Delete { table, selection } => {
            let affected = execute_delete(state, table, selection.as_ref())?;
            Ok(StatementOutcome::Affected(affected))
        }
        Statement::Meta(meta) => match meta {
            MetaCommand::Schema { table } => Ok(StatementOutcome::Message(
                state.catalog.schema_text(table.as_deref()),
            )),
            MetaCommand::Tables => Ok(StatementOutcome::Message(format!(
                "{}\n",
                state.catalog.list_tables().join("\n")
            ))),
            MetaCommand::Indexes => Ok(StatementOutcome::Message(format!(
                "{}\n",
                state.catalog.list_indexes().join("\n")
            ))),
            MetaCommand::Format { .. } => Ok(StatementOutcome::None),
            MetaCommand::Exit => Ok(StatementOutcome::None),
        },
        Statement::Begin | Statement::Commit | Statement::Rollback => Ok(StatementOutcome::None),
    }
}

pub fn execute_select(state: &mut DatabaseState, select: &SelectStmt) -> Result<ResultSet> {
    let mut plan = plan_select(state, select)?;
    let executor = QueryExecutor::default();
    executor.run(&mut plan, state)
}

fn plan_select(state: &DatabaseState, select: &SelectStmt) -> Result<PlanNode> {
    let _from_table = state.table(&select.from.name)?;
    let scan = if let Some((column, predicate)) =
        choose_index_predicate(state, &select.from, select.selection.as_ref())?
    {
        PlanNode::IndexScan(IndexScan::new(select.from.clone(), column, predicate))
    } else {
        PlanNode::SeqScan(SeqScan::new(select.from.clone()))
    };

    let mut root = scan;
    if let Some(pred) = &select.selection {
        root = PlanNode::Filter(Filter {
            child: Box::new(root),
            predicate: pred.clone(),
            schema: Vec::new(),
            rows: Vec::new(),
            pos: 0,
        });
    }

    for join in &select.joins {
        root = PlanNode::NestedLoopJoin(NestedLoopJoin {
            left: Box::new(root),
            right: Box::new(PlanNode::SeqScan(SeqScan::new(join.table.clone()))),
            on: join.on.clone(),
            schema: Vec::new(),
            rows: Vec::new(),
            pos: 0,
        });
    }

    root = PlanNode::Project(Project {
        child: Box::new(root),
        items: select.projection.clone(),
        schema: Vec::new(),
        rows: Vec::new(),
        pos: 0,
    });

    if let Some(order_by) = &select.order_by {
        root = PlanNode::Sort(Sort {
            child: Box::new(root),
            order_by: order_by.clone(),
            schema: Vec::new(),
            rows: Vec::new(),
            pos: 0,
        });
    }

    if let Some(limit) = &select.limit {
        root = PlanNode::Limit(OpLimit {
            child: Box::new(root),
            limit: limit.limit,
            offset: limit.offset,
            schema: Vec::new(),
            rows: Vec::new(),
            pos: 0,
        });
    }

    Ok(root)
}

fn choose_index_predicate(
    state: &DatabaseState,
    table: &TableRef,
    selection: Option<&Expr>,
) -> Result<Option<(String, IndexPredicate)>> {
    let selection = match selection {
        Some(selection) => selection,
        None => return Ok(None),
    };
    let conjuncts = flatten_and(selection);
    for expr in conjuncts {
        if let Some((column, predicate)) = extract_index_predicate(state, table, expr)? {
            return Ok(Some((column, predicate)));
        }
    }
    Ok(None)
}

fn extract_index_predicate(
    state: &DatabaseState,
    table: &TableRef,
    expr: &Expr,
) -> Result<Option<(String, IndexPredicate)>> {
    match expr {
        Expr::Binary { left, op, right } => {
            if let Some(column) = extract_column_name(table, left) {
                if let Some(index) = state.catalog.index_on_column(&table.name, &column) {
                    let value = match evaluate(right, None) {
                        Ok(value) => value,
                        Err(_) => return Ok(None),
                    };
                    let predicate = match op {
                        BinaryOp::Eq => Some(IndexPredicate::Eq(value)),
                        BinaryOp::Lt => Some(IndexPredicate::Lt(value)),
                        BinaryOp::Lte => Some(IndexPredicate::Lte(value)),
                        BinaryOp::Gt => Some(IndexPredicate::Gt(value)),
                        BinaryOp::Gte => Some(IndexPredicate::Gte(value)),
                        _ => None,
                    };
                    if let Some(predicate) = predicate {
                        return Ok(Some((index.column.clone(), predicate)));
                    }
                }
            }
            if let Some(column) = extract_column_name(table, right) {
                if let Some(index) = state.catalog.index_on_column(&table.name, &column) {
                    let value = match evaluate(left, None) {
                        Ok(value) => value,
                        Err(_) => return Ok(None),
                    };
                    let predicate = match op {
                        BinaryOp::Eq => Some(IndexPredicate::Eq(value)),
                        BinaryOp::Lt => Some(IndexPredicate::Gt(value)),
                        BinaryOp::Lte => Some(IndexPredicate::Gte(value)),
                        BinaryOp::Gt => Some(IndexPredicate::Lt(value)),
                        BinaryOp::Gte => Some(IndexPredicate::Lte(value)),
                        _ => None,
                    };
                    if let Some(predicate) = predicate {
                        return Ok(Some((index.column.clone(), predicate)));
                    }
                }
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn extract_column_name(table: &TableRef, expr: &Expr) -> Option<String> {
    match expr {
        Expr::ColumnRef { table: t, column } => {
            if t.as_deref()
                .map(|s| s == table.alias.as_deref().unwrap_or(&table.name))
                .unwrap_or(true)
                || t.as_deref() == Some(table.name.as_str())
            {
                Some(column.clone())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn flatten_and(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            let mut out = flatten_and(left);
            out.extend(flatten_and(right));
            out
        }
        _ => vec![expr],
    }
}

fn insert_rows(
    state: &mut DatabaseState,
    table_name: &str,
    columns: Option<&Vec<String>>,
    values: &[Expr],
) -> Result<usize> {
    let table = state.table(table_name)?.clone();
    let mut row = vec![Value::Null; table.schema.columns.len()];
    let target_columns: Vec<usize> = match columns {
        Some(cols) => cols
            .iter()
            .map(|name| {
                table
                    .schema
                    .columns
                    .iter()
                    .position(|col| col.name == *name)
                    .ok_or_else(|| MiniDbError::Runtime(format!("unknown column {name}")))
            })
            .collect::<Result<Vec<_>>>()?,
        None => (0..table.schema.columns.len()).collect(),
    };
    if target_columns.len() != values.len() {
        return Err(MiniDbError::Runtime(
            "column count does not match value count".into(),
        ));
    }
    for (target, expr) in target_columns.iter().zip(values.iter()) {
        row[*target] = evaluate(expr, None)?;
    }
    apply_defaults_and_validate(state, table_name, &mut row)?;
    state.insert_row(table_name, row)?;
    Ok(1)
}

fn apply_defaults_and_validate(
    state: &DatabaseState,
    table_name: &str,
    row: &mut Row,
) -> Result<()> {
    let table = state.table(table_name)?;
    for (idx, col) in table.schema.columns.iter().enumerate() {
        if matches!(row[idx], Value::Null) {
            if let Some(default) = &col.constraints.default {
                row[idx] = Value::from_literal(default);
            }
        }
        if col.constraints.not_null && row[idx].is_null() {
            return Err(MiniDbError::Runtime(format!(
                "column {} cannot be null",
                col.name
            )));
        }
    }
    for constraint in &table.schema.table_constraints {
        match constraint {
            TableConstraint::Check(expr) => {
                let ctx = crate::exec::RowContext {
                    columns: table
                        .schema
                        .columns
                        .iter()
                        .map(|col| SchemaColumn {
                            table: Some(table.schema.name.clone()),
                            name: col.name.clone(),
                            col_type: col.col_type,
                        })
                        .collect(),
                    values: row.clone(),
                };
                if !matches!(evaluate(expr, Some(&ctx))?, Value::Bool(true)) {
                    return Err(MiniDbError::Runtime("check constraint failed".into()));
                }
            }
        }
    }
    Ok(())
}

fn execute_update(
    state: &mut DatabaseState,
    table_name: &str,
    assignments: &[(String, Expr)],
    selection: Option<&Expr>,
) -> Result<usize> {
    let table = state.table(table_name)?.clone();
    let mut updated_rows = Vec::new();
    for (row_id, row) in table.heap.iter() {
        let ctx = crate::exec::RowContext {
            columns: table
                .schema
                .columns
                .iter()
                .map(|col| SchemaColumn {
                    table: Some(table.schema.name.clone()),
                    name: col.name.clone(),
                    col_type: col.col_type,
                })
                .collect(),
            values: row.clone(),
        };
        if let Some(selection) = selection {
            if !matches!(evaluate(selection, Some(&ctx))?, Value::Bool(true)) {
                continue;
            }
        }
        let mut new_row = row.clone();
        for (column, expr) in assignments {
            let idx = table
                .schema
                .columns
                .iter()
                .position(|col| col.name == *column)
                .ok_or_else(|| MiniDbError::Runtime(format!("unknown column {column}")))?;
            new_row[idx] = evaluate(expr, Some(&ctx))?;
        }
        apply_defaults_and_validate(state, table_name, &mut new_row)?;
        updated_rows.push((row_id, new_row));
    }
    state.update_rows(table_name, &updated_rows)?;
    Ok(updated_rows.len())
}

fn execute_delete(
    state: &mut DatabaseState,
    table_name: &str,
    selection: Option<&Expr>,
) -> Result<usize> {
    let table = state.table(table_name)?.clone();
    let mut deleted = Vec::new();
    for (row_id, row) in table.heap.iter() {
        let ctx = crate::exec::RowContext {
            columns: table
                .schema
                .columns
                .iter()
                .map(|col| SchemaColumn {
                    table: Some(table.schema.name.clone()),
                    name: col.name.clone(),
                    col_type: col.col_type,
                })
                .collect(),
            values: row.clone(),
        };
        if let Some(selection) = selection {
            if !matches!(evaluate(selection, Some(&ctx))?, Value::Bool(true)) {
                continue;
            }
        }
        deleted.push(row_id);
    }
    state.delete_rows(table_name, &deleted)?;
    Ok(deleted.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    #[test]
    fn executes_create_and_insert() {
        let mut state = DatabaseState::default();
        let statements =
            parse("CREATE TABLE t (id INT PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'a');")
                .unwrap();
        for stmt in statements {
            apply_statement_without_wal(&mut state, &stmt).unwrap();
        }
        assert_eq!(state.tables["t"].heap.len(), 1);
    }
}
