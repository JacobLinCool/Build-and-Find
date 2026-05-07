use crate::parser::ast::*;
use crate::types::Value;

#[derive(Debug, Clone)]
pub enum LogicalPlan {
    TableScan {
        table: String,
        alias: Option<String>,
    },
    IndexScan {
        table: String,
        column: String,
        op: ScanOperator,
        value: Value,
    },
    Filter {
        predicate: Expr,
        input: Box<LogicalPlan>,
    },
    Projection {
        items: Vec<SelectItem>,
        input: Box<LogicalPlan>,
    },
    Sort {
        items: Vec<OrderByItem>,
        input: Box<LogicalPlan>,
    },
    Limit {
        count: Expr,
        offset: Option<Expr>,
        input: Box<LogicalPlan>,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        rows: Vec<Vec<Expr>>,
    },
    Update {
        table: String,
        assignments: Vec<Assignment>,
        where_clause: Option<Expr>,
    },
    Delete {
        table: String,
        where_clause: Option<Expr>,
    },
}

#[derive(Debug, Clone)]
pub enum ScanOperator {
    Eq,
    Gt,
    Ge,
    Lt,
    Le,
}
