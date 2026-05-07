//! Logical plan tree. A simple intermediate representation between the AST
//! and the physical plan; useful as a stable API even though the planner
//! currently translates statements directly into physical plans.

use crate::parser::ast::{Expr, OrderItem, Projection};

#[derive(Debug, Clone)]
pub enum LogicalPlan {
    /// A scan over a base table (no predicate yet).
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
        projections: Vec<Projection>,
    },
    Join {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        on: Expr,
    },
    Sort {
        input: Box<LogicalPlan>,
        items: Vec<OrderItem>,
    },
    Limit {
        input: Box<LogicalPlan>,
        limit: i64,
        offset: i64,
    },
}
