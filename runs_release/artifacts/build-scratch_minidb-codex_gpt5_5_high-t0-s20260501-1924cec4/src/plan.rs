use crate::catalog::normalize;
use crate::parser::ast::{BinaryOp, Expr, From, SelectStmt, Statement};
use crate::storage::DatabaseState;
use crate::types::Value;

#[derive(Debug, Clone)]
pub enum LogicalPlan {
    Statement(Statement),
}

#[derive(Debug, Clone)]
pub enum PhysicalPlan {
    SeqScan {
        table: String,
    },
    IndexScan {
        table: String,
        index: String,
        column: String,
        op: BinaryOp,
        value: Value,
    },
    Filter {
        input: Box<PhysicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<PhysicalPlan>,
        select: SelectStmt,
    },
    NestedLoopJoin {
        left: Box<PhysicalPlan>,
        right: Box<PhysicalPlan>,
        on: Expr,
    },
    Sort {
        input: Box<PhysicalPlan>,
        select: SelectStmt,
    },
    Limit {
        input: Box<PhysicalPlan>,
        limit: Option<usize>,
        offset: Option<usize>,
    },
}

pub struct Planner;

impl Planner {
    pub fn logical(stmt: Statement) -> LogicalPlan {
        LogicalPlan::Statement(stmt)
    }

    pub fn plan_select(state: &DatabaseState, select: &SelectStmt) -> PhysicalPlan {
        let mut root = match &select.from {
            From::Table { name } => scan_for_table(state, name, select.where_clause.as_ref()),
            From::Join { left, right, on } => PhysicalPlan::NestedLoopJoin {
                left: Box::new(scan_for_table(state, left, None)),
                right: Box::new(scan_for_table(state, right, None)),
                on: on.clone(),
            },
        };
        if let Some(predicate) = &select.where_clause {
            root = PhysicalPlan::Filter {
                input: Box::new(root),
                predicate: predicate.clone(),
            };
        }
        if !select.order_by.is_empty() {
            root = PhysicalPlan::Sort {
                input: Box::new(root),
                select: select.clone(),
            };
        }
        if select.limit.is_some() || select.offset.is_some() {
            root = PhysicalPlan::Limit {
                input: Box::new(root),
                limit: select.limit,
                offset: select.offset,
            };
        }
        PhysicalPlan::Project {
            input: Box::new(root),
            select: select.clone(),
        }
    }
}

fn scan_for_table(state: &DatabaseState, table: &str, predicate: Option<&Expr>) -> PhysicalPlan {
    if let Some((column, op, value)) = predicate.and_then(single_indexable_predicate) {
        if let Some(index) = state.catalog.index_on_column(table, &column) {
            return PhysicalPlan::IndexScan {
                table: table.to_string(),
                index: index.name.clone(),
                column,
                op,
                value,
            };
        }
    }
    PhysicalPlan::SeqScan {
        table: table.to_string(),
    }
}

fn single_indexable_predicate(expr: &Expr) -> Option<(String, BinaryOp, Value)> {
    match expr {
        Expr::Binary { left, op, right }
            if matches!(
                op,
                BinaryOp::Eq | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
            ) =>
        {
            match (&**left, &**right) {
                (Expr::Identifier(parts), Expr::Literal(value)) => {
                    parts.last().map(|c| (normalize(c), *op, value.clone()))
                }
                (Expr::Literal(value), Expr::Identifier(parts)) => {
                    let flipped = match op {
                        BinaryOp::Lt => BinaryOp::Gt,
                        BinaryOp::Le => BinaryOp::Ge,
                        BinaryOp::Gt => BinaryOp::Lt,
                        BinaryOp::Ge => BinaryOp::Le,
                        other => *other,
                    };
                    parts.last().map(|c| (normalize(c), flipped, value.clone()))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::parser::parse_sql;

    #[test]
    fn exposes_logical_wrapper() {
        let stmt = parse_sql("BEGIN;").unwrap().remove(0);
        assert!(matches!(Planner::logical(stmt), LogicalPlan::Statement(_)));
    }

    #[test]
    fn defaults_to_seq_scan_without_index() {
        let state = DatabaseState {
            catalog: Catalog::default(),
            ..DatabaseState::default()
        };
        let select = match parse_sql("SELECT * FROM t WHERE id = 1;")
            .unwrap()
            .remove(0)
        {
            Statement::Select(s) => s,
            _ => unreachable!(),
        };
        assert!(matches!(
            Planner::plan_select(&state, &select),
            PhysicalPlan::Project { .. }
        ));
    }
}
