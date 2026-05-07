use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::exec::{Executor, QueryContext, QueryResult};
use crate::lexer::Lexer;
use crate::parser::ast::*;
use crate::parser::parse;
use crate::plan::Planner;
use crate::storage::btree::BTreeIndex;
use crate::storage::heap::{self, HeapFile};
use crate::storage::wal::Wal;
use crate::storage::{PageId, Pager};
use crate::txn::TransactionManager;
use crate::types::*;
use log::debug;
use std::collections::HashMap;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct Config {
    pub db_path: PathBuf,
    pub mode: RunMode,
    pub format: OutputFormat,
    pub quiet: bool,
    pub continue_on_error: bool,
}

pub enum RunMode {
    Repl,
    Eval(String),
    File(PathBuf),
    Init,
}

#[derive(Debug, Clone, Copy)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

impl OutputFormat {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "table" => Some(Self::Table),
            "csv" => Some(Self::Csv),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

pub fn parse_args(args: Vec<String>) -> std::result::Result<Config, String> {
    let mut db_path = PathBuf::from("./minidb.db");
    let mut mode = None;
    let mut format = OutputFormat::Table;
    let mut quiet = false;
    let mut continue_on_error = false;
    let mut is_repl = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("minidb {VERSION}");
                std::process::exit(0);
            }
            "--db" => {
                i += 1;
                if i >= args.len() {
                    return Err("--db requires a path argument".into());
                }
                db_path = PathBuf::from(&args[i]);
            }
            "--repl" => {
                is_repl = true;
            }
            "--eval" | "-e" => {
                i += 1;
                if i >= args.len() {
                    return Err("--eval requires a SQL string".into());
                }
                mode = Some(RunMode::Eval(args[i].clone()));
            }
            "--file" | "-f" => {
                i += 1;
                if i >= args.len() {
                    return Err("--file requires a path argument".into());
                }
                mode = Some(RunMode::File(PathBuf::from(&args[i])));
            }
            "--init" => {
                mode = Some(RunMode::Init);
            }
            "--format" => {
                i += 1;
                if i >= args.len() {
                    return Err("--format requires an argument (table, csv, json)".into());
                }
                format = OutputFormat::from_str(&args[i])
                    .ok_or_else(|| format!("unknown format: {}", args[i]))?;
            }
            "--quiet" | "-q" => {
                quiet = true;
            }
            "--continue-on-error" => {
                continue_on_error = true;
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option: {other}"));
            }
            _ => {
                return Err(format!("unexpected argument: {}", args[i]));
            }
        }
        i += 1;
    }

    let mode = mode.unwrap_or_else(|| {
        if is_repl || atty_is_tty() {
            RunMode::Repl
        } else {
            RunMode::Repl // default to REPL; if stdin is not TTY, we'll read from stdin
        }
    });

    Ok(Config {
        db_path,
        mode,
        format,
        quiet,
        continue_on_error,
    })
}

fn atty_is_tty() -> bool {
    unsafe { libc_isatty(0) != 0 }
}

#[cfg(unix)]
fn libc_isatty(fd: i32) -> i32 {
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(fd) }
}

#[cfg(not(unix))]
fn libc_isatty(_fd: i32) -> i32 {
    0
}

fn print_help() {
    eprintln!("minidb {VERSION} - an embeddable SQL-subset relational database");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("    minidb [OPTIONS]");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("    --db <PATH>             Database file path (default: ./minidb.db)");
    eprintln!("    --repl                  Force interactive REPL mode");
    eprintln!("    --eval, -e <SQL>        Execute SQL and exit");
    eprintln!("    --file, -f <PATH>       Execute SQL script file");
    eprintln!("    --init                  Create fresh empty database and exit");
    eprintln!("    --format <FMT>          Output format: table, csv, json (default: table)");
    eprintln!("    --quiet, -q             Suppress per-statement summary lines");
    eprintln!("    --continue-on-error     Continue script execution on error");
    eprintln!("    --version, -V           Print version");
    eprintln!("    --help, -h              Print this help");
    eprintln!();
    eprintln!("META COMMANDS (in REPL):");
    eprintln!("    .schema                 Show all table schemas");
    eprintln!("    .tables                 List all tables");
    eprintln!("    .indexes                List all indexes");
    eprintln!("    .format <kind>          Change output format");
    eprintln!("    .exit                   Exit the REPL");
}

pub struct Engine {
    pub catalog: Catalog,
    pub heaps: HashMap<String, HeapFile>,
    pub indexes: HashMap<String, BTreeIndex>,
    pub txn: TransactionManager,
    pub pager: Arc<Mutex<Pager>>,
    pub db_path: PathBuf,
}

impl Engine {
    pub fn open(db_path: &PathBuf) -> Result<Self> {
        let pager = Pager::open(db_path)?;
        let pager = Arc::new(Mutex::new(pager));

        // WAL recovery
        let mut wal_path = db_path.clone();
        wal_path.set_extension("wal");
        if wal_path.exists() {
            let committed = Wal::replay(&wal_path)?;
            if !committed.is_empty() {
                log::info!(
                    "WAL recovery: replaying {} committed page writes",
                    committed.len()
                );
                // Apply committed page writes to the pager
                for rec in &committed {
                    if let crate::storage::wal::WalRecord::PageWrite { page_id, data, .. } = rec {
                        pager.lock().unwrap().write_page(PageId(*page_id), data)?;
                    }
                }
                // Truncate WAL after recovery
                Wal::truncate(&wal_path)?;
            }
        }

        let catalog = Catalog::load(pager.clone())?;

        // Initialize heap files and indexes from catalog
        let mut heaps = HashMap::new();
        for (name, entry) in &catalog.data.tables {
            let pages: Vec<PageId> = if entry.pages.is_empty() {
                vec![PageId(entry.root_page)]
            } else {
                entry.pages.iter().map(|p| PageId(*p)).collect()
            };
            let heap = HeapFile::from_pages(entry.table_id, pages, pager.clone());
            heaps.insert(name.clone(), heap);
        }

        let mut indexes = HashMap::new();
        for (name, entry) in &catalog.data.indexes {
            let idx = BTreeIndex::new(entry.index_id, PageId(entry.root_page), pager.clone());
            indexes.insert(name.clone(), idx);
        }

        let txn = TransactionManager::new(wal_path)?;

        Ok(Self {
            catalog,
            heaps,
            indexes,
            txn,
            pager,
            db_path: db_path.clone(),
        })
    }

    pub fn execute_sql(
        &mut self,
        sql: &str,
        format: OutputFormat,
        quiet: bool,
    ) -> Result<Vec<QueryResult>> {
        let tokens = Lexer::new(sql).tokenize()?;
        let stmts = parse(tokens)?;
        let mut results = Vec::new();

        for stmt in stmts {
            let result = self.execute_statement(stmt)?;
            // Flush dirty pages to disk after each statement
            self.flush_all_dirty()?;
            results.push(result);
        }

        Ok(results)
    }

    fn flush_all_dirty(&mut self) -> Result<()> {
        // Flush dirty pages and collect updated page lists
        let mut updates: Vec<(String, Vec<u64>)> = Vec::new();
        for (name, heap) in &mut self.heaps {
            heap.flush_dirty()?;
            let pages: Vec<u64> = heap.pages.iter().map(|p| p.0).collect();
            updates.push((name.clone(), pages));
            heap.clear_dirty();
        }
        // Persist page lists to catalog
        for (name, pages) in updates {
            self.catalog.update_table_pages(&name, pages)?;
        }
        self.pager.lock().unwrap().sync()?;
        Ok(())
    }

    fn execute_statement(&mut self, stmt: Statement) -> Result<QueryResult> {
        match stmt {
            Statement::CreateTable(ct) => {
                let schema = TableSchema {
                    name: ct.name.clone(),
                    columns: ct
                        .columns
                        .iter()
                        .map(|c| ColumnDef {
                            name: c.name.clone(),
                            col_type: c.col_type,
                            primary_key: c.primary_key,
                            not_null: c.not_null,
                            unique: c.unique,
                            default: c.default.clone(),
                        })
                        .collect(),
                    check_expr: None, // TODO: serialize check expr
                };
                let table_id = self.catalog.create_table(schema)?;
                let entry = self.catalog.get_table(&ct.name).unwrap().clone();
                let heap =
                    HeapFile::new(entry.table_id, PageId(entry.root_page), self.pager.clone());
                self.heaps.insert(ct.name.to_lowercase(), heap);
                Ok(QueryResult {
                    columns: vec![],
                    rows: vec![],
                    rows_affected: 0,
                })
            }
            Statement::DropTable { name } => {
                self.heaps.remove(&name.to_lowercase());
                self.catalog.drop_table(&name)?;
                Ok(QueryResult {
                    columns: vec![],
                    rows: vec![],
                    rows_affected: 0,
                })
            }
            Statement::CreateIndex(ci) => {
                let index_id =
                    self.catalog
                        .create_index(&ci.index_name, &ci.table_name, &ci.column_name)?;
                let entry = self.catalog.get_index(&ci.index_name).unwrap().clone();
                let idx =
                    BTreeIndex::new(entry.index_id, PageId(entry.root_page), self.pager.clone());
                self.indexes.insert(ci.index_name.to_lowercase(), idx);
                Ok(QueryResult {
                    columns: vec![],
                    rows: vec![],
                    rows_affected: 0,
                })
            }
            Statement::DropIndex { name } => {
                self.indexes.remove(&name.to_lowercase());
                self.catalog.drop_index(&name)?;
                Ok(QueryResult {
                    columns: vec![],
                    rows: vec![],
                    rows_affected: 0,
                })
            }
            Statement::Select(sel) => {
                let planner = Planner::new(&self.catalog);
                let plan = planner.plan_select(&sel)?;
                let mut ctx = QueryContext {
                    catalog: &mut self.catalog,
                    heaps: &mut self.heaps,
                    indexes: &mut self.indexes,
                    txn: &mut self.txn,
                    pager: self.pager.clone(),
                };
                Executor::execute_plan(plan, &mut ctx)
            }
            Statement::Insert(ins) => {
                let planner = Planner::new(&self.catalog);
                let plan = crate::plan::physical::PhysicalPlan::Insert {
                    table_name: ins.table.clone(),
                    table_id: self
                        .catalog
                        .get_table(&ins.table)
                        .map(|t| t.table_id)
                        .unwrap_or(0),
                    columns: ins.columns,
                    values: ins.values,
                };
                let mut ctx = QueryContext {
                    catalog: &mut self.catalog,
                    heaps: &mut self.heaps,
                    indexes: &mut self.indexes,
                    txn: &mut self.txn,
                    pager: self.pager.clone(),
                };
                Executor::execute_plan(plan, &mut ctx)
            }
            Statement::Update(upd) => {
                let table_entry = self
                    .catalog
                    .get_table(&upd.table)
                    .ok_or_else(|| MiniDbError::TableNotFound(upd.table.clone()))?;
                let filter_plan = upd.where_clause.map(|pred| {
                    let schema = &table_entry.schema;
                    Box::new(crate::plan::physical::PhysicalPlan::Filter {
                        input: Box::new(crate::plan::physical::PhysicalPlan::SeqScan {
                            table_name: upd.table.clone(),
                            table_id: table_entry.table_id,
                            root_page: table_entry.root_page,
                            alias: None,
                            schema: schema.clone(),
                        }),
                        predicate: pred,
                    })
                });
                let plan = crate::plan::physical::PhysicalPlan::Update {
                    table_name: upd.table.clone(),
                    table_id: table_entry.table_id,
                    assignments: upd.assignments,
                    filter: filter_plan,
                };
                let mut ctx = QueryContext {
                    catalog: &mut self.catalog,
                    heaps: &mut self.heaps,
                    indexes: &mut self.indexes,
                    txn: &mut self.txn,
                    pager: self.pager.clone(),
                };
                Executor::execute_plan(plan, &mut ctx)
            }
            Statement::Delete(del) => {
                let table_entry = self
                    .catalog
                    .get_table(&del.table)
                    .ok_or_else(|| MiniDbError::TableNotFound(del.table.clone()))?;
                let filter_plan = del.where_clause.map(|pred| {
                    let schema = &table_entry.schema;
                    Box::new(crate::plan::physical::PhysicalPlan::Filter {
                        input: Box::new(crate::plan::physical::PhysicalPlan::SeqScan {
                            table_name: del.table.clone(),
                            table_id: table_entry.table_id,
                            root_page: table_entry.root_page,
                            alias: None,
                            schema: schema.clone(),
                        }),
                        predicate: pred,
                    })
                });
                let plan = crate::plan::physical::PhysicalPlan::Delete {
                    table_name: del.table.clone(),
                    table_id: table_entry.table_id,
                    filter: filter_plan,
                };
                let mut ctx = QueryContext {
                    catalog: &mut self.catalog,
                    heaps: &mut self.heaps,
                    indexes: &mut self.indexes,
                    txn: &mut self.txn,
                    pager: self.pager.clone(),
                };
                Executor::execute_plan(plan, &mut ctx)
            }
            Statement::Begin => {
                self.txn.begin()?;
                Ok(QueryResult {
                    columns: vec![],
                    rows: vec![],
                    rows_affected: 0,
                })
            }
            Statement::Commit => {
                // Flush dirty pages and persist page lists
                let mut updates: Vec<(String, Vec<u64>)> = Vec::new();
                for (name, heap) in &mut self.heaps {
                    heap.flush_dirty()?;
                    let pages: Vec<u64> = heap.pages.iter().map(|p| p.0).collect();
                    updates.push((name.clone(), pages));
                    heap.clear_dirty();
                }
                for (name, pages) in updates {
                    self.catalog.update_table_pages(&name, pages)?;
                }
                self.txn.commit()?;
                Ok(QueryResult {
                    columns: vec![],
                    rows: vec![],
                    rows_affected: 0,
                })
            }
            Statement::Rollback => {
                for heap in self.heaps.values_mut() {
                    heap.discard_dirty();
                }
                self.txn.rollback()?;
                Ok(QueryResult {
                    columns: vec![],
                    rows: vec![],
                    rows_affected: 0,
                })
            }
            Statement::MetaCommand(mc) => {
                match mc {
                    MetaCommand::Schema => {
                        let mut rows = Vec::new();
                        for (_, entry) in &self.catalog.data.tables {
                            let mut col_strs = Vec::new();
                            for col in &entry.schema.columns {
                                let mut parts = vec![format!("{} {}", col.name, col.col_type)];
                                if col.primary_key {
                                    parts.push("PRIMARY KEY".into());
                                }
                                if col.not_null && !col.primary_key {
                                    parts.push("NOT NULL".into());
                                }
                                if col.unique && !col.primary_key {
                                    parts.push("UNIQUE".into());
                                }
                                if let Some(ref d) = col.default {
                                    parts.push(format!("DEFAULT {d}"));
                                }
                                col_strs.push(parts.join(" "));
                            }
                            let create = format!(
                                "CREATE TABLE {} ({});",
                                entry.schema.name,
                                col_strs.join(", ")
                            );
                            rows.push(vec![Value::Text(create)]);
                        }
                        Ok(QueryResult {
                            columns: vec!["schema".into()],
                            rows,
                            rows_affected: 0,
                        })
                    }
                    MetaCommand::Tables => {
                        let rows: Vec<Row> = self
                            .catalog
                            .data
                            .tables
                            .keys()
                            .map(|k| vec![Value::Text(k.clone())])
                            .collect();
                        Ok(QueryResult {
                            columns: vec!["name".into()],
                            rows,
                            rows_affected: 0,
                        })
                    }
                    MetaCommand::Indexes => {
                        let rows: Vec<Row> = self
                            .catalog
                            .data
                            .indexes
                            .values()
                            .map(|idx| {
                                vec![
                                    Value::Text(idx.schema.name.clone()),
                                    Value::Text(idx.schema.table_name.clone()),
                                    Value::Text(idx.schema.column_name.clone()),
                                ]
                            })
                            .collect();
                        Ok(QueryResult {
                            columns: vec!["name".into(), "table".into(), "column".into()],
                            rows,
                            rows_affected: 0,
                        })
                    }
                    MetaCommand::Exit => {
                        std::process::exit(0);
                    }
                    MetaCommand::Format(_) => {
                        // Handled at caller level
                        Ok(QueryResult {
                            columns: vec![],
                            rows: vec![],
                            rows_affected: 0,
                        })
                    }
                }
            }
        }
    }
}

pub fn format_result(
    result: &QueryResult,
    format: OutputFormat,
    writer: &mut dyn Write,
) -> io::Result<()> {
    match format {
        OutputFormat::Table => format_table(result, writer),
        OutputFormat::Csv => format_csv(result, writer),
        OutputFormat::Json => format_json(result, writer),
    }
}

fn format_table(result: &QueryResult, writer: &mut dyn Write) -> io::Result<()> {
    if result.columns.is_empty() && result.rows.is_empty() {
        return Ok(());
    }

    let ncols = if result.columns.is_empty() {
        result.rows.first().map(|r| r.len()).unwrap_or(0)
    } else {
        result.columns.len()
    };

    // Calculate column widths
    let mut widths = vec![0usize; ncols];
    for (i, col) in result.columns.iter().enumerate() {
        if i < widths.len() {
            widths[i] = widths[i].max(col.len());
        }
    }
    for row in &result.rows {
        for (i, val) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(format!("{val}").len());
            }
        }
    }

    // Header
    if !result.columns.is_empty() {
        for (i, col) in result.columns.iter().enumerate() {
            if i > 0 {
                write!(writer, " | ")?;
            }
            write!(
                writer,
                "{:<width$}",
                col,
                width = widths.get(i).copied().unwrap_or(0)
            )?;
        }
        writeln!(writer)?;
        for (i, _) in result.columns.iter().enumerate() {
            if i > 0 {
                write!(writer, "-+-")?;
            }
            let w = widths.get(i).copied().unwrap_or(0);
            for _ in 0..w {
                write!(writer, "-")?;
            }
        }
        writeln!(writer)?;
    }

    // Rows
    for row in &result.rows {
        for (i, val) in row.iter().enumerate() {
            if i > 0 {
                write!(writer, " | ")?;
            }
            let w = widths.get(i).copied().unwrap_or(0);
            write!(writer, "{:<width$}", val, width = w)?;
        }
        writeln!(writer)?;
    }

    Ok(())
}

fn format_csv(result: &QueryResult, writer: &mut dyn Write) -> io::Result<()> {
    // Header
    if !result.columns.is_empty() {
        for (i, col) in result.columns.iter().enumerate() {
            if i > 0 {
                write!(writer, ",")?;
            }
            write_csv_field(writer, col)?;
        }
        writeln!(writer)?;
    }

    // Rows
    for row in &result.rows {
        for (i, val) in row.iter().enumerate() {
            if i > 0 {
                write!(writer, ",")?;
            }
            write_csv_field(writer, &format!("{val}"))?;
        }
        writeln!(writer)?;
    }

    Ok(())
}

fn write_csv_field(writer: &mut dyn Write, field: &str) -> io::Result<()> {
    if field.contains(',') || field.contains('"') || field.contains('\n') {
        write!(writer, "\"{}\"", field.replace('"', "\"\""))
    } else {
        write!(writer, "{field}")
    }
}

fn format_json(result: &QueryResult, writer: &mut dyn Write) -> io::Result<()> {
    let rows: Vec<serde_json::Value> = result
        .rows
        .iter()
        .map(|row| {
            let mut obj = serde_json::Map::new();
            for (i, val) in row.iter().enumerate() {
                let key = result
                    .columns
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| format!("col{i}"));
                let json_val = match val {
                    Value::Null => serde_json::Value::Null,
                    Value::Int(n) => serde_json::Value::Number((*n).into()),
                    Value::Real(r) => serde_json::Number::from_f64(*r)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null),
                    Value::Text(s) => serde_json::Value::String(s.clone()),
                    Value::Bool(b) => serde_json::Value::Bool(*b),
                };
                obj.insert(key, json_val);
            }
            serde_json::Value::Object(obj)
        })
        .collect();

    let json = serde_json::to_string(&rows).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    writeln!(writer, "{json}")
}

pub fn run(config: Config) -> i32 {
    match run_inner(&config) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn run_inner(config: &Config) -> Result<i32> {
    match &config.mode {
        RunMode::Init => {
            let _ = Engine::open(&config.db_path)?;
            return Ok(0);
        }
        _ => {}
    }

    let mut engine = Engine::open(&config.db_path)?;
    let mut format = config.format;

    match &config.mode {
        RunMode::Eval(sql) => {
            let results = engine.execute_sql(sql, format, config.quiet)?;
            for result in &results {
                format_result(result, format, &mut io::stdout())?;
                if !config.quiet {
                    print_summary(result, &mut io::stderr());
                }
            }
            Ok(0)
        }
        RunMode::File(path) => {
            let content = std::fs::read_to_string(path)?;
            let tokens = Lexer::new(&content).tokenize()?;
            let stmts = parse(tokens)?;
            for stmt in stmts {
                match engine.execute_statement(stmt) {
                    Ok(result) => {
                        engine.flush_all_dirty()?;
                        format_result(&result, format, &mut io::stdout())?;
                        if !config.quiet {
                            print_summary(&result, &mut io::stderr());
                        }
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        if !config.continue_on_error {
                            return Ok(1);
                        }
                    }
                }
            }
            Ok(0)
        }
        RunMode::Repl => {
            let stdin = io::stdin();
            let mut stdout = io::stdout();
            let mut stderr = io::stderr();

            if atty_is_tty() {
                eprintln!("minidb {VERSION}");
                eprintln!("Type .exit to quit, .schema for table info.");
            }

            let mut buffer = String::new();
            loop {
                if atty_is_tty() {
                    eprint!("minidb> ");
                    stderr.flush().ok();
                }

                buffer.clear();
                let bytes_read = stdin.lock().read_line(&mut buffer)?;
                if bytes_read == 0 {
                    // EOF
                    return Ok(0);
                }

                let trimmed = buffer.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Handle meta-format command specially
                if trimmed.starts_with(".format ") {
                    let kind = trimmed.strip_prefix(".format ").unwrap().trim();
                    if let Some(f) = OutputFormat::from_str(kind) {
                        format = f;
                        eprintln!("Output format set to: {kind}");
                    } else {
                        eprintln!("error: unknown format: {kind}");
                    }
                    continue;
                }

                // Check if statement is complete (ends with ;)
                let sql = if trimmed.ends_with(';') {
                    trimmed.to_string()
                } else {
                    // Read more lines until we get a semicolon
                    let mut full = trimmed.to_string();
                    loop {
                        if atty_is_tty() {
                            eprint!("   ...> ");
                            stderr.flush().ok();
                        }
                        let mut line = String::new();
                        let n = stdin.lock().read_line(&mut line)?;
                        if n == 0 {
                            break;
                        }
                        full.push(' ');
                        full.push_str(line.trim());
                        if line.trim().ends_with(';') {
                            break;
                        }
                    }
                    full
                };

                match engine.execute_sql(&sql, format, config.quiet) {
                    Ok(results) => {
                        for result in &results {
                            format_result(result, format, &mut stdout)?;
                            stdout.flush().ok();
                            if !config.quiet {
                                print_summary(result, &mut stderr);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                    }
                }
            }
        }
        RunMode::Init => unreachable!(),
    }
}

fn print_summary(result: &QueryResult, writer: &mut dyn Write) {
    if result.rows_affected > 0 {
        let s = if result.rows_affected == 1 { "" } else { "s" };
        writeln!(writer, "ok {} row{s} affected", result.rows_affected).ok();
    } else if !result.rows.is_empty() {
        let s = if result.rows.len() == 1 { "" } else { "s" };
        writeln!(writer, "ok {} row{s}", result.rows.len()).ok();
    } else {
        writeln!(writer, "ok").ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_table() {
        let result = QueryResult {
            columns: vec!["id".into(), "name".into()],
            rows: vec![
                vec![Value::Int(1), Value::Text("Alice".into())],
                vec![Value::Int(2), Value::Text("Bob".into())],
            ],
            rows_affected: 0,
        };
        let mut buf = Vec::new();
        format_table(&result, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Alice"));
        assert!(output.contains("Bob"));
    }

    #[test]
    fn test_format_csv() {
        let result = QueryResult {
            columns: vec!["a".into()],
            rows: vec![vec![Value::Text("hello, world".into())]],
            rows_affected: 0,
        };
        let mut buf = Vec::new();
        format_csv(&result, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("\"hello, world\""));
    }

    #[test]
    fn test_format_json() {
        let result = QueryResult {
            columns: vec!["x".into()],
            rows: vec![vec![Value::Int(42)]],
            rows_affected: 0,
        };
        let mut buf = Vec::new();
        format_json(&result, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("42"));
    }
}
