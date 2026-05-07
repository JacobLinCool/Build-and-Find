//! Physical plan: data describing the operator tree the executor will run.
//!
//! Operator construction itself lives in `exec`; the plan only carries the
//! parameters needed to materialise an operator. Keeping the data plain
//! makes it easy to inspect a plan, log it, or build alternative executors
//! later without churn.

use crate::parser::ast::{Expr, OrderItem, Projection};
use crate::types::Value;

#[derive(Debug, Clone)]
pub enum IndexBound {
    Equal(Value),
    Range {
        lo: Option<Value>,
        hi: Option<Value>,
        lo_inclusive: bool,
        hi_inclusive: bool,
    },
}

#[derive(Debug, Clone)]
pub enum PhysicalPlan {
    SeqScan {
        table: String,
        alias: Option<String>,
    },
    IndexScan {
        table: String,
        alias: Option<String>,
        column: String,
        index_name: String,
        bound: IndexBound,
    },
    Filter {
        input: Box<PhysicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<PhysicalPlan>,
        projections: Vec<Projection>,
    },
    NestedLoopJoin {
        left: Box<PhysicalPlan>,
        right: Box<PhysicalPlan>,
        on: Expr,
    },
    Sort {
        input: Box<PhysicalPlan>,
        items: Vec<OrderItem>,
    },
    Limit {
        input: Box<PhysicalPlan>,
        limit: i64,
        offset: i64,
    },
}

#[derive(Debug, Clone)]
pub enum WritePlan {
    Insert {
        table: String,
        rows: Vec<Vec<Expr>>,
        columns: Option<Vec<String>>,
    },
    Update {
        table: String,
        scan: PhysicalPlan,
        assignments: Vec<(String, Expr)>,
    },
    Delete {
        table: String,
        scan: PhysicalPlan,
    },
}
