use crate::catalog::{Catalog, ColumnSchema, IndexSchema, TableSchema};
use crate::error::{MiniDbError, Result};
use crate::exec::expression;
use crate::exec::{Executor, OutputFormat};
use crate::parser;
use crate::plan::plan_statement;
use crate::storage::btree::IndexKey;
use crate::storage::heap::HeapFile;
use crate::storage::wal::Wal;
use crate::txn::TransactionManager;
use crate::types::{ColumnType, Value};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};

pub struct MiniDbCli {
    db_path: String,
    format: OutputFormat,
    quiet: bool,
    init_only: bool,
    continue_on_error: bool,
}

impl MiniDbCli {
    pub fn new(
        db_path: &str,
        format: OutputFormat,
        quiet: bool,
        init_only: bool,
        continue_on_error: bool,
    ) -> Self {
        Self {
            db_path: db_path.to_string(),
            format,
            quiet,
            init_only,
            continue_on_error,
        }
    }

    pub fn run(&self, eval: Option<&str>, file: Option<&str>) -> Result<()> {
        env_logger::init();

        let wal_path = format!("{}.wal", self.db_path);
        let catalog_path = format!("{}.catalog", self.db_path);
        let mut wal = Wal::open(&wal_path)?;
        let mut heap = HeapFile::open(&self.db_path)?;
        let mut catalog = if let Ok(c) = Catalog::load(&catalog_path) {
            c
        } else {
            Catalog::new()
        };

        // Replay WAL for recovery
        let records = wal.replay()?;
        self.replay_wal(&records, &mut catalog, &mut heap)?;
        wal.clear()?;

        if self.init_only {
            return Ok(());
        }

        let mut txn_mgr = TransactionManager::new(wal);

        if let Some(sql) = eval {
            return self.execute_sql_batch(&sql, &mut catalog, &mut heap, &mut txn_mgr);
        }

        if let Some(path) = file {
            return self.execute_file(path, &mut catalog, &mut heap, &mut txn_mgr);
        }

        // REPL mode
        self.run_repl(&mut catalog, &mut heap, &mut txn_mgr)
    }

    fn replay_wal(
        &self,
        records: &[crate::storage::wal::WalRecord],
        catalog: &mut Catalog,
        heap: &mut HeapFile,
    ) -> Result<()> {
        use crate::storage::wal::WalRecord;
        let mut committed_txns = std::collections::HashSet::new();
        let mut active_txns = std::collections::HashSet::new();

        for record in records {
            match record {
                WalRecord::Begin { tx_id } => {
                    active_txns.insert(*tx_id);
                }
                WalRecord::Commit { tx_id } => {
                    committed_txns.insert(*tx_id);
                    active_txns.remove(tx_id);
                }
                WalRecord::Rollback { tx_id } => {
                    active_txns.remove(tx_id);
                }
                _ => {}
            }
        }

        // Apply committed transactions
        for record in records {
            match record {
                WalRecord::Insert {
                    tx_id,
                    table,
                    row_id,
                    data,
                } if committed_txns.contains(tx_id) => {
                    if catalog.table_exists(table) {
                        let row_id = crate::storage::RowId(*row_id);
                        heap.add_row(table, data)?;
                    }
                }
                WalRecord::Delete {
                    tx_id,
                    table,
                    row_id,
                } if committed_txns.contains(tx_id) => {
                    if catalog.table_exists(table) {
                        let row_id = crate::storage::RowId(*row_id);
                        heap.delete_row(row_id)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn execute_sql_batch(
        &self,
        sql: &str,
        catalog: &mut Catalog,
        heap: &mut HeapFile,
        txn_mgr: &mut TransactionManager,
    ) -> Result<()> {
        let statements = parser::parse(sql)?;
        for stmt in &statements {
            self.execute_statement(stmt, catalog, heap, txn_mgr)?;
        }
        Ok(())
    }

    fn execute_file(
        &self,
        path: &str,
        catalog: &mut Catalog,
        heap: &mut HeapFile,
        txn_mgr: &mut TransactionManager,
    ) -> Result<()> {
        let content = fs::read_to_string(path).map_err(|e| MiniDbError::Io(e))?;
        let statements = parser::parse(&content)?;
        for stmt in &statements {
            match self.execute_statement(stmt, catalog, heap, txn_mgr) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("Error: {}", e);
                    if !self.continue_on_error {
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

    fn run_repl(
        &self,
        catalog: &mut Catalog,
        heap: &mut HeapFile,
        txn_mgr: &mut TransactionManager,
    ) -> Result<()> {
        let stdin = io::stdin();
        let reader = stdin.lock();
        let mut buf_reader = BufReader::new(reader);

        loop {
            eprint!("minidb> ");
            eflush()?;
            let mut line = String::new();
            match buf_reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {}
                Err(e) => return Err(MiniDbError::Io(e)),
            }
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line == ".exit" {
                break;
            }
            if line.starts_with('.') {
                self.execute_meta_command(line, catalog, txn_mgr)?;
                continue;
            }
            // Read full statement (may span multiple lines)
            let mut full_stmt = line.to_string();
            while !full_stmt.ends_with(';') {
                eprint!("  ... ");
                eflush()?;
                let mut next_line = String::new();
                match buf_reader.read_line(&mut next_line) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) => return Err(MiniDbError::Io(e)),
                }
                let next_line = next_line.trim();
                if next_line == ".exit" {
                    return Ok(());
                }
                full_stmt.push(' ');
                full_stmt.push_str(next_line);
            }
            match parser::parse(&full_stmt) {
                Ok(stmts) => {
                    for stmt in &stmts {
                        match self.execute_statement(stmt, catalog, heap, txn_mgr) {
                            Ok(()) => {}
                            Err(e) => {
                                eprintln!("Error: {}", e);
                                if !txn_mgr.is_in_transaction() {
                                    // Auto mode, continue
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                }
            }
        }
        Ok(())
    }

    fn execute_meta_command(
        &self,
        cmd: &str,
        catalog: &mut Catalog,
        txn_mgr: &mut TransactionManager,
    ) -> Result<()> {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        match parts[0] {
            ".schema" => {
                if parts.len() > 1 {
                    let sql = catalog.schema_sql(parts[1])?;
                    println!("{}", sql);
                } else {
                    for name in catalog.all_table_names() {
                        let sql = catalog.schema_sql(name)?;
                        println!("{};", sql);
                    }
                }
            }
            ".tables" => {
                for name in catalog.all_table_names() {
                    println!("{}", name);
                }
            }
            ".indexes" => {
                for name in catalog.all_index_names() {
                    let idx = catalog.get_index(name)?;
                    println!("{} ON {} ({})", idx.name, idx.table, idx.column);
                }
            }
            ".format" => {
                if parts.len() > 1 {
                    match parts[1] {
                        "csv" => eprintln!("Format set to CSV"),
                        "json" => eprintln!("Format set to JSON"),
                        "table" => eprintln!("Format set to TABLE"),
                        _ => eprintln!("Unknown format: {}", parts[1]),
                    }
                }
            }
            _ => {
                eprintln!("Unknown command: {}", cmd);
            }
        }
        Ok(())
    }

    fn execute_statement(
        &self,
        stmt: &parser::ast::Statement,
        catalog: &mut Catalog,
        heap: &mut HeapFile,
        txn_mgr: &mut TransactionManager,
    ) -> Result<()> {
        match stmt {
            parser::ast::Statement::Begin => {
                txn_mgr.begin()?;
                if !self.quiet {
                    eprintln!("ok transaction started");
                }
                Ok(())
            }
            parser::ast::Statement::Commit => {
                txn_mgr.commit()?;
                if !self.quiet {
                    eprintln!("ok committed");
                }
                Ok(())
            }
            parser::ast::Statement::Rollback => {
                txn_mgr.rollback()?;
                if !self.quiet {
                    eprintln!("ok rolled back");
                }
                Ok(())
            }
            parser::ast::Statement::CreateTable(ct) => {
                let mut columns = Vec::new();
                for col in &ct.columns {
                    columns.push(ColumnSchema {
                        name: col.name.clone(),
                        col_type: col.col_type.to_type_def(),
                        primary_key: col.primary_key,
                        not_null: col.not_null,
                        unique: col.unique,
                        default: col.default.clone(),
                    });
                }
                let schema = TableSchema {
                    name: ct.name.clone(),
                    columns,
                    check_constraints: ct.check_constraints.clone(),
                };
                catalog.create_table(schema)?;
                catalog.save(&format!("{}.catalog", self.db_path))?;
                if !self.quiet {
                    eprintln!("ok table created");
                }
                Ok(())
            }
            parser::ast::Statement::DropTable(name) => {
                catalog.drop_table(name)?;
                catalog.save(&format!("{}.catalog", self.db_path))?;
                if !self.quiet {
                    eprintln!("ok table dropped");
                }
                Ok(())
            }
            parser::ast::Statement::CreateIndex(ci) => {
                let schema = IndexSchema {
                    name: ci.name.clone(),
                    table: ci.table.clone(),
                    column: ci.column.clone(),
                };
                catalog.create_index(schema)?;
                // Populate index with existing data
                let idx_name = ci.name.clone();
                let table_name = ci.table.clone();
                let col_name = ci.column.clone();
                let col_idx = catalog.column_index(&table_name, &col_name)?;
                let row_ids = heap.all_row_ids();
                for row_id in row_ids {
                    if let Ok(data) = heap.get_row(row_id) {
                        if !data.is_empty() && data[0] != 0xFF {
                            let schema = catalog.get_table(&table_name)?;
                            let columns: Vec<(String, ColumnType)> = schema
                                .columns
                                .iter()
                                .map(|c| (c.name.clone(), c.col_type.clone()))
                                .collect();
                            let values = expression::deserialize_row(&data, &columns);
                            if let Some(val) = values.get(col_idx) {
                                let key = IndexKey::from_value(val);
                                catalog.add_index_entry(&idx_name, key, row_id)?;
                            }
                        }
                    }
                }
                catalog.save(&format!("{}.catalog", self.db_path))?;
                if !self.quiet {
                    eprintln!("ok index created");
                }
                Ok(())
            }
            parser::ast::Statement::DropIndex(name) => {
                catalog.drop_index(name)?;
                catalog.save(&format!("{}.catalog", self.db_path))?;
                if !self.quiet {
                    eprintln!("ok index dropped");
                }
                Ok(())
            }
            parser::ast::Statement::Select(_)
            | parser::ast::Statement::Insert(_)
            | parser::ast::Statement::Update(_)
            | parser::ast::Statement::Delete(_) => {
                let plan = plan_statement(stmt, catalog)?;
                let tx_id = txn_mgr.auto_begin()?;
                let mut executor =
                    Executor::new(catalog, heap, tx_id, plan).with_format(self.format.clone());
                let result = executor.execute()?;
                if result.is_query {
                    match self.format {
                        OutputFormat::Table => print!("{}", result.format_table()),
                        OutputFormat::Csv => print!("{}", result.format_csv()),
                        OutputFormat::Json => println!("{}", result.format_json()),
                    }
                    if !self.quiet {
                        let count = result.rows.len();
                        let noun = if count == 1 { "row" } else { "rows" };
                        eprintln!("ok {} {}", count, noun);
                    }
                } else {
                    if !self.quiet {
                        let count = result.affected_rows;
                        let noun = if count == 1 { "row" } else { "rows" };
                        eprintln!("ok {} {} affected", count, noun);
                    }
                }
                txn_mgr.auto_commit()?;
                Ok(())
            }
        }
    }
}

fn eflush() -> Result<()> {
    io::stderr().flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parse_args() {
        let cli = MiniDbCli::new("test.db", OutputFormat::Table, false, false, false);
        assert_eq!(cli.db_path, "test.db");
        assert_eq!(cli.format, OutputFormat::Table);
    }
}
