//! Volcano-style physical operator framework and the operator implementations.
//!
//! Each operator implements `PhysicalOperator` and exposes a `next()` method
//! that yields one row at a time. Operators compose into trees built by the
//! planner; the runner walks the tree and produces a `ResultSet`.

pub mod expr;
pub mod operators;
pub mod runner;

pub use operators::{
    Delete as DeleteOp, ExecCtx, ExecRow, Filter, IndexScan, Insert as InsertOp, Limit,
    NestedLoopJoin, PhysicalOperator, Project, SeqScan, Sort, Update as UpdateOp,
};
pub use runner::{ResultSet, WriteResult};
