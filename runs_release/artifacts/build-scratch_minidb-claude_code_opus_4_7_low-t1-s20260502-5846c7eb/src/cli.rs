//! Command-line front-end and REPL.

use crate::error::{ErrorCategory, MiniDbError, Result};
use crate::exec::{AffectedKind, Database, StatementResult};
use crate::types::Value;
use std::io::{BufRead, BufReader, IsTerminal, Read, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

impl OutputFormat {
    pub fn parse(s: &str) -> std::result::Result<OutputFormat, String> {
        match s.to_ascii_lowercase().as_str() {
            "table" => Ok(OutputFormat::Table),
            "csv" => Ok(OutputFormat::Csv),
            "json" => Ok(OutputFormat::Json),
            _ => Err(format!("unknown format: {}", s)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CliOptions {
    pub db: PathBuf,
    pub eval: Option<String>,
    pub file: Option<PathBuf>,
    pub repl: bool,
    pub format: OutputFormat,
    pub quiet: bool,
    pub continue_on_error: bool,
    pub init: bool,
    pub show_help: bool,
    pub show_version: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        CliOptions {
            db: PathBuf::from("./minidb.db"),
            eval: None,
            file: None,
            repl: false,
            format: OutputFormat::Table,
            quiet: false,
            continue_on_error: false,
            init: false,
            show_help: false,
            show_version: false,
        }
    }
}

pub const HELP_TEXT: &str = "\
minidb — embeddable SQL-subset relational database

Usage:
  minidb [options]

Options:
  --db <PATH>            Path to the database file (default: ./minidb.db)
  --eval \"<SQL>\"        Execute the given SQL statement(s) and exit
  --file <PATH>          Execute the SQL script and exit
  --repl                 Force interactive REPL mode
  --format {table,csv,json}
                         Output format for SELECT result sets
  --quiet                Suppress per-statement summary lines on stderr
  --continue-on-error    With --file, continue past errors instead of stopping
  --init                 Create a fresh empty database file and exit
  --version              Print version and exit
  --help                 Show this help and exit

Meta-commands (REPL only):
  .schema [table]        Show CREATE TABLE for the named table (or all tables)
  .tables                List user tables
  .indexes               List user indexes
  .format <kind>         Switch the output format (table|csv|json)
  .exit                  Quit the REPL
";

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn parse_args(argv: &[String]) -> std::result::Result<CliOptions, String> {
    let mut opt = CliOptions::default();
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        match a.as_str() {
            "--help" | "-h" => {
                opt.show_help = true;
                i += 1;
            }
            "--version" | "-V" => {
                opt.show_version = true;
                i += 1;
            }
            "--repl" => {
                opt.repl = true;
                i += 1;
            }
            "--quiet" => {
                opt.quiet = true;
                i += 1;
            }
            "--continue-on-error" => {
                opt.continue_on_error = true;
                i += 1;
            }
            "--init" => {
                opt.init = true;
                i += 1;
            }
            "--db" => {
                i += 1;
                if i >= argv.len() {
                    return Err("--db expects a path".into());
                }
                opt.db = PathBuf::from(&argv[i]);
                i += 1;
            }
            "--eval" => {
                i += 1;
                if i >= argv.len() {
                    return Err("--eval expects a SQL string".into());
                }
                opt.eval = Some(argv[i].clone());
                i += 1;
            }
            "--file" => {
                i += 1;
                if i >= argv.len() {
                    return Err("--file expects a path".into());
                }
                opt.file = Some(PathBuf::from(&argv[i]));
                i += 1;
            }
            "--format" => {
                i += 1;
                if i >= argv.len() {
                    return Err("--format expects a value".into());
                }
                opt.format = OutputFormat::parse(&argv[i])?;
                i += 1;
            }
            other => return Err(format!("unknown argument: {}", other)),
        }
    }
    Ok(opt)
}

/// Run the CLI; return the process exit code.
pub fn run<R: Read, W: Write, E: Write>(
    argv: Vec<String>,
    stdin: R,
    stdout: &mut W,
    stderr: &mut E,
    is_tty: bool,
) -> i32 {
    let opt = match parse_args(&argv) {
        Ok(o) => o,
        Err(e) => {
            let _ = writeln!(stderr, "minidb: {}", e);
            let _ = writeln!(stderr, "{}", HELP_TEXT);
            return 2;
        }
    };
    if opt.show_help {
        let _ = writeln!(stdout, "{}", HELP_TEXT);
        return 0;
    }
    if opt.show_version {
        let _ = writeln!(stdout, "minidb {}", VERSION);
        return 0;
    }
    if opt.init {
        // Create a fresh empty database (truncate if exists).
        if opt.db.exists() {
            if let Err(e) = std::fs::remove_file(&opt.db) {
                let _ = writeln!(
                    stderr,
                    "minidb: failed to remove {}: {}",
                    opt.db.display(),
                    e
                );
                return 3;
            }
        }
        match Database::open(&opt.db) {
            Ok(db) => {
                let _ = db.close();
                return 0;
            }
            Err(e) => {
                let _ = writeln!(stderr, "minidb: {}", e);
                return 3;
            }
        }
    }

    // Open database
    let mut db = match Database::open(&opt.db) {
        Ok(d) => d,
        Err(e) => {
            let _ = writeln!(stderr, "minidb: {}", e);
            return classify_exit(&e);
        }
    };

    if let Some(sql) = &opt.eval {
        return execute_string(&mut db, sql, &opt, stdout, stderr);
    }
    if let Some(path) = &opt.file {
        let sql = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                let _ = writeln!(stderr, "minidb: cannot read {}: {}", path.display(), e);
                return 3;
            }
        };
        return execute_script(&mut db, &sql, &opt, stdout, stderr);
    }

    if opt.repl || is_tty {
        return repl(&mut db, opt, stdin, stdout, stderr);
    }
    // Non-interactive stdin: read all and execute as a script.
    let mut buf = String::new();
    let mut reader = BufReader::new(stdin);
    if let Err(e) = reader.read_to_string(&mut buf) {
        let _ = writeln!(stderr, "minidb: {}", e);
        return 3;
    }
    execute_script(&mut db, &buf, &opt, stdout, stderr)
}

fn classify_exit(e: &MiniDbError) -> i32 {
    match e.category() {
        ErrorCategory::Sql => 1,
        ErrorCategory::Usage => 2,
        ErrorCategory::Storage => 3,
    }
}

fn execute_string<W: Write, E: Write>(
    db: &mut Database,
    sql: &str,
    opt: &CliOptions,
    stdout: &mut W,
    stderr: &mut E,
) -> i32 {
    match db.execute_sql(sql) {
        Ok(results) => {
            for r in results {
                emit_result(&r, opt, stdout, stderr);
            }
            0
        }
        Err(e) => {
            let _ = writeln!(stderr, "minidb: {}", e);
            classify_exit(&e)
        }
    }
}

fn execute_script<W: Write, E: Write>(
    db: &mut Database,
    sql: &str,
    opt: &CliOptions,
    stdout: &mut W,
    stderr: &mut E,
) -> i32 {
    // Split by semicolons but respect string literals and comments.
    let statements = split_statements(sql);
    let mut last_code = 0;
    for stmt in statements {
        let stmt = stmt.trim();
        if stmt.is_empty() {
            continue;
        }
        if let Some(rest) = stmt.strip_prefix('.') {
            if let Some(code) = handle_meta(db, rest, opt, stdout, stderr) {
                if code != 0 {
                    last_code = code;
                    if !opt.continue_on_error {
                        return last_code;
                    }
                }
                continue;
            }
        }
        match db.execute_sql(&format!("{};", stmt)) {
            Ok(results) => {
                for r in results {
                    emit_result(&r, opt, stdout, stderr);
                }
            }
            Err(e) => {
                let _ = writeln!(stderr, "minidb: {}", e);
                last_code = classify_exit(&e);
                if !opt.continue_on_error {
                    return last_code;
                }
            }
        }
    }
    last_code
}

fn repl<R: Read, W: Write, E: Write>(
    db: &mut Database,
    mut opt: CliOptions,
    stdin: R,
    stdout: &mut W,
    stderr: &mut E,
) -> i32 {
    let mut reader = BufReader::new(stdin);
    let mut buffer = String::new();
    let _ = writeln!(stderr, "minidb {} — type .exit to quit", VERSION);
    loop {
        if buffer.is_empty() {
            let _ = write!(stderr, "minidb> ");
        } else {
            let _ = write!(stderr, "    ... ");
        }
        let _ = stderr.flush();
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                let _ = writeln!(stderr, "minidb: {}", e);
                return 3;
            }
        }
        buffer.push_str(&line);

        // Meta-commands at the start of a fresh buffer
        let trimmed = buffer.trim();
        if trimmed.starts_with('.') && (line.ends_with('\n') || line.ends_with(';')) {
            let cmd = trimmed
                .trim_end_matches(';')
                .trim_start_matches('.')
                .to_string();
            buffer.clear();
            match handle_meta(db, &cmd, &opt, stdout, stderr) {
                Some(_code) => {
                    if cmd == "exit" {
                        return 0;
                    }
                    // continue
                }
                None => {
                    let _ = writeln!(stderr, "unknown meta-command: .{}", cmd);
                }
            }
            // .format may have changed the format
            opt.format = current_format();
            continue;
        }

        // Statement complete? Look for trailing ';' (outside strings).
        if !ends_statement(&buffer) {
            continue;
        }
        let to_run = std::mem::take(&mut buffer);
        match db.execute_sql(&to_run) {
            Ok(results) => {
                for r in results {
                    emit_result(&r, &opt, stdout, stderr);
                }
            }
            Err(e) => {
                let _ = writeln!(stderr, "minidb: {}", e);
            }
        }
    }
    0
}

// Process-global format toggled by .format meta-command in REPL.
use std::sync::atomic::{AtomicU8, Ordering};
static CUR_FORMAT: AtomicU8 = AtomicU8::new(0);

fn set_format(f: OutputFormat) {
    let v = match f {
        OutputFormat::Table => 0,
        OutputFormat::Csv => 1,
        OutputFormat::Json => 2,
    };
    CUR_FORMAT.store(v, Ordering::Relaxed);
}

fn current_format() -> OutputFormat {
    match CUR_FORMAT.load(Ordering::Relaxed) {
        1 => OutputFormat::Csv,
        2 => OutputFormat::Json,
        _ => OutputFormat::Table,
    }
}

fn handle_meta<W: Write, E: Write>(
    db: &mut Database,
    cmd: &str,
    _opt: &CliOptions,
    stdout: &mut W,
    stderr: &mut E,
) -> Option<i32> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let head = parts.first().map(|s| *s).unwrap_or("");
    match head {
        "exit" | "quit" => {
            // signal exit; in REPL we return immediately
            std::process::exit(0);
        }
        "tables" => {
            let mut names: Vec<&String> = db.catalog.tables.keys().collect();
            names.sort();
            for n in names {
                let _ = writeln!(stdout, "{}", n);
            }
            Some(0)
        }
        "indexes" => {
            let mut idx: Vec<&crate::catalog::IndexSchema> = db
                .catalog
                .indexes
                .values()
                .filter(|i| !i.name.starts_with("__"))
                .collect();
            idx.sort_by(|a, b| a.name.cmp(&b.name));
            for i in idx {
                let _ = writeln!(stdout, "{}\t{}\t{}", i.name, i.table, i.column);
            }
            Some(0)
        }
        "schema" => {
            let target = parts.get(1);
            let mut names: Vec<String> = db.catalog.tables.keys().cloned().collect();
            names.sort();
            for n in names {
                if let Some(t) = target {
                    if &n != t {
                        continue;
                    }
                }
                let ts = db.catalog.tables.get(&n).unwrap();
                let _ = writeln!(stdout, "{}", schema_to_sql(ts));
            }
            Some(0)
        }
        "format" => {
            if let Some(kind) = parts.get(1) {
                match OutputFormat::parse(kind) {
                    Ok(f) => {
                        set_format(f);
                        Some(0)
                    }
                    Err(e) => {
                        let _ = writeln!(stderr, "{}", e);
                        Some(2)
                    }
                }
            } else {
                let _ = writeln!(stderr, ".format expects a kind");
                Some(2)
            }
        }
        _ => None,
    }
}

fn schema_to_sql(t: &crate::catalog::TableSchema) -> String {
    let mut s = format!("CREATE TABLE {} (", t.name);
    let mut parts = Vec::new();
    for c in &t.columns {
        let mut p = format!("{} {}", c.name, c.data_type);
        if c.primary_key {
            p.push_str(" PRIMARY KEY");
        } else if c.not_null {
            p.push_str(" NOT NULL");
        }
        if c.unique && !c.primary_key {
            p.push_str(" UNIQUE");
        }
        parts.push(p);
    }
    s.push_str(&parts.join(", "));
    s.push_str(");");
    s
}

fn ends_statement(buf: &str) -> bool {
    let mut in_str = false;
    for c in buf.chars() {
        if c == '\'' {
            in_str = !in_str;
        }
    }
    if in_str {
        return false;
    }
    buf.trim_end().ends_with(';')
}

/// Split a SQL script into individual statements at top-level ';'.
pub fn split_statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut in_line_comment = false;
    let mut prev = ' ';
    for c in sql.chars() {
        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
            }
            cur.push(c);
            prev = c;
            continue;
        }
        if !in_str && prev == '-' && c == '-' {
            in_line_comment = true;
            cur.push(c);
            prev = c;
            continue;
        }
        if c == '\'' {
            in_str = !in_str;
        }
        if c == ';' && !in_str {
            out.push(cur.trim().to_string());
            cur.clear();
            prev = c;
            continue;
        }
        cur.push(c);
        prev = c;
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

fn emit_result<W: Write, E: Write>(
    r: &StatementResult,
    opt: &CliOptions,
    stdout: &mut W,
    stderr: &mut E,
) {
    let fmt =
        if matches!(opt.format, OutputFormat::Table) && current_format() != OutputFormat::Table {
            current_format()
        } else {
            opt.format
        };
    match r {
        StatementResult::Rows { columns, rows } => {
            match fmt {
                OutputFormat::Table => print_table(columns, rows, stdout),
                OutputFormat::Csv => print_csv(columns, rows, stdout),
                OutputFormat::Json => print_json(columns, rows, stdout),
            }
            if !opt.quiet {
                let n = rows.len();
                let s = if n == 1 { "row" } else { "rows" };
                let _ = writeln!(stderr, "ok {} {}", n, s);
            }
        }
        StatementResult::Affected { kind, n } => {
            if !opt.quiet {
                let s = if *n == 1 { "row" } else { "rows" };
                match kind {
                    AffectedKind::Inserted => {
                        let _ = writeln!(stderr, "ok {} {} inserted", n, s);
                    }
                    AffectedKind::Updated => {
                        let _ = writeln!(stderr, "ok {} {} affected", n, s);
                    }
                    AffectedKind::Deleted => {
                        let _ = writeln!(stderr, "ok {} {} deleted", n, s);
                    }
                }
            }
        }
        StatementResult::Acknowledged(s) => {
            if !opt.quiet {
                let _ = writeln!(stderr, "ok {}", s);
            }
        }
    }
}

fn print_table<W: Write>(cols: &[String], rows: &[Vec<Value>], out: &mut W) {
    let mut widths: Vec<usize> = cols.iter().map(|c| c.chars().count()).collect();
    let mut rendered: Vec<Vec<String>> = Vec::with_capacity(rows.len());
    for r in rows {
        let mut rr = Vec::with_capacity(r.len());
        for (i, v) in r.iter().enumerate() {
            let s = v.to_display();
            if i < widths.len() && s.chars().count() > widths[i] {
                widths[i] = s.chars().count();
            }
            rr.push(s);
        }
        rendered.push(rr);
    }
    // header
    let mut header = String::new();
    for (i, c) in cols.iter().enumerate() {
        if i > 0 {
            header.push_str(" | ");
        }
        header.push_str(&pad(c, widths[i]));
    }
    let _ = writeln!(out, "{}", header);
    let mut sep = String::new();
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            sep.push_str("-+-");
        }
        for _ in 0..*w {
            sep.push('-');
        }
    }
    let _ = writeln!(out, "{}", sep);
    for r in &rendered {
        let mut line = String::new();
        for (i, v) in r.iter().enumerate() {
            if i > 0 {
                line.push_str(" | ");
            }
            line.push_str(&pad(v, widths[i]));
        }
        let _ = writeln!(out, "{}", line);
    }
}

fn pad(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n >= w {
        s.to_string()
    } else {
        let mut x = s.to_string();
        for _ in 0..(w - n) {
            x.push(' ');
        }
        x
    }
}

fn print_csv<W: Write>(cols: &[String], rows: &[Vec<Value>], out: &mut W) {
    let header: Vec<String> = cols.iter().map(|c| csv_escape(c)).collect();
    let _ = writeln!(out, "{}", header.join(","));
    for r in rows {
        let parts: Vec<String> = r.iter().map(|v| v.to_csv()).collect();
        let _ = writeln!(out, "{}", parts.join(","));
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        let esc = s.replace('"', "\"\"");
        format!("\"{}\"", esc)
    } else {
        s.to_string()
    }
}

fn print_json<W: Write>(cols: &[String], rows: &[Vec<Value>], out: &mut W) {
    let _ = write!(out, "[");
    for (ri, r) in rows.iter().enumerate() {
        if ri > 0 {
            let _ = write!(out, ",");
        }
        let _ = write!(out, "{{");
        for (ci, v) in r.iter().enumerate() {
            if ci > 0 {
                let _ = write!(out, ",");
            }
            let _ = write!(out, "{}:{}", json_string(&cols[ci]), v.to_json());
        }
        let _ = write!(out, "}}");
    }
    let _ = writeln!(out, "]");
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
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Standard entry: read argv from std::env, dispatch.
pub fn main_entry() -> i32 {
    let _ = env_logger::try_init();
    let argv: Vec<String> = std::env::args().collect();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let is_tty = stdin.is_terminal();
    let mut sout = stdout.lock();
    let mut serr = stderr.lock();
    run(argv, stdin.lock(), &mut sout, &mut serr, is_tty)
}

#[allow(dead_code)]
fn _unused(_: Result<()>) {}
