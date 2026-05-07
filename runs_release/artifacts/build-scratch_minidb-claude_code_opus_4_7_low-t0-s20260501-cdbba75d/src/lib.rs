//! `minidb` — a single-binary, on-disk relational database with an embeddable
//! library API.
//!
//! Module map:
//! - `lexer`, `parser` — SQL front-end (no storage access).
//! - `plan` — logical and physical plans.
//! - `exec` — Volcano-style physical operators and expression evaluation.
//! - `storage` — page manager, WAL, B-tree index.
//! - `txn` — single-writer transaction manager.
//! - `catalog` — schema and index metadata + in-memory table data.
//! - `engine` — top-level orchestration (open/recover/execute).
//! - `cli` — argument parsing, REPL, output formatting.

pub mod catalog;
pub mod cli;
pub mod engine;
pub mod error;
pub mod exec;
pub mod lexer;
pub mod parser;
pub mod plan;
pub mod storage;
pub mod txn;
pub mod types;

pub use engine::{Database, StmtResult};
pub use error::{MiniDbError, Result};
