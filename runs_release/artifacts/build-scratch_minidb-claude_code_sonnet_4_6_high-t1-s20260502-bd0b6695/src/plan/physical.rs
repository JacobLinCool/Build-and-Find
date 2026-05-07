use crate::parser::ast::{Expr, OrderByItem, SelectItem};
use crate::types::Value;

/// Physical plan node -- maps closely to physical operator types.
#[derive(Debug, Clone)]
pub enum PhysicalPlan {
    SeqScan {
        table: String,
        alias: Option<String>,
    },
    IndexScan {
        table: String,
        index: String,
        column: String,
        op: ScanOp,
        value: Value,
        alias: Option<String>,
    },
    Filter {
        input: Box<PhysicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<PhysicalPlan>,
        projections: Vec<SelectItem>,
    },
    NestedLoopJoin {
        left: Box<PhysicalPlan>,
        right: Box<PhysicalPlan>,
        condition: Option<Expr>,
    },
    Sort {
        input: Box<PhysicalPlan>,
        order_by: Vec<OrderByItem>,
    },
    Limit {
        input: Box<PhysicalPlan>,
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

#[derive(Debug, Clone, PartialEq)]
pub enum ScanOp {
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
}
