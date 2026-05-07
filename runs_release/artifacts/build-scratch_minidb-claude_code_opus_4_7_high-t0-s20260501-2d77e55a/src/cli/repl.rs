//! REPL and batch script driver.

use std::io::{BufRead, Write};

use crate::engine::{AffectedVerb, Engine, ExecResult};
use crate::error::{MiniDbError, Result};
use crate::lexer::{Lexer, TokenKind};
use crate::parser::ast;
use crate::parser::parse;

use super::args::{CliArgs, RunMode, HELP_TEXT};
use super::format::Formatter;
use super::OutputFormat;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Top-level entry point used by `main.rs`.
pub fn run(args: CliArgs) -> Result<i32> {
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let stdin = std::io::stdin();
    run_with_io(
        args,
        &mut stdin.lock(),
        &mut stdout.lock(),
        &mut stderr.lock(),
    )
}

pub fn run_with_io<R: BufRead, O: Write, E: Write>(
    args: CliArgs,
    input: &mut R,
    out: &mut O,
    err: &mut E,
) -> Result<i32> {
    if args.print_help {
        write!(out, "{}", HELP_TEXT)?;
        return Ok(0);
    }
    if args.print_version {
        writeln!(out, "minidb {}", VERSION)?;
        return Ok(0);
    }
    if args.init {
        Engine::create_empty(&args.db)?;
        writeln!(err, "initialized empty database at {:?}", args.db)?;
        return Ok(0);
    }

    let mut engine = Engine::open(&args.db)?;
    let mut state = SessionState::new(args.format, args.quiet);

    match &args.mode {
        Some(RunMode::Eval(sql)) => run_eval(&mut engine, &mut state, sql, out, err),
        Some(RunMode::File(path)) => {
            let bytes = std::fs::read_to_string(path)
                .map_err(|e| MiniDbError::cli(format!("could not read {:?}: {}", path, e)))?;
            run_script(
                &mut engine,
                &mut state,
                &bytes,
                args.continue_on_error,
                out,
                err,
            )
        }
        Some(RunMode::Repl) => run_repl(&mut engine, &mut state, input, out, err),
        None => {
            // Decide based on stdin TTY.
            if atty::is(atty::Stream::Stdin) {
                run_repl(&mut engine, &mut state, input, out, err)
            } else {
                let mut buf = String::new();
                input.read_to_string(&mut buf)?;
                run_script(
                    &mut engine,
                    &mut state,
                    &buf,
                    args.continue_on_error,
                    out,
                    err,
                )
            }
        }
    }
}

struct SessionState {
    format: OutputFormat,
    quiet: bool,
}

impl SessionState {
    fn new(format: OutputFormat, quiet: bool) -> Self {
        Self { format, quiet }
    }
}

fn run_eval<O: Write, E: Write>(
    engine: &mut Engine,
    state: &mut SessionState,
    sql: &str,
    out: &mut O,
    err: &mut E,
) -> Result<i32> {
    match execute_script_text(engine, state, sql, false, out, err) {
        Ok(()) => Ok(0),
        Err(e) => {
            print_error(err, &e)?;
            Ok(e.exit_code())
        }
    }
}

fn run_script<O: Write, E: Write>(
    engine: &mut Engine,
    state: &mut SessionState,
    sql: &str,
    cont_on_err: bool,
    out: &mut O,
    err: &mut E,
) -> Result<i32> {
    match execute_script_text(engine, state, sql, cont_on_err, out, err) {
        Ok(()) => Ok(0),
        Err(e) => {
            print_error(err, &e)?;
            Ok(e.exit_code())
        }
    }
}

fn run_repl<R: BufRead, O: Write, E: Write>(
    engine: &mut Engine,
    state: &mut SessionState,
    input: &mut R,
    out: &mut O,
    err: &mut E,
) -> Result<i32> {
    writeln!(
        err,
        "minidb {} -- type SQL terminated by ';'. Ctrl-D to exit.",
        VERSION
    )?;
    let mut buf = String::new();
    loop {
        if buf.is_empty() {
            write!(err, "minidb> ")?;
            err.flush()?;
        } else {
            write!(err, "    ... ")?;
            err.flush()?;
        }
        let mut line = String::new();
        let n = input.read_line(&mut line)?;
        if n == 0 {
            // EOF
            if !buf.trim().is_empty() {
                let _ = execute_script_text(engine, state, &buf, true, out, err);
            }
            return Ok(0);
        }
        buf.push_str(&line);
        // Detect a "complete" command: either a `;` token (outside strings) or a leading meta `.`.
        if is_complete(&buf) {
            let to_run = std::mem::take(&mut buf);
            let trimmed = to_run.trim();
            if trimmed.is_empty() {
                continue;
            }
            match execute_script_text(engine, state, trimmed, true, out, err) {
                Ok(()) => {}
                Err(e) => {
                    print_error(err, &e)?;
                    if let Some(_exit) = check_exit(trimmed) {
                        return Ok(0);
                    }
                }
            }
            // Special handling: .exit
            if let Some(()) = check_exit(trimmed) {
                return Ok(0);
            }
        }
    }
}

fn check_exit(s: &str) -> Option<()> {
    let t = s.trim().trim_end_matches(';').trim();
    if t.eq_ignore_ascii_case(".exit") {
        Some(())
    } else {
        None
    }
}

fn is_complete(buf: &str) -> bool {
    // A meta-command (starting with .) is complete on a newline.
    let trimmed = buf.trim_start();
    if trimmed.starts_with('.') {
        return buf.contains('\n');
    }
    // Otherwise, check whether the lexer can tokenize the buffer through to EOF
    // and the most recent non-Eof token is a Semicolon.
    let toks = match Lexer::new(buf).tokenize() {
        Ok(t) => t,
        Err(_) => return false, // wait for more input
    };
    // Find last non-Eof token.
    let last_real = toks
        .iter()
        .rev()
        .find(|t| !matches!(t.kind, TokenKind::Eof));
    matches!(last_real.map(|t| &t.kind), Some(TokenKind::Semicolon))
}

fn execute_script_text<O: Write, E: Write>(
    engine: &mut Engine,
    state: &mut SessionState,
    sql: &str,
    cont_on_err: bool,
    out: &mut O,
    err: &mut E,
) -> Result<()> {
    let stmts = parse(sql)?;
    for stmt in stmts {
        // Capture .format meta locally so the CLI can adjust output format.
        if let ast::Statement::Meta(m) = &stmt {
            if m.name == "format" {
                if let Some(v) = &m.arg {
                    state.format = OutputFormat::parse(v)?;
                    if !state.quiet {
                        writeln!(err, "ok format {}", v)?;
                    }
                    continue;
                }
            }
            if m.name == "exit" {
                return Ok(());
            }
        }
        match engine.execute_one(stmt) {
            Ok(result) => render_result(state, result, out, err)?,
            Err(e) => {
                if cont_on_err {
                    print_error(err, &e)?;
                } else {
                    return Err(e);
                }
            }
        }
    }
    Ok(())
}

fn render_result<O: Write, E: Write>(
    state: &SessionState,
    result: ExecResult,
    out: &mut O,
    err: &mut E,
) -> Result<()> {
    match result {
        ExecResult::Query { schema, rows } => {
            Formatter::new(state.format).write_query(out, &schema, &rows)?;
            if !state.quiet {
                writeln!(err, "ok {} rows", rows.len())?;
            }
        }
        ExecResult::Affected { verb, count } => {
            if !state.quiet {
                let label = match verb {
                    AffectedVerb::Inserted => "inserted",
                    AffectedVerb::Updated => "updated",
                    AffectedVerb::Deleted => "deleted",
                };
                let s = if count == 1 { "row" } else { "rows" };
                writeln!(err, "ok {} {} {}", count, s, label)?;
            }
        }
        ExecResult::Ok => {
            if !state.quiet {
                writeln!(err, "ok")?;
            }
        }
        ExecResult::Begin => {
            if !state.quiet {
                writeln!(err, "ok begin")?;
            }
        }
        ExecResult::Commit => {
            if !state.quiet {
                writeln!(err, "ok commit")?;
            }
        }
        ExecResult::Rollback => {
            if !state.quiet {
                writeln!(err, "ok rollback")?;
            }
        }
        ExecResult::TableList(names) => {
            for n in &names {
                writeln!(out, "{}", n)?;
            }
            if !state.quiet {
                writeln!(err, "ok {} tables", names.len())?;
            }
        }
        ExecResult::SchemaList(items) => {
            for (_, descr) in &items {
                writeln!(out, "{};", descr)?;
            }
            if !state.quiet {
                writeln!(err, "ok {} tables", items.len())?;
            }
        }
        ExecResult::IndexList(items) => {
            for (n, t, c) in &items {
                writeln!(out, "{} ON {}({})", n, t, c)?;
            }
            if !state.quiet {
                writeln!(err, "ok {} indexes", items.len())?;
            }
        }
        ExecResult::Meta(_) => {
            // .format etc. handled at call site.
        }
        ExecResult::Exit => {}
    }
    Ok(())
}

fn print_error<W: Write>(err: &mut W, e: &MiniDbError) -> Result<()> {
    writeln!(err, "error: {}", e)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_complete_statements() {
        assert!(is_complete("SELECT 1;"));
        assert!(!is_complete("SELECT 1"));
        assert!(is_complete(".tables\n"));
        assert!(!is_complete(".tables"));
    }
}
