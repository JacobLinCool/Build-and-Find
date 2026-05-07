//! Logical plan: a relational algebra tree with no physical operator choices.

use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;

#[derive(Clone, Debug)]
pub enum LogicalPlan {
    Scan {
        table: String,
        alias: Option<String>,
    },
    Filter {
        input: Box<LogicalPlan>,
        pred: Expr,
    },
    Project {
        input: Box<LogicalPlan>,
        exprs: Vec<Projection>,
    },
    Join {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        on: Expr,
    },
    Sort {
        input: Box<LogicalPlan>,
        keys: Vec<OrderKey>,
    },
    Limit {
        input: Box<LogicalPlan>,
        limit: i64,
        offset: i64,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        rows: Vec<Vec<Expr>>,
    },
    Update {
        table: String,
        assignments: Vec<(String, Expr)>,
        pred: Option<Expr>,
    },
    Delete {
        table: String,
        pred: Option<Expr>,
    },
    CreateTable(CreateTable),
    DropTable(DropTable),
    CreateIndex(CreateIndex),
    DropIndex(DropIndex),
    Begin,
    Commit,
    Rollback,
    Meta(MetaCommand),
}

pub fn build_logical(stmt: Statement) -> Result<LogicalPlan> {
    Ok(match stmt {
        Statement::Select(s) => build_select(s)?,
        Statement::Insert(i) => LogicalPlan::Insert {
            table: i.table,
            columns: i.columns,
            rows: i.rows,
        },
        Statement::Update(u) => LogicalPlan::Update {
            table: u.table,
            assignments: u.assignments,
            pred: u.where_clause,
        },
        Statement::Delete(d) => LogicalPlan::Delete {
            table: d.table,
            pred: d.where_clause,
        },
        Statement::CreateTable(c) => LogicalPlan::CreateTable(c),
        Statement::DropTable(d) => LogicalPlan::DropTable(d),
        Statement::CreateIndex(c) => LogicalPlan::CreateIndex(c),
        Statement::DropIndex(d) => LogicalPlan::DropIndex(d),
        Statement::Begin => LogicalPlan::Begin,
        Statement::Commit => LogicalPlan::Commit,
        Statement::Rollback => LogicalPlan::Rollback,
        Statement::Meta(m) => LogicalPlan::Meta(m),
    })
}

fn build_select(s: Select) -> Result<LogicalPlan> {
    let mut plan = match s.from {
        FromClause::Table { name, alias } => LogicalPlan::Scan { table: name, alias },
        FromClause::Join(j) => LogicalPlan::Join {
            left: Box::new(LogicalPlan::Scan {
                table: j.left_table,
                alias: j.left_alias,
            }),
            right: Box::new(LogicalPlan::Scan {
                table: j.right_table,
                alias: j.right_alias,
            }),
            on: j.on,
        },
    };
    if let Some(pred) = s.where_clause {
        plan = LogicalPlan::Filter {
            input: Box::new(plan),
            pred,
        };
    }
    plan = LogicalPlan::Project {
        input: Box::new(plan),
        exprs: s.projections,
    };
    if let Some(keys) = s.order_by {
        plan = LogicalPlan::Sort {
            input: Box::new(plan),
            keys,
        };
    }
    if let Some(lim) = s.limit {
        if lim.limit < 0 {
            return Err(MiniDbError::ty("LIMIT must be non-negative"));
        }
        plan = LogicalPlan::Limit {
            input: Box::new(plan),
            limit: lim.limit,
            offset: lim.offset.unwrap_or(0),
        };
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn select_builds_project_filter_scan() {
        let stmts = parse("SELECT a FROM t WHERE a > 1 LIMIT 10").unwrap();
        let plan = build_logical(stmts[0].clone()).unwrap();
        match plan {
            LogicalPlan::Limit { input, limit, .. } => {
                assert_eq!(limit, 10);
                match *input {
                    LogicalPlan::Project { .. } => {}
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
    }
}
