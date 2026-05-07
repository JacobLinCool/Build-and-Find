/// CLI: REPL, batch eval, file execution, output formatting.
use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::exec::{execute, ExecResult, ExecutionContext};
use crate::parser::{ast::Statement, Parser};
use crate::storage::{Pager, Wal, WalRecord};
use crate::txn::TransactionManager;
use crate::types::{Row, Schema, Value};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "table" => Ok(OutputFormat::Table),
            "csv" => Ok(OutputFormat::Csv),
            "json" => Ok(OutputFormat::Json),
            _ => Err(format!("unknown format: {}", s)),
        }
    }
}

pub struct Session {
    pub ctx: ExecutionContext,
    pub format: OutputFormat,
    pub quiet: bool,
    pub db_path: PathBuf,
}

impl Session {
    pub fn open(db_path: impl AsRef<Path>, quiet: bool) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        let wal_path = db_path.with_extension("wal");
        let cat_path = db_path.with_extension("catalog");

        let mut pager = Pager::open(&db_path)?;

        // Load or create catalog
        let mut catalog = if cat_path.exists() {
            let data = std::fs::read(&cat_path)?;
            Catalog::deserialize(&data)?
        } else {
            Catalog::new()
        };

        let wal = Wal::open(&wal_path)?;
        let txn = TransactionManager::new(wal);

        let mut ctx = ExecutionContext::new(pager, catalog, txn);

        // Replay WAL on startup
        ctx = replay_wal(ctx, &wal_path, &db_path)?;

        Ok(Session {
            ctx,
            format: OutputFormat::Table,
            quiet,
            db_path,
        })
    }

    pub fn set_format(&mut self, fmt: OutputFormat) {
        self.format = fmt;
    }

    pub fn execute_sql(&mut self, sql: &str) -> Result<Vec<(ExecResult, Option<usize>)>> {
        let stmts = Parser::parse_from_str(sql)?;
        let mut results = Vec::new();
        for stmt in &stmts {
            match stmt {
                Statement::MetaExit => {
                    results.push((ExecResult::Done, None));
                    return Ok(results);
                }
                Statement::MetaFormat(fmt) => {
                    let f: OutputFormat =
                        fmt.parse().map_err(|e: String| MiniDbError::Runtime(e))?;
                    self.format = f;
                    results.push((ExecResult::Done, None));
                }
                _ => {
                    let res = execute(stmt, &mut self.ctx)?;
                    results.push((res, None));
                }
            }
        }
        Ok(results)
    }

    pub fn print_result(&self, result: &ExecResult, schema_override: Option<&Schema>) {
        match result {
            ExecResult::Rows { schema, rows } => {
                let s = schema_override.unwrap_or(schema);
                match self.format {
                    OutputFormat::Table => print_table(s, rows),
                    OutputFormat::Csv => print_csv(s, rows),
                    OutputFormat::Json => print_json(s, rows),
                }
            }
            ExecResult::RowsAffected(n) => {
                if !self.quiet {
                    eprintln!("ok {} row{} affected", n, if *n == 1 { "" } else { "s" });
                }
            }
            ExecResult::Done => {
                if !self.quiet {
                    eprintln!("ok");
                }
            }
        }
    }

    pub fn print_summary(&self, result: &ExecResult) {
        if self.quiet {
            return;
        }
        match result {
            ExecResult::Rows { rows, .. } => {
                eprintln!(
                    "ok {} row{}",
                    rows.len(),
                    if rows.len() == 1 { "" } else { "s" }
                );
            }
            ExecResult::RowsAffected(n) => {
                eprintln!("ok {} row{} affected", n, if *n == 1 { "" } else { "s" });
            }
            ExecResult::Done => {
                eprintln!("ok");
            }
        }
    }

    pub fn handle_meta_command(&self, stmt: &Statement) -> Option<String> {
        match stmt {
            Statement::MetaSchema => {
                let mut output = String::new();
                for name in self.ctx.catalog.table_names() {
                    if let Ok(t) = self.ctx.catalog.get_table(&name) {
                        output.push_str(&format!("CREATE TABLE {} (\n", t.name));
                        let cols: Vec<String> = t
                            .schema
                            .columns
                            .iter()
                            .map(|c| {
                                let mut def = format!("  {} {}", c.name, c.data_type);
                                if c.primary_key {
                                    def.push_str(" PRIMARY KEY");
                                } else if !c.nullable {
                                    def.push_str(" NOT NULL");
                                }
                                if c.unique && !c.primary_key {
                                    def.push_str(" UNIQUE");
                                }
                                if let Some(d) = &c.default {
                                    def.push_str(&format!(" DEFAULT {}", d));
                                }
                                def
                            })
                            .collect();
                        output.push_str(&cols.join(",\n"));
                        output.push_str("\n);\n");
                    }
                }
                Some(output)
            }
            Statement::MetaTables => {
                let names = self.ctx.catalog.table_names();
                Some(names.join("\n"))
            }
            Statement::MetaIndexes => {
                let names = self.ctx.catalog.all_index_names();
                let mut output = String::new();
                for name in names {
                    if let Ok(idx) = self.ctx.catalog.get_index(&name) {
                        output.push_str(&format!(
                            "{} ON {} ({})\n",
                            idx.name,
                            idx.table,
                            idx.columns.join(", ")
                        ));
                    }
                }
                Some(output)
            }
            _ => None,
        }
    }

    pub fn save_catalog(&self) -> Result<()> {
        let cat_path = self.db_path.with_extension("catalog");
        let data = self.ctx.catalog.serialize()?;
        std::fs::write(&cat_path, &data)?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.ctx.pager.flush_all()?;
        self.save_catalog()?;
        // Truncate WAL after successful flush — all data is safely on disk.
        // On next startup, WAL is empty so no double-replay occurs.
        self.ctx.txn.wal_mut().truncate()?;
        Ok(())
    }
}

fn replay_wal(
    mut ctx: ExecutionContext,
    wal_path: &Path,
    db_path: &Path,
) -> Result<ExecutionContext> {
    if !wal_path.exists() {
        return Ok(ctx);
    }

    let wal = Wal::open(wal_path)?;
    let records = wal.replay()?;
    if records.is_empty() {
        return Ok(ctx);
    }

    log::info!("replaying {} WAL records", records.len());

    let committed_ops = crate::storage::wal::collect_committed_ops(&records);

    for record in committed_ops {
        match record {
            WalRecord::CatalogChange { payload, .. } => {
                ctx.catalog = Catalog::deserialize(payload)?;
            }
            WalRecord::InsertRow {
                table,
                row_id,
                data,
                ..
            } => {
                if let Ok(table_meta) = ctx.catalog.get_table(table) {
                    let heap_root = crate::storage::PageId(table_meta.heap_root);
                    let heap = crate::storage::Heap::open(heap_root);
                    let _ = heap.insert_row(&mut ctx.pager, data);
                }
            }
            WalRecord::DeleteRow { table, row_id, .. } => {
                if let Ok(table_meta) = ctx.catalog.get_table(table) {
                    let heap_root = crate::storage::PageId(table_meta.heap_root);
                    let heap = crate::storage::Heap::open(heap_root);
                    let rid = *row_id;
                    let mut target: Option<(crate::storage::PageId, u16)> = None;
                    let _ = heap.scan(&mut ctx.pager, |pid, slot, data| {
                        if data.len() >= 8 {
                            let r = u64::from_le_bytes(data[..8].try_into().unwrap());
                            if r == rid {
                                target = Some((pid, slot));
                                return Ok(false);
                            }
                        }
                        Ok(true)
                    });
                    if let Some((pid, slot)) = target {
                        let _ = heap.delete_row(&mut ctx.pager, pid, slot);
                    }
                }
            }
            WalRecord::UpdateRow {
                table,
                row_id,
                new_data,
                ..
            } => {
                if let Ok(table_meta) = ctx.catalog.get_table(table) {
                    let heap_root = crate::storage::PageId(table_meta.heap_root);
                    let heap = crate::storage::Heap::open(heap_root);
                    let rid = *row_id;
                    let nd = new_data.clone();
                    let mut target: Option<(crate::storage::PageId, u16)> = None;
                    let _ = heap.scan(&mut ctx.pager, |pid, slot, data| {
                        if data.len() >= 8 {
                            let r = u64::from_le_bytes(data[..8].try_into().unwrap());
                            if r == rid {
                                target = Some((pid, slot));
                                return Ok(false);
                            }
                        }
                        Ok(true)
                    });
                    if let Some((pid, slot)) = target {
                        let _ = heap.update_row(&mut ctx.pager, pid, slot, &nd);
                    }
                }
            }
            _ => {}
        }
    }

    ctx.pager.flush_all()?;
    Ok(ctx)
}

/// Run the interactive REPL
pub fn run_repl(session: &mut Session) -> Result<()> {
    let mut rl =
        rustyline::DefaultEditor::new().map_err(|e| MiniDbError::Runtime(e.to_string()))?;

    println!("minidb v{}", env!("CARGO_PKG_VERSION"));
    println!("Enter SQL statements terminated by ';'. Type '.exit' to quit.");

    let mut buffer = String::new();

    loop {
        let prompt = if buffer.is_empty() {
            "minidb> "
        } else {
            "     -> "
        };
        let readline = rl.readline(prompt);

        match readline {
            Ok(line) => {
                let _ = rl.add_history_entry(line.as_str());
                buffer.push_str(&line);
                buffer.push('\n');

                // Check for meta commands first (they don't need semicolons)
                let trimmed = buffer.trim();
                if trimmed.starts_with('.') {
                    let stmts = Parser::parse_from_str(trimmed);
                    match stmts {
                        Ok(stmts) => {
                            for stmt in &stmts {
                                match stmt {
                                    Statement::MetaExit => {
                                        session.flush()?;
                                        println!("bye");
                                        return Ok(());
                                    }
                                    Statement::MetaSchema
                                    | Statement::MetaTables
                                    | Statement::MetaIndexes => {
                                        if let Some(output) = session.handle_meta_command(stmt) {
                                            print!("{}", output);
                                        }
                                    }
                                    Statement::MetaFormat(fmt) => {
                                        match fmt.parse::<OutputFormat>() {
                                            Ok(f) => session.format = f,
                                            Err(e) => eprintln!("Error: {}", e),
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Err(e) => eprintln!("Error: {}", e),
                    }
                    buffer.clear();
                    continue;
                }

                // Process complete statements (terminated by semicolons)
                while let Some(semi_pos) = find_statement_end(&buffer) {
                    let stmt_sql = buffer[..semi_pos + 1].trim().to_string();
                    buffer = buffer[semi_pos + 1..].to_string();

                    if stmt_sql.is_empty() || stmt_sql == ";" {
                        continue;
                    }

                    match Parser::parse_from_str(&stmt_sql) {
                        Err(e) => {
                            eprintln!("Error: {}", e);
                        }
                        Ok(stmts) => {
                            for stmt in &stmts {
                                match stmt {
                                    Statement::MetaExit => {
                                        session.flush()?;
                                        println!("bye");
                                        return Ok(());
                                    }
                                    Statement::MetaSchema
                                    | Statement::MetaTables
                                    | Statement::MetaIndexes => {
                                        if let Some(output) = session.handle_meta_command(stmt) {
                                            print!("{}", output);
                                        }
                                    }
                                    Statement::MetaFormat(fmt) => {
                                        match fmt.parse::<OutputFormat>() {
                                            Ok(f) => session.format = f,
                                            Err(e) => eprintln!("Error: {}", e),
                                        }
                                    }
                                    _ => match execute(stmt, &mut session.ctx) {
                                        Ok(result) => {
                                            session.print_result(&result, None);
                                            session.print_summary(&result);
                                            let _ = session.flush();
                                        }
                                        Err(e) => eprintln!("Error: {}", e),
                                    },
                                }
                            }
                        }
                    }
                }
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                session.flush()?;
                return Ok(());
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                buffer.clear();
                eprintln!("^C");
            }
            Err(e) => {
                return Err(MiniDbError::Runtime(e.to_string()));
            }
        }
    }
}

/// Run batch mode: execute SQL from string
pub fn run_eval(
    session: &mut Session,
    sql: &str,
    continue_on_error: bool,
) -> std::result::Result<(), i32> {
    let stmts = match Parser::parse_from_str(sql) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Parse error: {}", e);
            return Err(1);
        }
    };

    for stmt in &stmts {
        match stmt {
            Statement::MetaExit => break,
            Statement::MetaSchema | Statement::MetaTables | Statement::MetaIndexes => {
                if let Some(output) = session.handle_meta_command(stmt) {
                    print!("{}", output);
                }
            }
            Statement::MetaFormat(fmt) => match fmt.parse::<OutputFormat>() {
                Ok(f) => session.format = f,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    if !continue_on_error {
                        return Err(1);
                    }
                }
            },
            _ => match execute(stmt, &mut session.ctx) {
                Ok(result) => {
                    session.print_result(&result, None);
                    session.print_summary(&result);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    if !continue_on_error {
                        return Err(1);
                    }
                }
            },
        }
    }

    let _ = session.flush();
    Ok(())
}

/// Run batch mode: execute SQL from file
pub fn run_file(
    session: &mut Session,
    path: &Path,
    continue_on_error: bool,
) -> std::result::Result<(), i32> {
    let sql = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", path.display(), e);
            return Err(3);
        }
    };
    run_eval(session, &sql, continue_on_error)
}

/// Find the end of the first complete statement (position of ';')
fn find_statement_end(s: &str) -> Option<usize> {
    let mut in_string = false;
    let mut string_char = '"';
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            if c == string_char {
                // Check for escaped quote (doubled)
                if i + 1 < chars.len() && chars[i + 1] == string_char {
                    i += 2;
                    continue;
                }
                in_string = false;
            }
        } else {
            match c {
                '\'' | '"' => {
                    in_string = true;
                    string_char = c;
                }
                ';' => return Some(i),
                '-' if i + 1 < chars.len() && chars[i + 1] == '-' => {
                    // Skip to end of line
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    continue;
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

// ─── Output formatting ─────────────────────────────────────────────────────────

fn print_table(schema: &Schema, rows: &[Row]) {
    // Compute column widths
    let mut widths: Vec<usize> = schema.columns.iter().map(|c| c.name.len()).collect();
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(val.to_string().len());
            }
        }
    }

    // Header
    let header: Vec<String> = schema
        .columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            format!(
                "{:width$}",
                c.name,
                width = widths.get(i).copied().unwrap_or(c.name.len())
            )
        })
        .collect();
    println!("| {} |", header.join(" | "));

    // Separator
    let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    println!("|-{}-|", sep.join("-|-"));

    // Rows
    for row in rows {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, v)| {
                format!(
                    "{:width$}",
                    v.to_string(),
                    width = widths.get(i).copied().unwrap_or(0)
                )
            })
            .collect();
        println!("| {} |", cells.join(" | "));
    }
}

fn print_csv(schema: &Schema, rows: &[Row]) {
    // Header
    let header: Vec<String> = schema.columns.iter().map(|c| csv_escape(&c.name)).collect();
    println!("{}", header.join(","));

    // Rows
    for row in rows {
        let cells: Vec<String> = row.iter().map(|v| csv_escape(&v.to_string())).collect();
        println!("{}", cells.join(","));
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn print_json(schema: &Schema, rows: &[Row]) {
    let json_rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let mut obj = serde_json::Map::new();
            for (i, col) in schema.columns.iter().enumerate() {
                if i < row.len() {
                    let json_val = match &row[i] {
                        crate::types::Value::Null => serde_json::Value::Null,
                        crate::types::Value::Bool(b) => serde_json::Value::Bool(*b),
                        crate::types::Value::Int(n) => serde_json::json!(n),
                        crate::types::Value::Real(r) => serde_json::Number::from_f64(*r)
                            .map(serde_json::Value::Number)
                            .unwrap_or(serde_json::Value::Null),
                        crate::types::Value::Text(s) => serde_json::Value::String(s.clone()),
                    };
                    obj.insert(col.name.clone(), json_val);
                }
            }
            serde_json::Value::Object(obj)
        })
        .collect();

    println!(
        "{}",
        serde_json::to_string_pretty(&json_rows).unwrap_or_default()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_session_open() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("test.db");
        let session = Session::open(&db, false).unwrap();
        assert_eq!(session.format, OutputFormat::Table);
    }

    #[test]
    fn test_find_statement_end() {
        assert_eq!(find_statement_end("SELECT 1;"), Some(8));
        assert_eq!(find_statement_end("SELECT 1"), None);
        assert_eq!(find_statement_end("SELECT 'a;b';"), Some(12));
    }

    #[test]
    fn test_csv_escape() {
        assert_eq!(csv_escape("hello"), "hello");
        assert_eq!(csv_escape("he,lo"), "\"he,lo\"");
        assert_eq!(csv_escape("he\"lo"), "\"he\"\"lo\"");
    }

    #[test]
    fn test_run_eval() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("test.db");
        let mut session = Session::open(&db, true).unwrap();
        let sql = "CREATE TABLE t (id INT, val TEXT); INSERT INTO t VALUES (1, 'hello');";
        run_eval(&mut session, sql, false).unwrap();
        let result = session.execute_sql("SELECT * FROM t;").unwrap();
        if let ExecResult::Rows { rows, .. } = &result[0].0 {
            assert_eq!(rows.len(), 1);
        }
    }
}
