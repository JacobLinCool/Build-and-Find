//! Volcano-style physical operators and the expression evaluator.
//!
//! The executor is split into:
//!   * `expr` — pure expression evaluation against a `Tuple`.
//!   * `operator` — the `PhysicalOperator` trait and its inputs/outputs.
//!   * `seq_scan`, `index_scan`, `filter`, `project`, `join`, `sort`, `limit`
//!     `insert`, `update`, `delete` — concrete operators.
//!
//! For simplicity, scan operators eagerly clone the rows they need; this keeps
//! lifetimes manageable in a single-writer system without sacrificing the
//! Volcano interface.

pub mod expr;
pub mod operator;

pub mod delete;
pub mod filter;
pub mod index_scan;
pub mod insert;
pub mod join;
pub mod limit;
pub mod project;
pub mod seq_scan;
pub mod sort;
pub mod update;

pub use operator::{ExecSchema, PhysicalOperator, Tuple};
