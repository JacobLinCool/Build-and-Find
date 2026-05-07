pub mod format;
pub mod repl;

use crate::cli::format::OutputFormat;
use crate::cli::repl::Repl;
use crate::error::MiniDbError;
use crate::exec::Database;
use crate::parser::ast::MetaCmd;
use crate::plan::Planner;
use std::path::{Path, PathBuf};

/// Execute a single SQL string, returning exit code.
pub fn exec_sql(
    db: &mut Database,
    sql: &str,
    fmt: &OutputFormat,
    quiet: bool,
    continue_on_error: bool,
) -> i32 {
    let stmts = match crate::parser::parse(sql) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {}", e);
            return 1;
        }
    };

    let mut last_rc = 0i32;
    for stmt in stmts {
        if let crate::parser::ast::Statement::MetaCommand(MetaCmd::Exit) = &stmt {
            return 0;
        }
        if let crate::parser::ast::Statement::MetaCommand(MetaCmd::Format(f)) = &stmt {
            // ignore in batch mode
            continue;
        }

        let planner = Planner::new(&db.catalog);
        let plan = match planner.plan_statement(stmt) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Error: {}", e);
                last_rc = 1;
                if !continue_on_error {
                    return last_rc;
                }
                continue;
            }
        };

        match db.execute_plan(plan) {
            Err(e) => {
                eprintln!("Error: {}", e);
                last_rc = 1;
                if !continue_on_error {
                    return last_rc;
                }
            }
            Ok(result) => {
                if let Some(msg) = &result.message {
                    if msg == "exit" {
                        return 0;
                    }
                    if !quiet {
                        eprintln!("ok {}", msg);
                    }
                } else if let Some(n) = result.affected_rows {
                    if !quiet {
                        let word = if n == 1 { "row" } else { "rows" };
                        eprintln!("ok {} {} affected", n, word);
                    }
                } else if !result.rows.is_empty() || !result.schema.columns.is_empty() {
                    print!("{}", format::format_result(&result, fmt));
                    if !quiet {
                        let n = result.rows.len();
                        let word = if n == 1 { "row" } else { "rows" };
                        eprintln!("ok {} {}", n, word);
                    }
                }
            }
        }
    }
    last_rc
}

/// Execute a SQL file, stopping on error (unless continue_on_error is set).
pub fn exec_file(
    db: &mut Database,
    path: &Path,
    fmt: &OutputFormat,
    quiet: bool,
    continue_on_error: bool,
) -> i32 {
    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file {}: {}", path.display(), e);
            return 3;
        }
    };
    exec_sql(db, &content, fmt, quiet, continue_on_error)
}
