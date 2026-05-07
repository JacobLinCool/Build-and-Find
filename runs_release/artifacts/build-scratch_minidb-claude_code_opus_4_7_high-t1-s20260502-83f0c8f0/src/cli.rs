//! Command line interface and REPL.
//!
//! Exit codes (per spec):
//! - 0: success
//! - 1: SQL / runtime error (parse, type, runtime, constraint, transaction, ...)
//! - 2: CLI usage error (bad arguments, unknown flag, ...)
//! - 3: storage / IO error (pager, WAL, file open failures)
//!
//! Top-level options:
//! - `--repl` (default when stdin is a TTY)
//! - `--eval "SQL"` execute a string and exit
//! - `--file PATH` execute a file and exit
//! - `--init PATH` execute `PATH` then enter REPL or stdin
//! - `--continue-on-error` keep going after a SQL error in batch/file mode
//! - `--format table|csv|json` output format for SELECT
//! - `--quiet` suppress the per-statement summary on stderr
//! - `--version` / `-V` print version and exit
//! - `--help` / `-h` print usage and exit
//!
//! Meta-commands (REPL or stdin):
//! - `.tables` list table names
//! - `.indexes` list indexes
//! - `.schema [name]` show CREATE-style schema
//! - `.format table|csv|json` change current output format
//! - `.exit` / `.quit` end the session

use crate::engine::Engine;
use crate::error::{MiniDbError, Result};
use crate::exec::runner::{execute_statement, ResultSet, StatementOutcome, WriteResult};
use crate::parser::{self, ast::Statement};
use crate::types::Value;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

impl OutputFormat {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "table" => Ok(OutputFormat::Table),
            "csv" => Ok(OutputFormat::Csv),
            "json" => Ok(OutputFormat::Json),
            other => Err(MiniDbError::cli(format!("unknown format: {}", other))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Args {
    pub db_path: Option<PathBuf>,
    pub mode: Mode,
    pub format: OutputFormat,
    pub quiet: bool,
    pub continue_on_error: bool,
    pub init: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum Mode {
    /// Read SQL interactively from a TTY.
    Repl,
    /// Read SQL from stdin (non-tty) and exit.
    Stdin,
    /// Execute the given inline SQL and exit.
    Eval(String),
    /// Execute the given file and exit.
    File(PathBuf),
    /// Print version and exit.
    Version,
    /// Print help and exit.
    Help,
}

const HELP: &str = "minidb — embeddable SQL-subset relational database

USAGE:
    minidb [DB_PATH] [OPTIONS]

OPTIONS:
    --repl              Force REPL mode (default when stdin is a TTY).
    --eval SQL          Execute the given SQL string and exit.
    --file PATH         Execute SQL from file PATH and exit.
    --init PATH         Run SQL from PATH before reading further input.
    --format FMT        Output format for SELECT: table|csv|json. Default: table.
    --quiet             Suppress per-statement summary on stderr.
    --continue-on-error Keep going after a SQL error in batch/file modes.
    -V, --version       Print version and exit.
    -h, --help          Show this help and exit.

POSITIONAL:
    DB_PATH             Path to the database file. Created if it doesn't exist.

EXIT CODES:
    0  success
    1  SQL / runtime error
    2  CLI usage error
    3  storage / IO error

EXAMPLES:
    minidb mydb.db --repl
    minidb mydb.db --eval \"CREATE TABLE t(id INT PRIMARY KEY)\"
    minidb mydb.db --file schema.sql
    cat queries.sql | minidb mydb.db
";

/// Entry point used by `main`. Returns the appropriate process exit code.
pub fn run() -> i32 {
    let argv: Vec<String> = std::env::args().collect();
    match parse_args(&argv) {
        Err(MiniDbError::Cli(m)) => {
            let _ = writeln!(io::stderr(), "minidb: {}", m);
            let _ = writeln!(io::stderr(), "Try `minidb --help` for usage.");
            2
        }
        Err(e) => {
            let _ = writeln!(io::stderr(), "minidb: {}", e);
            2
        }
        Ok(args) => execute(args),
    }
}

pub fn execute(args: Args) -> i32 {
    if let Mode::Help = &args.mode {
        let _ = io::stdout().write_all(HELP.as_bytes());
        return 0;
    }
    if let Mode::Version = &args.mode {
        let _ = writeln!(io::stdout(), "minidb {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }

    let db_path = match args.db_path.clone() {
        Some(p) => p,
        None => match &args.mode {
            Mode::Repl | Mode::Stdin | Mode::Eval(_) | Mode::File(_) => default_db_path(),
            _ => default_db_path(),
        },
    };

    let mut engine = match Engine::open(&db_path) {
        Ok(e) => e,
        Err(e) => {
            let _ = writeln!(io::stderr(), "minidb: failed to open {:?}: {}", db_path, e);
            return io_or_storage_exit_code(&e);
        }
    };

    let mut session = Session {
        engine: &mut engine,
        format: args.format,
        quiet: args.quiet,
        continue_on_error: args.continue_on_error,
    };

    if let Some(init) = &args.init {
        let sql = match std::fs::read_to_string(init) {
            Ok(s) => s,
            Err(e) => {
                let _ = writeln!(io::stderr(), "minidb: cannot read {:?}: {}", init, e);
                return 3;
            }
        };
        if let Some(code) = session.run_program(&sql) {
            return code;
        }
    }

    match args.mode {
        Mode::Eval(sql) => session.run_program(&sql).unwrap_or(0),
        Mode::File(path) => {
            let sql = match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) => {
                    let _ = writeln!(io::stderr(), "minidb: cannot read {:?}: {}", path, e);
                    return 3;
                }
            };
            session.run_program(&sql).unwrap_or(0)
        }
        Mode::Repl => session.run_repl(true),
        Mode::Stdin => session.run_repl(false),
        Mode::Help | Mode::Version => 0,
    }
}

fn io_or_storage_exit_code(e: &MiniDbError) -> i32 {
    match e {
        MiniDbError::Io(_) | MiniDbError::Storage(_) | MiniDbError::Wal(_) => 3,
        _ => 1,
    }
}

fn default_db_path() -> PathBuf {
    PathBuf::from(":memory:.minidb")
}

pub struct Session<'a> {
    pub engine: &'a mut Engine,
    pub format: OutputFormat,
    pub quiet: bool,
    pub continue_on_error: bool,
}

impl<'a> Session<'a> {
    /// Execute a sequence of semicolon-separated statements (and meta-commands).
    /// Returns `Some(exit_code)` to short-circuit; `None` means continue.
    pub fn run_program(&mut self, source: &str) -> Option<i32> {
        for chunk in split_top_level(source) {
            let trimmed = chunk.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(rc) = self.run_one(trimmed) {
                return Some(rc);
            }
        }
        None
    }

    /// Execute a single statement (or meta-command) from a trimmed string.
    /// Returns `Some(exit_code)` to short-circuit; `None` means continue.
    pub fn run_one(&mut self, text: &str) -> Option<i32> {
        let stripped = text.trim_end_matches(';').trim();
        if stripped.is_empty() {
            return None;
        }
        if stripped.starts_with('.') {
            return self.run_meta(stripped);
        }
        match parser::parse_statement(stripped) {
            Ok(stmt) => self.execute_one(&stmt),
            Err(e) => {
                self.report_error(&e);
                if self.continue_on_error {
                    None
                } else {
                    Some(error_exit_code(&e))
                }
            }
        }
    }

    fn execute_one(&mut self, stmt: &Statement) -> Option<i32> {
        match execute_statement(stmt, self.engine) {
            Ok(StatementOutcome::Select(rs)) => {
                if let Err(e) = self.print_result_set(&rs) {
                    self.report_error(&e);
                    return Some(io_or_storage_exit_code(&e));
                }
                None
            }
            Ok(StatementOutcome::Write(wr)) => {
                self.print_write_result(&wr);
                None
            }
            Err(e) => {
                self.report_error(&e);
                if self.continue_on_error {
                    None
                } else {
                    Some(error_exit_code(&e))
                }
            }
        }
    }

    fn run_meta(&mut self, line: &str) -> Option<i32> {
        let mut parts = line.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("").trim();
        match cmd.to_ascii_lowercase().as_str() {
            ".exit" | ".quit" => Some(0),
            ".tables" => {
                let mut names: Vec<String> = self
                    .engine
                    .catalog()
                    .tables()
                    .map(|t| t.name.clone())
                    .collect();
                names.sort();
                let mut out = io::stdout().lock();
                for n in names {
                    let _ = writeln!(out, "{}", n);
                }
                None
            }
            ".indexes" => {
                let mut idx: Vec<(String, String, String)> = self
                    .engine
                    .catalog()
                    .indexes()
                    .map(|i| (i.name.clone(), i.table.clone(), i.column.clone()))
                    .collect();
                idx.sort();
                let mut out = io::stdout().lock();
                for (n, t, c) in idx {
                    let _ = writeln!(out, "{}\t{}\t{}", n, t, c);
                }
                None
            }
            ".schema" => {
                let mut out = io::stdout().lock();
                let names: Vec<String> = if arg.is_empty() {
                    let mut v: Vec<String> = self
                        .engine
                        .catalog()
                        .tables()
                        .map(|t| t.name.clone())
                        .collect();
                    v.sort();
                    v
                } else {
                    vec![arg.to_string()]
                };
                for name in names {
                    match self.engine.catalog().get_table(&name) {
                        Ok(t) => {
                            let _ = writeln!(out, "{}", schema_for_table(t));
                        }
                        Err(e) => {
                            let _ = writeln!(io::stderr(), "{}", e);
                            if !self.continue_on_error {
                                return Some(1);
                            }
                        }
                    }
                }
                let idx_names: Vec<(String, String, String)> = self
                    .engine
                    .catalog()
                    .indexes()
                    .filter(|i| {
                        if arg.is_empty() {
                            true
                        } else {
                            i.table.eq_ignore_ascii_case(arg)
                        }
                    })
                    .filter(|i| !i.name.starts_with("__pk_") && !i.name.starts_with("__uniq_"))
                    .map(|i| (i.name.clone(), i.table.clone(), i.column.clone()))
                    .collect();
                for (n, t, c) in idx_names {
                    let _ = writeln!(out, "CREATE INDEX {} ON {}({});", n, t, c);
                }
                None
            }
            ".format" => {
                if arg.is_empty() {
                    let _ = writeln!(io::stdout(), "{}", format_name(self.format));
                    return None;
                }
                match OutputFormat::parse(arg) {
                    Ok(f) => {
                        self.format = f;
                        None
                    }
                    Err(e) => {
                        self.report_error(&e);
                        if self.continue_on_error {
                            None
                        } else {
                            Some(2)
                        }
                    }
                }
            }
            other => {
                let _ = writeln!(io::stderr(), "unknown meta-command: {}", other);
                if self.continue_on_error {
                    None
                } else {
                    Some(2)
                }
            }
        }
    }

    pub fn run_repl(&mut self, interactive: bool) -> i32 {
        let stdin = io::stdin();
        let mut handle = stdin.lock();
        let mut buf = String::new();
        let mut pending = String::new();

        if interactive {
            self.print_banner();
        }

        loop {
            buf.clear();
            if interactive {
                let prompt = if pending.is_empty() {
                    "minidb> "
                } else {
                    "    ...> "
                };
                let _ = io::stdout().write_all(prompt.as_bytes());
                let _ = io::stdout().flush();
            }
            let n = match handle.read_line(&mut buf) {
                Ok(n) => n,
                Err(e) => {
                    let _ = writeln!(io::stderr(), "minidb: read error: {}", e);
                    return 3;
                }
            };
            if n == 0 {
                // EOF
                if !pending.trim().is_empty() {
                    if let Some(rc) = self.run_one(pending.trim()) {
                        return rc;
                    }
                }
                return 0;
            }
            // Meta-commands and `.exit` are processed line-at-a-time; only when
            // pending buffer is empty.
            let line_trimmed = buf.trim();
            if pending.is_empty() && line_trimmed.starts_with('.') {
                if let Some(rc) = self.run_meta(line_trimmed) {
                    return rc;
                }
                continue;
            }

            pending.push_str(&buf);
            // A statement is complete once we see a semicolon at end-of-line
            // outside of a quoted string. Non-interactive stdin can deliver
            // multiple statements per read; for simplicity we always split on
            // top-level semicolons after each newline.
            if !contains_top_level_semicolon(&pending) {
                continue;
            }
            // Run all complete statements; keep any trailing fragment.
            let (complete, rest) = split_off_complete(&pending);
            for stmt in split_top_level(&complete) {
                let s = stmt.trim();
                if s.is_empty() {
                    continue;
                }
                if let Some(rc) = self.run_one(s) {
                    return rc;
                }
            }
            pending = rest;
        }
    }

    fn print_banner(&self) {
        let _ = writeln!(
            io::stderr(),
            "minidb {} — type SQL or .help for meta-commands",
            env!("CARGO_PKG_VERSION")
        );
    }

    fn report_error(&self, e: &MiniDbError) {
        let _ = writeln!(io::stderr(), "error: {}", e);
    }

    fn print_write_result(&self, wr: &WriteResult) {
        if !self.quiet {
            let _ = writeln!(io::stderr(), "{}", wr.summary);
        }
    }

    fn print_result_set(&self, rs: &ResultSet) -> Result<()> {
        let mut out = io::stdout().lock();
        match self.format {
            OutputFormat::Table => print_table(&mut out, rs)?,
            OutputFormat::Csv => print_csv(&mut out, rs)?,
            OutputFormat::Json => print_json(&mut out, rs)?,
        }
        out.flush()?;
        if !self.quiet {
            let _ = writeln!(
                io::stderr(),
                "({} row{})",
                rs.rows.len(),
                if rs.rows.len() == 1 { "" } else { "s" }
            );
        }
        Ok(())
    }
}

fn error_exit_code(e: &MiniDbError) -> i32 {
    match e {
        MiniDbError::Io(_) | MiniDbError::Storage(_) | MiniDbError::Wal(_) => 3,
        MiniDbError::Cli(_) => 2,
        _ => 1,
    }
}

fn format_name(f: OutputFormat) -> &'static str {
    match f {
        OutputFormat::Table => "table",
        OutputFormat::Csv => "csv",
        OutputFormat::Json => "json",
    }
}

// ---- argument parsing ----
fn parse_args(argv: &[String]) -> Result<Args> {
    let mut db_path: Option<PathBuf> = None;
    let mut mode: Option<Mode> = None;
    let mut format = OutputFormat::Table;
    let mut quiet = false;
    let mut continue_on_error = false;
    let mut init: Option<PathBuf> = None;

    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        match arg.as_str() {
            "--help" | "-h" => mode = Some(Mode::Help),
            "--version" | "-V" => mode = Some(Mode::Version),
            "--repl" => mode = Some(Mode::Repl),
            "--quiet" => quiet = true,
            "--continue-on-error" => continue_on_error = true,
            "--eval" => {
                i += 1;
                let s = argv
                    .get(i)
                    .ok_or_else(|| MiniDbError::cli("--eval requires an argument"))?;
                mode = Some(Mode::Eval(s.clone()));
            }
            s if s.starts_with("--eval=") => {
                mode = Some(Mode::Eval(s["--eval=".len()..].to_string()));
            }
            "--file" => {
                i += 1;
                let p = argv
                    .get(i)
                    .ok_or_else(|| MiniDbError::cli("--file requires a path"))?;
                mode = Some(Mode::File(PathBuf::from(p)));
            }
            s if s.starts_with("--file=") => {
                mode = Some(Mode::File(PathBuf::from(&s["--file=".len()..])));
            }
            "--init" => {
                i += 1;
                let p = argv
                    .get(i)
                    .ok_or_else(|| MiniDbError::cli("--init requires a path"))?;
                init = Some(PathBuf::from(p));
            }
            s if s.starts_with("--init=") => {
                init = Some(PathBuf::from(&s["--init=".len()..]));
            }
            "--format" => {
                i += 1;
                let f = argv
                    .get(i)
                    .ok_or_else(|| MiniDbError::cli("--format requires a value"))?;
                format = OutputFormat::parse(f)?;
            }
            s if s.starts_with("--format=") => {
                format = OutputFormat::parse(&s["--format=".len()..])?;
            }
            s if s.starts_with("--") => {
                return Err(MiniDbError::cli(format!("unknown option: {}", s)));
            }
            _ => {
                if db_path.is_some() {
                    return Err(MiniDbError::cli(format!(
                        "unexpected positional argument: {}",
                        arg
                    )));
                }
                db_path = Some(PathBuf::from(arg));
            }
        }
        i += 1;
    }

    let mode = match mode {
        Some(m) => m,
        None => {
            // Default: REPL when stdin is a TTY, else read all of stdin.
            if io::stdin().is_terminal() {
                Mode::Repl
            } else {
                Mode::Stdin
            }
        }
    };

    Ok(Args {
        db_path,
        mode,
        format,
        quiet,
        continue_on_error,
        init,
    })
}

// ---- formatting ----

fn schema_for_table(t: &crate::catalog::Table) -> String {
    let mut parts = Vec::new();
    for c in &t.columns {
        let mut def = format!("{} {}", c.name, c.data_type.display());
        if c.primary_key {
            def.push_str(" PRIMARY KEY");
        }
        if c.not_null && !c.primary_key {
            def.push_str(" NOT NULL");
        }
        if c.unique && !c.primary_key {
            def.push_str(" UNIQUE");
        }
        if let Some(d) = &c.default {
            def.push_str(&format!(" DEFAULT {}", literal_for_value(d)));
        }
        parts.push(def);
    }
    format!("CREATE TABLE {} ({});", t.name, parts.join(", "))
}

fn literal_for_value(v: &Value) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::Int(n) => n.to_string(),
        Value::Real(r) => {
            if r.is_finite() && r.fract() == 0.0 && r.abs() < 1e16 {
                format!("{:.1}", r)
            } else {
                r.to_string()
            }
        }
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Text(s) => format!("'{}'", s.replace('\'', "''")),
    }
}

fn print_table<W: Write>(w: &mut W, rs: &ResultSet) -> Result<()> {
    let headers: Vec<String> = rs.schema.columns.iter().map(|c| c.name.clone()).collect();
    let body: Vec<Vec<String>> = rs
        .rows
        .iter()
        .map(|r| r.iter().map(value_display).collect())
        .collect();
    let cols = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| display_width(h)).collect();
    for row in &body {
        for (i, cell) in row.iter().enumerate() {
            if i >= widths.len() {
                widths.push(display_width(cell));
            } else if display_width(cell) > widths[i] {
                widths[i] = display_width(cell);
            }
        }
    }
    if cols == 0 {
        return Ok(());
    }
    write_separator(w, &widths)?;
    write_row(w, &headers, &widths)?;
    write_separator(w, &widths)?;
    for row in &body {
        write_row(w, row, &widths)?;
    }
    write_separator(w, &widths)?;
    Ok(())
}

fn write_separator<W: Write>(w: &mut W, widths: &[usize]) -> Result<()> {
    write!(w, "+")?;
    for &n in widths {
        for _ in 0..(n + 2) {
            write!(w, "-")?;
        }
        write!(w, "+")?;
    }
    writeln!(w)?;
    Ok(())
}

fn write_row<W: Write>(w: &mut W, cells: &[String], widths: &[usize]) -> Result<()> {
    write!(w, "|")?;
    for (i, c) in cells.iter().enumerate() {
        let pad = widths
            .get(i)
            .copied()
            .unwrap_or(0)
            .saturating_sub(display_width(c));
        write!(w, " {}", c)?;
        for _ in 0..pad {
            write!(w, " ")?;
        }
        write!(w, " |")?;
    }
    writeln!(w)?;
    Ok(())
}

fn display_width(s: &str) -> usize {
    s.chars().count()
}

fn value_display(v: &Value) -> String {
    match v {
        Value::Null => "NULL".into(),
        other => format!("{}", other),
    }
}

fn print_csv<W: Write>(w: &mut W, rs: &ResultSet) -> Result<()> {
    let headers: Vec<String> = rs
        .schema
        .columns
        .iter()
        .map(|c| csv_quote(&c.name))
        .collect();
    writeln!(w, "{}", headers.join(","))?;
    for row in &rs.rows {
        let cells: Vec<String> = row.iter().map(csv_value).collect();
        writeln!(w, "{}", cells.join(","))?;
    }
    Ok(())
}

fn csv_value(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Int(n) => n.to_string(),
        Value::Real(r) => format!("{}", v_real(*r)),
        Value::Bool(b) => if *b { "true" } else { "false" }.into(),
        Value::Text(s) => csv_quote(s),
    }
}

fn v_real(r: f64) -> String {
    if r.is_finite() && r.fract() == 0.0 && r.abs() < 1e16 {
        format!("{:.1}", r)
    } else {
        r.to_string()
    }
}

fn csv_quote(s: &str) -> String {
    if s.chars().any(|c| matches!(c, '"' | ',' | '\n' | '\r')) {
        let escaped = s.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}

fn print_json<W: Write>(w: &mut W, rs: &ResultSet) -> Result<()> {
    write!(w, "[")?;
    for (i, row) in rs.rows.iter().enumerate() {
        if i > 0 {
            write!(w, ",")?;
        }
        write!(w, "{{")?;
        for (j, v) in row.iter().enumerate() {
            if j > 0 {
                write!(w, ",")?;
            }
            let name = rs
                .schema
                .columns
                .get(j)
                .map(|c| c.name.as_str())
                .unwrap_or("col");
            write!(w, "{}:{}", json_string(name), json_value(v))?;
        }
        write!(w, "}}")?;
    }
    writeln!(w, "]")?;
    Ok(())
}

fn json_value(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Int(n) => n.to_string(),
        Value::Real(r) => {
            if r.is_finite() {
                if r.fract() == 0.0 && r.abs() < 1e16 {
                    format!("{:.1}", r)
                } else {
                    r.to_string()
                }
            } else {
                "null".into()
            }
        }
        Value::Bool(b) => if *b { "true" } else { "false" }.into(),
        Value::Text(s) => json_string(s),
    }
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---- statement splitting ----

/// Split a SQL source on top-level semicolons (ignoring those inside string
/// literals or `--` line comments).
pub fn split_top_level(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut in_str = false;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str {
            buf.push(c);
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    buf.push(chars.next().unwrap());
                } else {
                    in_str = false;
                }
            }
            continue;
        }
        if c == '-' && chars.peek() == Some(&'-') {
            // line comment to end of line
            buf.push(c);
            buf.push(chars.next().unwrap());
            for c2 in chars.by_ref() {
                buf.push(c2);
                if c2 == '\n' {
                    break;
                }
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'*') {
            buf.push(c);
            buf.push(chars.next().unwrap());
            // consume until */
            let mut prev = '\0';
            for c2 in chars.by_ref() {
                buf.push(c2);
                if prev == '*' && c2 == '/' {
                    break;
                }
                prev = c2;
            }
            continue;
        }
        if c == '\'' {
            in_str = true;
            buf.push(c);
            continue;
        }
        if c == ';' {
            out.push(std::mem::take(&mut buf));
            continue;
        }
        buf.push(c);
    }
    if !buf.trim().is_empty() {
        out.push(buf);
    }
    out
}

fn contains_top_level_semicolon(src: &str) -> bool {
    let mut in_str = false;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                } else {
                    in_str = false;
                }
            }
            continue;
        }
        if c == '-' && chars.peek() == Some(&'-') {
            chars.next();
            for c2 in chars.by_ref() {
                if c2 == '\n' {
                    break;
                }
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut prev = '\0';
            for c2 in chars.by_ref() {
                if prev == '*' && c2 == '/' {
                    break;
                }
                prev = c2;
            }
            continue;
        }
        if c == '\'' {
            in_str = true;
            continue;
        }
        if c == ';' {
            return true;
        }
    }
    false
}

/// Split off the prefix containing all complete statements (up through the
/// last top-level `;`). Returns `(complete, rest)`.
fn split_off_complete(src: &str) -> (String, String) {
    let mut last_semi: Option<usize> = None;
    let mut in_str = false;
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if c == '\'' {
                if i + 1 < bytes.len() && bytes[i + 1] as char == '\'' {
                    i += 2;
                    continue;
                }
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == '-' && i + 1 < bytes.len() && bytes[i + 1] as char == '-' {
            // skip to newline
            while i < bytes.len() && bytes[i] as char != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] as char == '*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] as char == '*' && bytes[i + 1] as char == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        if c == '\'' {
            in_str = true;
            i += 1;
            continue;
        }
        if c == ';' {
            last_semi = Some(i);
        }
        i += 1;
    }
    match last_semi {
        Some(idx) => (src[..=idx].to_string(), src[idx + 1..].to_string()),
        None => (String::new(), src.to_string()),
    }
}

// ---- borrowed reader to silence dead-code warnings ----
#[allow(dead_code)]
fn _read_to_end<R: Read>(mut r: R) -> io::Result<String> {
    let mut s = String::new();
    r.read_to_string(&mut s)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_handles_strings_and_comments() {
        let src = "INSERT INTO t VALUES ('a;b'); -- comment;\nSELECT 1;";
        let parts = split_top_level(src);
        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains("'a;b'"));
        assert!(parts[1].contains("SELECT"));
    }

    #[test]
    fn output_format_parse() {
        assert_eq!(OutputFormat::parse("table").unwrap(), OutputFormat::Table);
        assert_eq!(OutputFormat::parse("CSV").unwrap(), OutputFormat::Csv);
        assert_eq!(OutputFormat::parse("Json").unwrap(), OutputFormat::Json);
        assert!(OutputFormat::parse("xml").is_err());
    }

    #[test]
    fn csv_quotes_special_chars() {
        assert_eq!(csv_quote("hi"), "hi");
        assert_eq!(csv_quote("a,b"), "\"a,b\"");
        assert_eq!(csv_quote("she said \"hi\""), "\"she said \"\"hi\"\"\"");
    }

    #[test]
    fn json_value_basic() {
        assert_eq!(json_value(&Value::Null), "null");
        assert_eq!(json_value(&Value::Int(7)), "7");
        assert_eq!(json_value(&Value::Bool(true)), "true");
        assert_eq!(json_value(&Value::Text("a\"b".into())), "\"a\\\"b\"");
    }
}
