use crate::parser::ast::{Expr, OrderByItem};

#[derive(Debug, Clone)]
pub enum PhysicalPlan {
    SeqScan {
        table: String,
        alias: Option<String>,
    },
    IndexScan {
        table: String,
        alias: Option<String>,
        index_name: String,
        predicate: IndexPredicate,
    },
    Filter {
        input: Box<PhysicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<PhysicalPlan>,
        projections: Vec<(Expr, Option<String>)>,
    },
    NestedLoopJoin {
        left: Box<PhysicalPlan>,
        right: Box<PhysicalPlan>,
        condition: Expr,
    },
    Sort {
        input: Box<PhysicalPlan>,
        keys: Vec<OrderByItem>,
    },
    Limit {
        input: Box<PhysicalPlan>,
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
        input: Box<PhysicalPlan>,
        assignments: Vec<(String, Expr)>,
    },
    Delete {
        table: String,
        input: Box<PhysicalPlan>,
    },
}

#[derive(Debug, Clone)]
pub enum IndexPredicate {
    Eq(Expr),
    Range {
        low: Option<Expr>,
        low_inclusive: bool,
        high: Option<Expr>,
        high_inclusive: bool,
    },
}
