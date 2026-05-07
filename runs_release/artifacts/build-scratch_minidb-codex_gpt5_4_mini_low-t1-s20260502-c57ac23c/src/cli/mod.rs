use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use atty::Stream;
use clap::{Parser as ClapParser, ValueEnum};

use crate::error::MiniDbError;
use crate::exec::{ExecResult, Executor};
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::storage::Storage;

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

#[derive(Debug, ClapParser)]
#[command(
    name = "minidb",
    version,
    about = "Embeddable SQL-subset relational database"
)]
pub struct Cli {
    #[arg(long, default_value = "./minidb.db")]
    pub db: PathBuf,
    #[arg(long)]
    pub repl: bool,
    #[arg(long)]
    pub eval: Option<String>,
    #[arg(long)]
    pub file: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub continue_on_error: bool,
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,
    #[arg(long, default_value_t = false)]
    pub quiet: bool,
    #[arg(long, default_value_t = false)]
    pub init: bool,
}

pub fn run() -> Result<i32, MiniDbError> {
    let cli = Cli::parse();
    if cli.init {
        Storage::init(&cli.db)?;
        return Ok(0);
    }
    let storage = Storage::open(&cli.db)?;
    let mut executor = Executor::new(storage);
    let repl = cli.repl || atty::is(Stream::Stdin);
    if let Some(sql) = cli.eval {
        return run_sql(&mut executor, &sql, cli.format, cli.quiet);
    }
    if let Some(path) = cli.file {
        let s = fs::read_to_string(path)?;
        return run_sql(&mut executor, &s, cli.format, cli.quiet);
    }
    if repl {
        return repl_loop(&mut executor, cli.format, cli.quiet);
    }
    Ok(0)
}

fn run_sql(
    executor: &mut Executor,
    sql: &str,
    fmt: OutputFormat,
    quiet: bool,
) -> Result<i32, MiniDbError> {
    let tokens = Lexer::new(sql).tokenize()?;
    let mut parser = Parser::new(tokens);
    for stmt in parser.parse_script()? {
        let res = executor.execute(stmt)?;
        let summary = emit(res, fmt.clone())?;
        if !quiet {
            eprintln!("{summary}");
        }
    }
    Ok(0)
}

fn repl_loop(executor: &mut Executor, fmt: OutputFormat, quiet: bool) -> Result<i32, MiniDbError> {
    let mut input = String::new();
    loop {
        input.clear();
        print!("minidb> ");
        io::stdout().flush().ok();
        if io::stdin().read_line(&mut input)? == 0 {
            return Ok(0);
        }
        if input.trim() == ".exit" {
            return Ok(0);
        }
        if input.trim() == ".tables" {
            println!(
                "{}",
                executor
                    .storage
                    .snapshot()
                    .catalog
                    .tables
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            continue;
        }
        if input.trim() == ".schema" {
            println!("{:#?}", executor.storage.snapshot().catalog);
            continue;
        }
        if input.trim() == ".indexes" {
            println!(
                "{}",
                executor
                    .storage
                    .snapshot()
                    .catalog
                    .indexes
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            continue;
        }
        if input.trim().starts_with(".format ") {
            continue;
        }
        if input.trim().is_empty() {
            continue;
        }
        match run_sql(executor, &input, fmt.clone(), quiet) {
            Ok(_) => {}
            Err(e) => eprintln!("{e}"),
        }
    }
}

fn emit(res: ExecResult, fmt: OutputFormat) -> Result<String, MiniDbError> {
    let summary = match &res {
        ExecResult::Rows { rows, .. } => format!("ok {} rows", rows.len()),
        ExecResult::Message(_, n) if *n == 1 => "ok 1 row affected".to_string(),
        ExecResult::Message(_, n) => format!("ok {n} rows affected"),
    };
    match res {
        ExecResult::Rows { columns, rows } => match fmt {
            OutputFormat::Table => {
                println!("{}", columns.join(" | "));
                for row in rows {
                    println!(
                        "{}",
                        row.into_iter()
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .join(" | ")
                    );
                }
            }
            OutputFormat::Csv => {
                let mut w = csv::Writer::from_writer(io::stdout());
                w.write_record(columns)
                    .map_err(|e| MiniDbError::cli(e.to_string()))?;
                for row in rows {
                    w.write_record(row.into_iter().map(|v| v.to_string()))
                        .map_err(|e| MiniDbError::cli(e.to_string()))?;
                }
                w.flush().map_err(|e| MiniDbError::cli(e.to_string()))?;
            }
            OutputFormat::Json => {
                let objs: Vec<serde_json::Value> = rows
                    .into_iter()
                    .map(|r| {
                        let map: serde_json::Map<String, serde_json::Value> = columns
                            .iter()
                            .cloned()
                            .zip(
                                r.into_iter()
                                    .map(|v| serde_json::Value::String(v.to_string())),
                            )
                            .collect();
                        serde_json::Value::Object(map)
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string(&objs).unwrap_or_else(|_| "[]".into())
                );
            }
        },
        ExecResult::Message(msg, _) => println!("{msg}"),
    }
    Ok(summary)
}
