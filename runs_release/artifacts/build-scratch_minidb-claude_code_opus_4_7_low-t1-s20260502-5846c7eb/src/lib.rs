//! minidb — an embeddable SQL-subset relational database.
//!
//! This crate is structured into modules that mirror a classic database
//! architecture:
//!
//! - [`lexer`] / [`parser`]: tokenisation and AST construction
//! - [`plan`]: logical and physical plans
//! - [`exec`]: Volcano-style operators and the top-level [`exec::Database`]
//! - [`storage`]: page manager, slotted heap, B-tree index, and WAL
//! - [`txn`]: transaction manager
//! - [`catalog`]: schema and index metadata
//! - [`types`]: SQL values and tri-valued logic
//! - [`cli`]: argument parsing and REPL
//! - [`error`]: crate-wide error type

pub mod catalog;
pub mod cli;
pub mod error;
pub mod exec;
pub mod lexer;
pub mod parser;
pub mod plan;
pub mod storage;
pub mod txn;
pub mod types;
