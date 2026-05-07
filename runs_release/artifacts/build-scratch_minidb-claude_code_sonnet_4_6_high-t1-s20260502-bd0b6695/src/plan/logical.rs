use crate::parser::ast::{BinaryOp, Expr, OrderByItem, SelectItem, UnaryOp};
use crate::types::Value;

/// Logical plan node.
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
        projections: Vec<SelectItem>,
    },
    Join {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        condition: Option<Expr>,
    },
    Sort {
        input: Box<LogicalPlan>,
        order_by: Vec<OrderByItem>,
    },
    Limit {
        input: Box<LogicalPlan>,
        limit: usize,
        offset: usize,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        values: Vec<Vec<Expr>>,
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
    CreateTable {
        stmt: crate::parser::ast::CreateTableStmt,
    },
    DropTable {
        name: String,
        if_exists: bool,
    },
    CreateIndex {
        stmt: crate::parser::ast::CreateIndexStmt,
    },
    DropIndex {
        name: String,
        if_exists: bool,
    },
    Begin,
    Commit,
    Rollback,
    MetaCommand(crate::parser::ast::MetaCmd),
}
