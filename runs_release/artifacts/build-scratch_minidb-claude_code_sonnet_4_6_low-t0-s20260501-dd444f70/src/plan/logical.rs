use crate::parser::ast::*;
use crate::types::Value;

#[derive(Debug, Clone)]
pub enum LogicalPlan {
    Scan {
        table: String,
        alias: Option<String>,
    },
    Filter {
        input: Box<LogicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<LogicalPlan>,
        projections: Vec<(Expr, Option<String>)>,
    },
    Join {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        condition: Expr,
    },
    Sort {
        input: Box<LogicalPlan>,
        keys: Vec<OrderByItem>,
    },
    Limit {
        input: Box<LogicalPlan>,
        limit: usize,
        offset: usize,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        rows: Vec<Vec<Expr>>,
    },
    Update {
        table: String,
        assignments: Vec<(String, Expr)>,
        predicate: Option<Expr>,
    },
    Delete {
        table: String,
        predicate: Option<Expr>,
    },
}
