use crate::error::{MiniDbError, Result};
use crate::exec::{
    Delete as DeleteOp, Filter, IndexPredicate, IndexScan, Insert as InsertOp, Limit,
    NestedLoopJoin, PhysicalOperator, Project, SeqScan, Sort, Update as UpdateOp,
};
use crate::parser::ast::{BinaryOp, Delete, Expr, Insert, Literal, Select, TableRef, Update};
use crate::storage::DatabaseState;
use crate::types::{Schema, Value};

pub struct PhysicalPlan {
    pub root: Box<dyn PhysicalOperator>,
}

pub fn build_select_plan(state: &DatabaseState, select: &Select) -> Result<PhysicalPlan> {
    let left_schema = table_schema_for_ref(state, &select.from.left)?;
    let mut root: Box<dyn PhysicalOperator> = choose_scan(
        state,
        &select.from.left,
        select.selection.as_ref(),
        &left_schema,
    )?;
    if let Some(join) = &select.from.join {
        let right_schema = table_schema_for_ref(state, &join.right)?;
        let right = choose_scan(state, &join.right, None, &right_schema)?;
        root = Box::new(NestedLoopJoin::new(root, right, join.on.clone()));
    }
    if let Some(selection) = &select.selection {
        if select.from.join.is_some()
            || !predicate_used_for_index(state, &select.from.left, selection)
        {
            root = Box::new(Filter::new(root, selection.clone()));
        }
    }
    if !select.order_by.is_empty() {
        root = Box::new(Sort::new(root, select.order_by.clone()));
    }
    root = Box::new(Project::new(root, select.projection.clone()));
    if let Some(limit) = &select.limit {
        root = Box::new(Limit::new(root, limit.limit, limit.offset));
    }
    Ok(PhysicalPlan { root })
}

pub fn build_insert_plan(statement: Insert) -> PhysicalPlan {
    PhysicalPlan {
        root: Box::new(InsertOp::new(statement)),
    }
}

pub fn build_update_plan(statement: Update) -> PhysicalPlan {
    PhysicalPlan {
        root: Box::new(UpdateOp::new(statement)),
    }
}

pub fn build_delete_plan(statement: Delete) -> PhysicalPlan {
    PhysicalPlan {
        root: Box::new(DeleteOp::new(statement)),
    }
}

fn table_schema_for_ref(state: &DatabaseState, table_ref: &TableRef) -> Result<Schema> {
    let schema = state
        .catalog
        .table(&table_ref.name)
        .ok_or_else(|| MiniDbError::runtime(format!("unknown table {}", table_ref.name)))?;
    Ok(crate::exec::table_schema_for_alias(
        schema,
        table_ref.alias.as_deref(),
    ))
}

fn choose_scan(
    state: &DatabaseState,
    table_ref: &TableRef,
    predicate: Option<&Expr>,
    schema: &Schema,
) -> Result<Box<dyn PhysicalOperator>> {
    if let Some(predicate) = predicate {
        if let Some((index, index_predicate)) = find_index_predicate(state, table_ref, predicate)? {
            return Ok(Box::new(IndexScan::new(
                &table_ref.name,
                table_ref.alias.clone(),
                index,
                schema.clone(),
                index_predicate,
            )));
        }
    }
    Ok(Box::new(SeqScan::new(
        &table_ref.name,
        table_ref.alias.clone(),
        schema.clone(),
    )))
}

fn predicate_used_for_index(state: &DatabaseState, table_ref: &TableRef, predicate: &Expr) -> bool {
    find_index_predicate(state, table_ref, predicate)
        .ok()
        .flatten()
        .is_some()
}

fn find_index_predicate(
    state: &DatabaseState,
    table_ref: &TableRef,
    predicate: &Expr,
) -> Result<Option<(String, IndexPredicate)>> {
    let table = match state.tables.get(&table_ref.name) {
        Some(table) => table,
        None => return Ok(None),
    };
    let candidates = match predicate {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            if let Some(found) = find_index_predicate(state, table_ref, left)? {
                Some(found)
            } else {
                find_index_predicate(state, table_ref, right)?
            }
        }
        Expr::Binary { left, op, right } => match (&**left, &**right) {
            (Expr::ColumnRef { table, column }, Expr::Literal(lit))
                if column_matches(table, table_ref.alias.as_deref(), table_ref.name.as_str()) =>
            {
                Some(match op {
                    BinaryOp::Eq => (column.clone(), IndexPredicate::Eq(literal_to_value(lit))),
                    BinaryOp::Lt => (
                        column.clone(),
                        IndexPredicate::Range {
                            lower: None,
                            upper: Some((literal_to_value(lit), false)),
                        },
                    ),
                    BinaryOp::Lte => (
                        column.clone(),
                        IndexPredicate::Range {
                            lower: None,
                            upper: Some((literal_to_value(lit), true)),
                        },
                    ),
                    BinaryOp::Gt => (
                        column.clone(),
                        IndexPredicate::Range {
                            lower: Some((literal_to_value(lit), false)),
                            upper: None,
                        },
                    ),
                    BinaryOp::Gte => (
                        column.clone(),
                        IndexPredicate::Range {
                            lower: Some((literal_to_value(lit), true)),
                            upper: None,
                        },
                    ),
                    _ => return Ok(None),
                })
            }
            (Expr::Literal(lit), Expr::ColumnRef { table, column })
                if column_matches(table, table_ref.alias.as_deref(), table_ref.name.as_str()) =>
            {
                let flipped = flip_op(op);
                Some(match flipped {
                    BinaryOp::Eq => (column.clone(), IndexPredicate::Eq(literal_to_value(lit))),
                    BinaryOp::Lt => (
                        column.clone(),
                        IndexPredicate::Range {
                            lower: None,
                            upper: Some((literal_to_value(lit), false)),
                        },
                    ),
                    BinaryOp::Lte => (
                        column.clone(),
                        IndexPredicate::Range {
                            lower: None,
                            upper: Some((literal_to_value(lit), true)),
                        },
                    ),
                    BinaryOp::Gt => (
                        column.clone(),
                        IndexPredicate::Range {
                            lower: Some((literal_to_value(lit), false)),
                            upper: None,
                        },
                    ),
                    BinaryOp::Gte => (
                        column.clone(),
                        IndexPredicate::Range {
                            lower: Some((literal_to_value(lit), true)),
                            upper: None,
                        },
                    ),
                    _ => return Ok(None),
                })
            }
            _ => None,
        },
        _ => None,
    };
    if let Some((column, predicate)) = candidates {
        if let Some(index) = table
            .indexes
            .values()
            .find(|index| index.schema.column == column)
            .map(|index| index.schema.name.clone())
        {
            return Ok(Some((index, predicate)));
        }
    }
    Ok(None)
}

fn column_matches(column_table: &Option<String>, alias: Option<&str>, table_name: &str) -> bool {
    match column_table.as_deref() {
        Some(table) => Some(table) == alias || table == table_name,
        None => true,
    }
}

fn literal_to_value(literal: &Literal) -> Value {
    match literal {
        Literal::Null => Value::Null,
        Literal::Bool(v) => Value::Bool(*v),
        Literal::Int(v) => Value::Int(*v),
        Literal::Real(v) => Value::Real(*v),
        Literal::Text(v) => Value::Text(v.clone()),
    }
}

fn flip_op(op: &BinaryOp) -> BinaryOp {
    match op {
        BinaryOp::Lt => BinaryOp::Gt,
        BinaryOp::Lte => BinaryOp::Gte,
        BinaryOp::Gt => BinaryOp::Lt,
        BinaryOp::Gte => BinaryOp::Lte,
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::TableSchema;
    use crate::parser::ast::SelectItem;
    use crate::storage::{heap::HeapTable, DatabaseState};
    use crate::types::{Column, ColumnType};

    #[test]
    fn builds_select_plan() {
        let mut state = DatabaseState::default();
        let schema = TableSchema {
            name: "t".into(),
            columns: vec![Column::new("id", ColumnType::Int)],
            checks: vec![],
        };
        state.catalog.create_table(schema.clone());
        state.tables.insert("t".into(), HeapTable::new(schema));
        let select = crate::parser::ast::Select {
            projection: vec![SelectItem::Wildcard],
            from: crate::parser::ast::FromClause {
                left: crate::parser::ast::TableRef {
                    name: "t".into(),
                    alias: None,
                },
                join: None,
            },
            selection: None,
            order_by: vec![],
            limit: None,
        };
        let plan = build_select_plan(&state, &select).unwrap();
        assert_eq!(plan.root.schema().columns.len(), 1);
    }
}
