//! minidb — a minimal SQL-subset relational database.
//!
//! The crate is organised into layered subsystems:
//!
//! * [`error`] — single crate-wide error enum.
//! * [`types`] — runtime values, column types, schemas, tri-valued logic.
//! * [`storage`] — pages, heap, B-tree index, write-ahead log.
//! * [`txn`] — transaction manager.
//! * [`catalog`] — system catalog (tables, indexes).
//! * [`lexer`] and [`parser`] — front-end producing an AST.
//! * [`plan`] — logical + physical plan nodes and the planner.
//! * [`exec`] — Volcano-style physical operators.
//! * [`format`] — result-set formatters (table / csv / json / ascii-art).
//! * [`cli`] — argument parsing, batch, and REPL.

pub mod catalog;
pub mod cli;
pub mod error;
pub mod exec;
pub mod format;
pub mod lexer;
pub mod parser;
pub mod plan;
pub mod storage;
pub mod txn;
pub mod types;

pub use error::{MiniDbError, Result};
