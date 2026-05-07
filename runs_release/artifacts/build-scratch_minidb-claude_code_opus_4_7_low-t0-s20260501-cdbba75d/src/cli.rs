//! CLI argument parsing, REPL, and output formatting.
//!
//! Output goes to stdout; per-statement summaries and error messages go to
//! stderr. The format selection (`--format` or `.format`) chooses between a
//! human-readable table, RFC 4180 CSV, or JSON arrays of row-objects.

use crate::engine::{Database, MutationKind, StmtResult};
use crate::error::{exit_code_for, MiniDbError, Result};
use crate::exec::operator::ExecSchema;
use crate::parser::parse_sql;
use crate::types::Value;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CliArgs {
    pub db: PathBuf,
    pub mode: Mode,
    pub format: OutputFormat,
    pub continue_on_error: bool,
    pub quiet: bool,
}

#[derive(Debug, Clone)]
pub enum Mode {
    Repl,
    Eval(String),
    File(PathBuf),
    Init,
    PrintHelp,
    PrintVersion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

impl OutputFormat {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "table" => OutputFormat::Table,
            "csv" => OutputFormat::Csv,
            "json" => OutputFormat::Json,
            _ => return None,
        })
    }
}

pub const HELP_TEXT: &str = "minidb — embeddable SQL-subset relational database\n\
\n\
USAGE:\n\
    minidb [OPTIONS]\n\
\n\
OPTIONS:\n\
    --db <PATH>             database file path (default: ./minidb.db)\n\
    --eval \"<SQL>\"          execute one SQL statement (or ;-separated list)\n\
    --file <PATH>           execute the SQL script at PATH\n\
    --repl                  force REPL mode even when stdin is not a TTY\n\
    --init                  create a fresh empty database file and exit\n\
    --format {table|csv|json}\n\
    --continue-on-error     do not stop --file on the first error\n\
    --quiet                 suppress per-statement summary lines on stderr\n\
    --version               print version and exit\n\
    --help                  print this help and exit\n";

pub fn parse_args(args: &[String]) -> Result<CliArgs> {
    let mut db = PathBuf::from("./minidb.db");
    let mut format = OutputFormat::Table;
    let mut continue_on_error = false;
    let mut quiet = false;
    let mut mode: Option<Mode> = None;
    let mut force_repl = false;

    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--help" | "-h" => mode = Some(Mode::PrintHelp),
            "--version" | "-V" => mode = Some(Mode::PrintVersion),
            "--init" => mode = Some(Mode::Init),
            "--repl" => force_repl = true,
            "--db" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| MiniDbError::Cli("--db requires an argument".into()))?;
                db = PathBuf::from(v);
            }
            "--eval" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| MiniDbError::Cli("--eval requires an argument".into()))?;
                mode = Some(Mode::Eval(v.clone()));
            }
            "--file" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| MiniDbError::Cli("--file requires an argument".into()))?;
                mode = Some(Mode::File(PathBuf::from(v)));
            }
            "--format" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| MiniDbError::Cli("--format requires an argument".into()))?;
                format = OutputFormat::parse(v)
                    .ok_or_else(|| MiniDbError::Cli(format!("unknown format: {}", v)))?;
            }
            "--continue-on-error" => continue_on_error = true,
            "--quiet" => quiet = true,
            other => return Err(MiniDbError::Cli(format!("unknown argument: {}", other))),
        }
        i += 1;
    }
    let mode = mode.unwrap_or(if force_repl || atty_stdin() {
        Mode::Repl
    } else {
        Mode::Repl
    });
    Ok(CliArgs {
        db,
        mode,
        format,
        continue_on_error,
        quiet,
    })
}

fn atty_stdin() -> bool {
    // Conservative check: rely on `isatty(0)` via libc. If unavailable, assume
    // TTY so REPL still launches in interactive shells.
    #[cfg(unix)]
    {
        unsafe { libc_isatty(0) }
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(unix)]
unsafe fn libc_isatty(fd: i32) -> bool {
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    isatty(fd) == 1
}

pub fn run(args: CliArgs) -> Result<i32> {
    let mut session = Session {
        format: args.format,
        quiet: args.quiet,
    };
    match &args.mode {
        Mode::PrintHelp => {
            println!("{}", HELP_TEXT);
            Ok(0)
        }
        Mode::PrintVersion => {
            println!("minidb {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Mode::Init => {
            Database::init_empty(&args.db)?;
            eprintln!("initialized empty database at {}", args.db.display());
            Ok(0)
        }
        Mode::Eval(sql) => {
            let mut db = Database::open(&args.db)?;
            let stdout = io::stdout();
            let mut out = stdout.lock();
            run_sql(&mut db, sql, &mut session, &mut out, args.continue_on_error)
        }
        Mode::File(path) => {
            let mut db = Database::open(&args.db)?;
            let sql = std::fs::read_to_string(path)?;
            let stdout = io::stdout();
            let mut out = stdout.lock();
            run_sql(
                &mut db,
                &sql,
                &mut session,
                &mut out,
                args.continue_on_error,
            )
        }
        Mode::Repl => {
            let mut db = Database::open(&args.db)?;
            run_repl(&mut db, &mut session)
        }
    }
}

pub struct Session {
    pub format: OutputFormat,
    pub quiet: bool,
}

pub fn run_sql<W: Write>(
    db: &mut Database,
    sql: &str,
    session: &mut Session,
    out: &mut W,
    continue_on_error: bool,
) -> Result<i32> {
    // Split out meta-commands (lines starting with `.`) from real SQL.
    let mut last_err: Option<MiniDbError> = None;
    for chunk in split_for_meta(sql) {
        match chunk {
            Chunk::Meta(m) => {
                if let Err(e) = run_meta(&m, db, session, out) {
                    eprintln!("error: {}", e);
                    if !continue_on_error {
                        return Ok(exit_code_for(&e));
                    }
                    last_err = Some(e);
                }
            }
            Chunk::Sql(text) => match parse_sql(&text) {
                Ok(stmts) => {
                    for s in stmts {
                        match db.execute(&s) {
                            Ok(r) => emit_result(&r, session, out)?,
                            Err(e) => {
                                eprintln!("error: {}", e);
                                if !continue_on_error {
                                    return Ok(exit_code_for(&e));
                                }
                                last_err = Some(e);
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    if !continue_on_error {
                        return Ok(exit_code_for(&e));
                    }
                    last_err = Some(e);
                }
            },
        }
    }
    if let Some(e) = last_err {
        Ok(exit_code_for(&e))
    } else {
        Ok(0)
    }
}

fn run_repl(db: &mut Database, session: &mut Session) -> Result<i32> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut buf = String::new();
    let mut interactive = atty_stdin();
    if interactive {
        eprintln!(
            "minidb {} — type SQL ending with ';', or .exit to quit",
            env!("CARGO_PKG_VERSION")
        );
    }
    loop {
        if interactive {
            eprint!("minidb> ");
            let _ = io::stderr().flush();
        }
        let mut line = String::new();
        let n = stdin.lock().read_line(&mut line)?;
        if n == 0 {
            break;
        }
        buf.push_str(&line);
        // A leading `.` at the head of `buf` is a meta-command; it terminates
        // at the newline rather than at `;`.
        while let Some(rest) = buf.strip_prefix('.') {
            // find newline
            if let Some(nl) = rest.find('\n') {
                let cmd: String = format!(".{}", &rest[..nl]).trim().to_string();
                let consumed = 1 + nl + 1;
                buf = buf[consumed..].to_string();
                if let Err(e) = run_meta(&cmd, db, session, &mut out) {
                    eprintln!("error: {}", e);
                }
                if cmd.eq_ignore_ascii_case(".exit") {
                    return Ok(0);
                }
            } else {
                break;
            }
        }
        // Process complete statements terminated by `;` (outside string literals).
        loop {
            match find_terminator(&buf) {
                Some(pos) => {
                    let stmt: String = buf.drain(..=pos).collect();
                    let trimmed = stmt.trim().trim_end_matches(';').trim().to_string();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if trimmed.starts_with('.') {
                        if let Err(e) = run_meta(&trimmed, db, session, &mut out) {
                            eprintln!("error: {}", e);
                        }
                        if trimmed.eq_ignore_ascii_case(".exit") {
                            return Ok(0);
                        }
                    } else {
                        match parse_sql(&format!("{};", trimmed)) {
                            Ok(stmts) => {
                                for s in stmts {
                                    match db.execute(&s) {
                                        Ok(r) => {
                                            if let Err(e) = emit_result(&r, session, &mut out) {
                                                eprintln!("error: {}", e);
                                            }
                                        }
                                        Err(e) => eprintln!("error: {}", e),
                                    }
                                }
                            }
                            Err(e) => eprintln!("error: {}", e),
                        }
                    }
                }
                None => break,
            }
        }
        let _ = interactive; // avoid warning
        interactive = atty_stdin();
    }
    Ok(0)
}

fn find_terminator(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut in_str: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match in_str {
            Some(q) => {
                if b == q {
                    if i + 1 < bytes.len() && bytes[i + 1] == q {
                        i += 2;
                        continue;
                    }
                    in_str = None;
                }
                i += 1;
            }
            None => {
                if b == b'\'' || b == b'"' {
                    in_str = Some(b);
                    i += 1;
                } else if b == b';' {
                    return Some(i);
                } else if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
    None
}

enum Chunk {
    Meta(String),
    Sql(String),
}

fn split_for_meta(s: &str) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut sql_buf = String::new();
    for line in s.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('.') {
            if !sql_buf.trim().is_empty() {
                out.push(Chunk::Sql(std::mem::take(&mut sql_buf)));
            } else {
                sql_buf.clear();
            }
            out.push(Chunk::Meta(
                trimmed.trim_end_matches(';').trim().to_string(),
            ));
        } else {
            sql_buf.push_str(line);
            sql_buf.push('\n');
        }
    }
    if !sql_buf.trim().is_empty() {
        out.push(Chunk::Sql(sql_buf));
    }
    out
}

fn run_meta<W: Write>(
    cmd: &str,
    db: &mut Database,
    session: &mut Session,
    out: &mut W,
) -> Result<()> {
    let mut parts = cmd.split_whitespace();
    let head = parts.next().unwrap_or("");
    match head.to_ascii_lowercase().as_str() {
        ".tables" => {
            for t in db.list_tables() {
                writeln!(out, "{}", t)?;
            }
        }
        ".indexes" => {
            for (n, t, c) in db.list_indexes() {
                writeln!(out, "{}\t{}({})", n, t, c)?;
            }
        }
        ".schema" => {
            let target = parts.next();
            for tname in db.list_tables() {
                if let Some(t) = target {
                    if !tname.eq_ignore_ascii_case(t) {
                        continue;
                    }
                }
                if let Some(s) = db.schema_of(&tname) {
                    let cols: Vec<String> = s
                        .columns
                        .iter()
                        .map(|c| {
                            let mut s = format!("{} {}", c.name, c.data_type);
                            if c.primary_key {
                                s.push_str(" PRIMARY KEY");
                            } else if c.not_null {
                                s.push_str(" NOT NULL");
                            }
                            if c.unique && !c.primary_key {
                                s.push_str(" UNIQUE");
                            }
                            s
                        })
                        .collect();
                    writeln!(out, "CREATE TABLE {} ({});", tname, cols.join(", "))?;
                }
            }
        }
        ".format" => {
            let v = parts
                .next()
                .ok_or_else(|| MiniDbError::Cli(".format requires kind".into()))?;
            session.format = OutputFormat::parse(v)
                .ok_or_else(|| MiniDbError::Cli(format!("unknown format: {}", v)))?;
        }
        ".exit" => {}
        other => return Err(MiniDbError::Cli(format!("unknown meta-command: {}", other))),
    }
    Ok(())
}

fn emit_result<W: Write>(r: &StmtResult, session: &Session, out: &mut W) -> Result<()> {
    match r {
        StmtResult::Rows { schema, rows } => {
            match session.format {
                OutputFormat::Table => write_table(out, schema, rows)?,
                OutputFormat::Csv => write_csv(out, schema, rows)?,
                OutputFormat::Json => write_json(out, schema, rows)?,
            }
            if !session.quiet {
                let n = rows.len();
                let label = if n == 1 { "row" } else { "rows" };
                eprintln!("ok {} {}", n, label);
            }
        }
        StmtResult::Mutation { affected, kind } => {
            if !session.quiet {
                let label = match kind {
                    MutationKind::Insert | MutationKind::Update | MutationKind::Delete => {
                        if *affected == 1 {
                            "row affected"
                        } else {
                            "rows affected"
                        }
                    }
                    MutationKind::Ddl => "ok",
                };
                if matches!(kind, MutationKind::Ddl) {
                    eprintln!("ok");
                } else {
                    eprintln!("ok {} {}", affected, label);
                }
            }
        }
        StmtResult::Ack(s) => {
            if !session.quiet {
                eprintln!("ok {}", s);
            }
        }
    }
    Ok(())
}

fn write_table<W: Write>(out: &mut W, schema: &ExecSchema, rows: &[Vec<Value>]) -> Result<()> {
    let names: Vec<String> = schema.names();
    if names.is_empty() {
        return Ok(());
    }
    let mut widths: Vec<usize> = names.iter().map(|s| s.chars().count()).collect();
    let str_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| r.iter().map(|v| format!("{}", v)).collect())
        .collect();
    for r in &str_rows {
        for (i, c) in r.iter().enumerate() {
            widths[i] = widths[i].max(c.chars().count());
        }
    }
    // header
    let line = |out: &mut W, widths: &[usize]| -> Result<()> {
        let parts: Vec<String> = widths.iter().map(|w| "-".repeat(*w + 2)).collect();
        writeln!(out, "+{}+", parts.join("+"))?;
        Ok(())
    };
    line(out, &widths)?;
    let header: Vec<String> = names
        .iter()
        .zip(widths.iter())
        .map(|(n, w)| format!(" {:<width$} ", n, width = w))
        .collect();
    writeln!(out, "|{}|", header.join("|"))?;
    line(out, &widths)?;
    for r in &str_rows {
        let cells: Vec<String> = r
            .iter()
            .zip(widths.iter())
            .map(|(c, w)| format!(" {:<width$} ", c, width = w))
            .collect();
        writeln!(out, "|{}|", cells.join("|"))?;
    }
    line(out, &widths)?;
    Ok(())
}

fn write_csv<W: Write>(out: &mut W, schema: &ExecSchema, rows: &[Vec<Value>]) -> Result<()> {
    let names: Vec<String> = schema.names().iter().map(|s| csv_escape(s)).collect();
    writeln!(out, "{}", names.join(","))?;
    for r in rows {
        let cells: Vec<String> = r.iter().map(|v| v.display_csv()).collect();
        writeln!(out, "{}", cells.join(","))?;
    }
    Ok(())
}

fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn write_json<W: Write>(out: &mut W, schema: &ExecSchema, rows: &[Vec<Value>]) -> Result<()> {
    let names = schema.names();
    write!(out, "[")?;
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            write!(out, ",")?;
        }
        write!(out, "{{")?;
        for (j, (n, v)) in names.iter().zip(r.iter()).enumerate() {
            if j > 0 {
                write!(out, ",")?;
            }
            write!(out, "{}:{}", json_quote(n), v.display_json())?;
        }
        write!(out, "}}")?;
    }
    writeln!(out, "]")?;
    Ok(())
}

fn json_quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_args() {
        let args: Vec<String> = ["minidb", "--db", "x.db", "--eval", "SELECT 1;"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let a = parse_args(&args).unwrap();
        assert_eq!(a.db, PathBuf::from("x.db"));
        match a.mode {
            Mode::Eval(s) => assert_eq!(s, "SELECT 1;"),
            _ => panic!(),
        }
    }

    #[test]
    fn output_format_parse() {
        assert_eq!(OutputFormat::parse("csv"), Some(OutputFormat::Csv));
        assert_eq!(OutputFormat::parse("json"), Some(OutputFormat::Json));
        assert_eq!(OutputFormat::parse("nope"), None);
    }

    #[test]
    fn terminator_finds_outside_strings() {
        assert_eq!(find_terminator("a;b"), Some(1));
        assert_eq!(find_terminator("'a;b';"), Some(5));
        assert_eq!(find_terminator("nope"), None);
    }
}
