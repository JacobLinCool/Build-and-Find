use crate::{
    error::Result,
    exec::{Executor, Output},
    parser::{
        Parser,
        ast::{MetaCommand, Statement},
    },
    plan::lower,
    storage::Storage,
    txn::TransactionManager,
};
use clap::Parser as ClapParser;
use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::PathBuf,
};

#[derive(ClapParser, Debug)]
#[command(name = "minidb", version, about = "Embeddable SQL-subset database")]
pub struct Args {
    #[arg(long)]
    pub db: Option<PathBuf>,
    #[arg(long)]
    pub repl: bool,
    #[arg(long)]
    pub eval: Option<String>,
    #[arg(long)]
    pub file: Option<PathBuf>,
    #[arg(long)]
    pub continue_on_error: bool,
    #[arg(long, default_value = "table")]
    pub format: String,
    #[arg(long)]
    pub init: bool,
    #[arg(long)]
    pub quiet: bool,
}

pub fn run() -> Result<i32> {
    let args = Args::parse();
    let db = args.db.unwrap_or_else(|| PathBuf::from("minidb.db"));
    if args.init {
        Storage::init(&db)?;
        return Ok(0);
    }
    let (storage, mut catalog) = Storage::open(&db)?;
    let mut txn = TransactionManager::new();
    let mut format = args.format.clone();
    if let Some(sql) = args.eval {
        return run_sql(
            &storage,
            &mut txn,
            &mut catalog,
            &sql,
            &mut format,
            args.quiet,
        )
        .map(|_| 0);
    }
    if let Some(path) = args.file {
        let sql = fs::read_to_string(path)?;
        return run_sql(
            &storage,
            &mut txn,
            &mut catalog,
            &sql,
            &mut format,
            args.quiet,
        )
        .map(|_| 0);
    }
    let repl = args.repl || io::stdin().is_terminal();
    if repl {
        run_repl(&storage, &mut txn, &mut catalog, &mut format, args.quiet)?;
        return Ok(0);
    }
    Ok(2)
}

fn run_sql(
    storage: &Storage,
    txn: &mut TransactionManager,
    catalog: &mut crate::catalog::Catalog,
    sql: &str,
    format: &mut String,
    quiet: bool,
) -> Result<()> {
    let mut parser = Parser::new(sql)?;
    for stmt in parser.parse_statements()? {
        if let Statement::Meta(MetaCommand::Format(f)) = &stmt {
            *format = f.clone();
            continue;
        }
        let out = Executor::execute(storage, txn, catalog, lower(stmt))?;
        emit(out, format, quiet)?;
    }
    Ok(())
}

fn run_repl(
    storage: &Storage,
    txn: &mut TransactionManager,
    catalog: &mut crate::catalog::Catalog,
    format: &mut String,
    quiet: bool,
) -> Result<()> {
    let mut buf = String::new();
    let mut input = String::new();
    loop {
        print!("minidb> ");
        io::stdout().flush()?;
        input.clear();
        if io::stdin().read_line(&mut input)? == 0 {
            break;
        }
        buf.push_str(&input);
        if buf.contains(';') {
            run_sql(storage, txn, catalog, &buf, format, quiet)?;
            buf.clear();
        }
    }
    Ok(())
}

fn emit(out: Output, format: &str, quiet: bool) -> Result<()> {
    match out {
        Output::Rows(rows, schema) => match format {
            "csv" => {
                println!("{}", csv(&rows));
            }
            "json" => {
                println!("{}", serde_json::to_string(&rows)?);
            }
            _ => {
                println!("{}", table(&rows, &schema));
            }
        },
        Output::Affected(n) => {
            println!("");
            if !quiet {
                eprintln!("ok {} row{}", n, if n == 1 { "" } else { "s" });
            }
        }
        Output::Message(_) => {
            if !quiet {
                eprintln!("ok");
            }
        }
    }
    Ok(())
}

fn table(rows: &[Vec<crate::types::Value>], _schema: &[String]) -> String {
    rows.iter()
        .map(|r| {
            r.iter()
                .map(|v| format!("{v:?}"))
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn csv(rows: &[Vec<crate::types::Value>]) -> String {
    rows.iter()
        .map(|r| {
            r.iter()
                .map(|v| format!("{v:?}"))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
