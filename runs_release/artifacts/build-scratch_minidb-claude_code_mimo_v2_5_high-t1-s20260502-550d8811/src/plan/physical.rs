use crate::parser::ast::*;
use crate::plan::logical::ScanOperator;
use crate::types::Value;

#[derive(Debug, Clone)]
pub enum PhysicalPlan {
    SeqScan {
        table: String,
    },
    IndexScan {
        table: String,
        column: String,
        op: ScanOperator,
        value: Value,
    },
    Filter {
        predicate: Expr,
        input: Box<PhysicalPlan>,
    },
    Projection {
        items: Vec<SelectItem>,
        input: Box<PhysicalPlan>,
    },
    Sort {
        items: Vec<OrderByItem>,
        input: Box<PhysicalPlan>,
    },
    Limit {
        count: Expr,
        offset: Option<Expr>,
        input: Box<PhysicalPlan>,
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
