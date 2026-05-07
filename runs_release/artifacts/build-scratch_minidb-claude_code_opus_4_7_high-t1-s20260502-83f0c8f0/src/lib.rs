//! `minidb`: an embeddable SQL-subset relational database in pure Rust.
//!
//! The library is composed of:
//!
//! - [`lexer`] — turns source SQL into a stream of [`lexer::Token`]s.
//! - [`parser`] — recursive-descent parser producing an [`parser::ast::Statement`].
//! - [`plan`] — logical/physical query plans.
//! - [`exec`] — Volcano-style operator framework + the executor entry point
//!   [`exec::runner::execute_statement`].
//! - [`storage`] — pager (4 KiB pages), heap, B-tree (sorted leaf chain), and
//!   write-ahead log.
//! - [`catalog`] — in-memory catalog of tables and indexes.
//! - [`txn`] — transaction manager + buffered ops.
//! - [`engine`] — the [`engine::Engine`] glues all of the above together.
//! - [`cli`] — command line / REPL frontend.

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

pub use engine::Engine;
pub use error::{MiniDbError, Result};
pub use exec::runner::{execute_statement, ResultSet, StatementOutcome, WriteResult};
pub use exec::PhysicalOperator;

/// Open or create a database at `path`. Equivalent to [`Engine::open`].
pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Engine> {
    Engine::open(path)
}

/// Convenience: parse and execute a single SQL statement against `engine`.
pub fn run_sql(engine: &mut Engine, sql: &str) -> Result<StatementOutcome> {
    let stmt = parser::parse_statement(sql)?;
    execute_statement(&stmt, engine)
}
