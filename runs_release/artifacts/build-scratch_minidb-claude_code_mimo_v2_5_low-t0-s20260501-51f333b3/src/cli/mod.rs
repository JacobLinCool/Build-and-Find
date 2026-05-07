use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::exec::operators::seq_scan::serialize_row;
use crate::exec::planner::PhysicalPlanner;
use crate::lexer::tokenize;
use crate::parser::{
    ast::{MetaCommand, Program, Statement},
    parse,
};
use crate::storage::manager::StorageManager;
use crate::txn::TransactionManager;
use crate::types::{Row, Value};
use log::info;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

pub struct MiniDbCli {
    db_path: PathBuf,
    catalog_path: PathBuf,
    catalog: Catalog,
    storage: Option<StorageManager>,
    txn_manager: TransactionManager,
    quiet: bool,
    format: OutputFormat,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

impl MiniDbCli {
    pub fn new(db_path: PathBuf, quiet: bool) -> Self {
        let catalog_path = db_path.with_extension("db.catalog");
        MiniDbCli {
            db_path,
            catalog_path,
            catalog: Catalog::new(),
            storage: None,
            txn_manager: TransactionManager::new(),
            quiet,
            format: OutputFormat::Table,
        }
    }

    pub fn set_format(&mut self, format: OutputFormat) {
        self.format = format;
    }

    pub fn open(&mut self) -> Result<()> {
        let storage = StorageManager::open(&self.db_path)?;
        self.storage = Some(storage);

        self.catalog = Catalog::load(&self.catalog_path)?;

        Ok(())
    }

    pub fn save_catalog(&mut self) -> Result<()> {
        self.catalog.save(&self.catalog_path)
    }

    pub fn init_db(&mut self) -> Result<()> {
        self.open()?;
        info!("Database initialized at {:?}", self.db_path);
        Ok(())
    }

    pub fn execute_sql(&mut self, sql: &str) -> Result<ExecutionResult> {
        let tokens = tokenize(sql)?;
        let program = parse(tokens)?;

        match program {
            Program::Meta(meta) => self.execute_meta(meta),
            Program::Statement(stmt) => self.execute_statement(stmt),
            _ => Err(MiniDbError::SqlError("unsupported program".to_string())),
        }
    }

    fn execute_meta(&mut self, meta: MetaCommand) -> Result<ExecutionResult> {
        match meta {
            MetaCommand::Schema => {
                let mut output = String::new();
                for table_name in self.catalog.all_table_names() {
                    let schema = self.catalog.get_table(table_name)?;
                    output.push_str(&format!("CREATE TABLE {} (\n", table_name));
                    for (i, col) in schema.columns.iter().enumerate() {
                        let mut parts = vec![format!("  {} {}", col.name, col.col_type)];
                        if col.primary_key {
                            parts.push("PRIMARY KEY".to_string());
                        }
                        if !col.nullable && !col.primary_key {
                            parts.push("NOT NULL".to_string());
                        }
                        if col.unique && !col.primary_key {
                            parts.push("UNIQUE".to_string());
                        }
                        if let Some(ref default) = col.default {
                            parts.push(format!("DEFAULT {}", default));
                        }
                        output.push_str(&parts.join(" "));
                        if i < schema.columns.len() - 1 {
                            output.push(',');
                        }
                        output.push('\n');
                    }
                    output.push_str(")\n\n");
                }
                Ok(ExecutionResult::MetaOutput(output))
            }
            MetaCommand::Tables => {
                let names = self.catalog.all_table_names();
                let mut output = String::new();
                for name in &names {
                    output.push_str(&format!("{}\n", name));
                }
                Ok(ExecutionResult::MetaOutput(output))
            }
            MetaCommand::Indexes => {
                let names = self.catalog.all_index_names();
                let mut output = String::new();
                for name in &names {
                    let idx = self.catalog.get_index(name)?;
                    output.push_str(&format!("{} ON {} ({})\n", idx.name, idx.table, idx.column));
                }
                Ok(ExecutionResult::MetaOutput(output))
            }
            MetaCommand::Exit => Err(MiniDbError::ExitRequested),
            MetaCommand::Format(fmt) => {
                self.format = match fmt.to_lowercase().as_str() {
                    "csv" => OutputFormat::Csv,
                    "json" => OutputFormat::Json,
                    _ => OutputFormat::Table,
                };
                Ok(ExecutionResult::MetaOutput(String::new()))
            }
        }
    }

    fn execute_statement(&mut self, stmt: Statement) -> Result<ExecutionResult> {
        match stmt {
            Statement::CreateTable { name, columns } => {
                self.catalog.create_table(name, columns)?;
                self.save_catalog()?;
                Ok(ExecutionResult::Ok("table created".to_string()))
            }
            Statement::DropTable { name } => {
                self.catalog.drop_table(&name)?;
                self.save_catalog()?;
                Ok(ExecutionResult::Ok("table dropped".to_string()))
            }
            Statement::CreateIndex {
                name,
                table,
                column,
            } => {
                self.catalog
                    .create_index(name.clone(), table.clone(), column.clone())?;
                let idx = self.catalog.get_index(&name)?;
                let root_page = idx.root_page;
                if let Some(ref mut storage) = self.storage {
                    storage.load_btree(root_page)?;
                }
                self.save_catalog()?;
                Ok(ExecutionResult::Ok("index created".to_string()))
            }
            Statement::DropIndex { name } => {
                self.catalog.drop_index(&name)?;
                self.save_catalog()?;
                Ok(ExecutionResult::Ok("index dropped".to_string()))
            }
            Statement::Begin => {
                let txn_id = self.txn_manager.begin()?;
                if let Some(ref mut storage) = self.storage {
                    storage.begin_txn()?;
                }
                Ok(ExecutionResult::Ok(format!(
                    "transaction {} started",
                    txn_id
                )))
            }
            Statement::Commit => {
                if let Some(ref mut storage) = self.storage {
                    storage.commit_txn()?;
                }
                self.txn_manager.commit()?;
                Ok(ExecutionResult::Ok("committed".to_string()))
            }
            Statement::Rollback => {
                if let Some(ref mut storage) = self.storage {
                    storage.rollback_txn()?;
                }
                self.txn_manager.rollback()?;
                Ok(ExecutionResult::Ok("rolled back".to_string()))
            }
            Statement::Select { .. }
            | Statement::Insert { .. }
            | Statement::Update { .. }
            | Statement::Delete { .. } => self.execute_dml(stmt),
        }
    }

    fn execute_dml(&mut self, stmt: Statement) -> Result<ExecutionResult> {
        let storage = self
            .storage
            .as_mut()
            .ok_or_else(|| MiniDbError::StorageError("database not open".to_string()))?;

        // Auto-begin transaction if not in one
        let auto_txn = !self.txn_manager.is_in_transaction();
        if auto_txn {
            self.txn_manager.begin()?;
            storage.begin_txn()?;
        }

        let result = match &stmt {
            Statement::Select {
                columns,
                from,
                where_clause,
                order_by,
                limit,
                offset,
            } => {
                let planner = PhysicalPlanner::new(&self.catalog);
                let program = Program::Statement(stmt.clone());
                let physical_plan = planner.plan(&program)?;

                // Build and execute the plan
                self.execute_physical_plan(physical_plan)
            }
            Statement::Insert {
                table,
                columns,
                values,
            } => {
                let schema = self.catalog.get_table(table)?.clone();
                let root_page = schema.root_page;
                let col_names: Vec<String> =
                    schema.columns.iter().map(|c| c.name.clone()).collect();

                let mut count = 0;
                for row_values in values {
                    let mut full_values = self.catalog.default_values(table)?;

                    if let Some(ref cols) = columns {
                        for (i, col_name) in cols.iter().enumerate() {
                            if let Some(idx) = col_names.iter().position(|c| c == col_name) {
                                if i < row_values.len() {
                                    let evaluator = crate::exec::evaluator::ExprEvaluator::new(
                                        row_values[i].clone(),
                                        col_names.clone(),
                                    );
                                    let empty_row = Row::new(vec![Value::Null; col_names.len()]);
                                    full_values[idx] = evaluator.evaluate(&empty_row)?;
                                }
                            }
                        }
                    } else {
                        for (i, expr) in row_values.iter().enumerate() {
                            if i < full_values.len() {
                                let evaluator = crate::exec::evaluator::ExprEvaluator::new(
                                    expr.clone(),
                                    col_names.clone(),
                                );
                                let empty_row = Row::new(vec![Value::Null; col_names.len()]);
                                full_values[i] = evaluator.evaluate(&empty_row)?;
                            }
                        }
                    }

                    self.catalog.validate_row(table, &full_values)?;

                    // Handle INSERT with specific columns
                    let actual_values = if let Some(ref cols) = columns {
                        let mut vals = self.catalog.default_values(table)?;
                        for (i, col_name) in cols.iter().enumerate() {
                            if let Some(idx) = col_names.iter().position(|c| c == col_name) {
                                if i < row_values.len() {
                                    let evaluator = crate::exec::evaluator::ExprEvaluator::new(
                                        row_values[i].clone(),
                                        col_names.clone(),
                                    );
                                    let empty_row = Row::new(vec![Value::Null; col_names.len()]);
                                    vals[idx] = evaluator.evaluate(&empty_row)?;
                                }
                            }
                        }
                        vals
                    } else {
                        let mut vals = Vec::new();
                        for (i, expr) in row_values.iter().enumerate() {
                            if i < schema.columns.len() {
                                let evaluator = crate::exec::evaluator::ExprEvaluator::new(
                                    expr.clone(),
                                    col_names.clone(),
                                );
                                let empty_row = Row::new(vec![Value::Null; col_names.len()]);
                                vals.push(evaluator.evaluate(&empty_row)?);
                            }
                        }
                        // Pad with defaults
                        while vals.len() < schema.columns.len() {
                            let defaults = self.catalog.default_values(table)?;
                            if vals.len() < defaults.len() {
                                vals.push(defaults[vals.len()].clone());
                            } else {
                                break;
                            }
                        }
                        vals
                    };

                    let row_data = serialize_row(&actual_values);
                    storage.insert_row(root_page, &row_data)?;

                    // Update indexes if any
                    let index_cols: Vec<(String, u64)> = self
                        .catalog
                        .all_index_names()
                        .iter()
                        .filter_map(|name| {
                            let idx = self.catalog.get_index(name).ok()?;
                            if idx.table == *table {
                                Some((idx.column.clone(), idx.root_page))
                            } else {
                                None
                            }
                        })
                        .collect();

                    for (col_name, idx_root) in index_cols {
                        if let Some(col_idx) = col_names.iter().position(|c| *c == col_name) {
                            let val = &actual_values[col_idx];
                            let slot = storage.num_rows(root_page)? as u16 - 1;
                            storage.btree_insert(idx_root, val, root_page, slot)?;
                        }
                    }

                    count += 1;
                }

                if auto_txn {
                    storage.commit_txn()?;
                    self.txn_manager.commit()?;
                }

                let summary = if count == 1 {
                    format!("ok 1 row inserted")
                } else {
                    format!("ok {} rows inserted", count)
                };
                Ok(ExecutionResult::DmlResult {
                    rows_affected: count,
                    summary,
                })
            }
            Statement::Update {
                table,
                set_clauses,
                where_clause,
            } => {
                let schema = self.catalog.get_table(table)?.clone();
                let root_page = schema.root_page;
                let col_names: Vec<String> =
                    schema.columns.iter().map(|c| c.name.clone()).collect();

                let where_evaluator = where_clause.as_ref().map(|expr| {
                    crate::exec::evaluator::ExprEvaluator::new(expr.clone(), col_names.clone())
                });

                let set_evaluators: Vec<(usize, crate::exec::evaluator::ExprEvaluator)> =
                    set_clauses
                        .iter()
                        .filter_map(|(col_name, expr)| {
                            col_names.iter().position(|c| c == col_name).map(|idx| {
                                (
                                    idx,
                                    crate::exec::evaluator::ExprEvaluator::new(
                                        expr.clone(),
                                        col_names.clone(),
                                    ),
                                )
                            })
                        })
                        .collect();

                let rows = storage.scan_page(root_page)?;
                let mut count = 0;

                for (slot, data) in rows {
                    let values =
                        crate::exec::operators::seq_scan::deserialize_row(&data, col_names.len())?;
                    let row = Row::new(values.clone());

                    if let Some(ref evaluator) = where_evaluator {
                        let result = evaluator.evaluate(&row)?;
                        match result {
                            Value::Bool(true) => {}
                            _ => continue,
                        }
                    }

                    let mut new_values = values.clone();
                    for (idx, evaluator) in &set_evaluators {
                        let new_val = evaluator.evaluate(&row)?;
                        new_values[*idx] = new_val;
                    }

                    self.catalog.validate_row(table, &new_values)?;

                    let row_data = serialize_row(&new_values);
                    storage.delete_row(root_page, slot)?;
                    storage.insert_row(root_page, &row_data)?;

                    count += 1;
                }

                if auto_txn {
                    storage.commit_txn()?;
                    self.txn_manager.commit()?;
                }

                let summary = if count == 1 {
                    format!("ok 1 row updated")
                } else {
                    format!("ok {} rows updated", count)
                };
                Ok(ExecutionResult::DmlResult {
                    rows_affected: count,
                    summary,
                })
            }
            Statement::Delete {
                table,
                where_clause,
            } => {
                let schema = self.catalog.get_table(table)?.clone();
                let root_page = schema.root_page;
                let col_names: Vec<String> =
                    schema.columns.iter().map(|c| c.name.clone()).collect();

                let where_evaluator = where_clause.as_ref().map(|expr| {
                    crate::exec::evaluator::ExprEvaluator::new(expr.clone(), col_names.clone())
                });

                let rows = storage.scan_page(root_page)?;
                let mut count = 0;

                for (slot, data) in rows {
                    let values =
                        crate::exec::operators::seq_scan::deserialize_row(&data, col_names.len())?;
                    let row = Row::new(values);

                    if let Some(ref evaluator) = where_evaluator {
                        let result = evaluator.evaluate(&row)?;
                        match result {
                            Value::Bool(true) => {}
                            _ => continue,
                        }
                    }

                    storage.delete_row(root_page, slot)?;
                    count += 1;
                }

                if auto_txn {
                    storage.commit_txn()?;
                    self.txn_manager.commit()?;
                }

                let summary = if count == 1 {
                    format!("ok 1 row deleted")
                } else {
                    format!("ok {} rows deleted", count)
                };
                Ok(ExecutionResult::DmlResult {
                    rows_affected: count,
                    summary,
                })
            }
            _ => unreachable!(),
        };

        result
    }

    fn execute_physical_plan(
        &mut self,
        plan: crate::plan::physical::PhysicalPlan,
    ) -> Result<ExecutionResult> {
        let storage = self
            .storage
            .as_mut()
            .ok_or_else(|| MiniDbError::StorageError("database not open".to_string()))?;

        // For now, handle SELECT by scanning the table
        match plan {
            crate::plan::physical::PhysicalPlan::Project { input, columns } => {
                if let crate::plan::physical::PhysicalPlan::SeqScan { table, .. } = *input {
                    let schema = self.catalog.get_table(&table)?;
                    let root_page = schema.root_page;
                    let rows = storage.scan_page(root_page)?;

                    let col_names: Vec<String> =
                        schema.columns.iter().map(|c| c.name.clone()).collect();
                    let mut result_rows = Vec::new();

                    for (_, data) in rows {
                        let values = crate::exec::operators::seq_scan::deserialize_row(
                            &data,
                            col_names.len(),
                        )?;
                        let row = Row::new(values);

                        // Apply projection
                        let has_star = columns
                            .iter()
                            .any(|c| matches!(c, crate::parser::ast::SelectColumn::Star));
                        if has_star {
                            result_rows.push(row);
                        } else {
                            let mut projected = Vec::new();
                            for col in &columns {
                                match col {
                                    crate::parser::ast::SelectColumn::Expr(expr, _) => {
                                        let evaluator = crate::exec::evaluator::ExprEvaluator::new(
                                            expr.clone(),
                                            col_names.clone(),
                                        );
                                        projected.push(evaluator.evaluate(&row)?);
                                    }
                                    _ => unreachable!(),
                                }
                            }
                            result_rows.push(Row::new(projected));
                        }
                    }

                    let result_schema: Vec<String> = if columns
                        .iter()
                        .any(|c| matches!(c, crate::parser::ast::SelectColumn::Star))
                    {
                        col_names
                    } else {
                        columns
                            .iter()
                            .map(|c| match c {
                                crate::parser::ast::SelectColumn::Expr(_, alias) => {
                                    alias.clone().unwrap_or_else(|| "expr".to_string())
                                }
                                _ => "*".to_string(),
                            })
                            .collect()
                    };

                    Ok(ExecutionResult::QueryResult {
                        schema: result_schema,
                        rows: result_rows,
                    })
                } else {
                    Err(MiniDbError::SqlError("unsupported plan shape".to_string()))
                }
            }
            crate::plan::physical::PhysicalPlan::SeqScan { table, .. } => {
                let schema = self.catalog.get_table(&table)?;
                let root_page = schema.root_page;
                let rows = storage.scan_page(root_page)?;

                let col_names: Vec<String> =
                    schema.columns.iter().map(|c| c.name.clone()).collect();
                let mut result_rows = Vec::new();

                for (_, data) in rows {
                    let values =
                        crate::exec::operators::seq_scan::deserialize_row(&data, col_names.len())?;
                    result_rows.push(Row::new(values));
                }

                Ok(ExecutionResult::QueryResult {
                    schema: col_names,
                    rows: result_rows,
                })
            }
            _ => Err(MiniDbError::SqlError(
                "unsupported physical plan".to_string(),
            )),
        }
    }

    pub fn run_repl(&mut self) -> Result<()> {
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        let mut stdout = io::stdout();

        loop {
            print!("minidb> ");
            stdout.flush()?;

            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {}
                Err(e) => return Err(e.into()),
            }

            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            match self.execute_sql(line) {
                Ok(result) => self.print_result(&result)?,
                Err(MiniDbError::ExitRequested) => break,
                Err(e) => {
                    eprintln!("Error: {}", e);
                }
            }
        }

        Ok(())
    }

    pub fn run_eval(&mut self, sql: &str) -> Result<()> {
        let statements: Vec<&str> = sql.split(';').filter(|s| !s.trim().is_empty()).collect();

        for stmt in statements {
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }

            match self.execute_sql(stmt) {
                Ok(result) => self.print_result(&result)?,
                Err(MiniDbError::ExitRequested) => break,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    pub fn run_file(&mut self, path: &str, continue_on_error: bool) -> Result<()> {
        let content = std::fs::read_to_string(path)?;
        let statements: Vec<&str> = content
            .split(';')
            .filter(|s| !s.trim().is_empty())
            .collect();

        for stmt in statements {
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }

            match self.execute_sql(stmt) {
                Ok(result) => self.print_result(&result)?,
                Err(MiniDbError::ExitRequested) => break,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    if !continue_on_error {
                        return Err(e);
                    }
                }
            }
        }

        Ok(())
    }

    fn print_result(&self, result: &ExecutionResult) -> Result<()> {
        match result {
            ExecutionResult::Ok(msg) => {
                if !self.quiet {
                    eprintln!("{}", msg);
                }
            }
            ExecutionResult::DmlResult { summary, .. } => {
                if !self.quiet {
                    eprintln!("{}", summary);
                }
            }
            ExecutionResult::QueryResult { schema, rows } => match self.format {
                OutputFormat::Table => self.print_table(schema, rows)?,
                OutputFormat::Csv => self.print_csv(schema, rows)?,
                OutputFormat::Json => self.print_json(schema, rows)?,
            },
            ExecutionResult::MetaOutput(output) => {
                if !output.is_empty() {
                    print!("{}", output);
                }
            }
        }
        Ok(())
    }

    fn print_table(&self, schema: &[String], rows: &[Row]) -> Result<()> {
        if schema.is_empty() {
            return Ok(());
        }

        // Calculate column widths
        let mut widths: Vec<usize> = schema.iter().map(|s| s.len()).collect();
        for row in rows {
            for (i, val) in row.values.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(val.to_string().len());
                }
            }
        }

        // Print header
        for (i, name) in schema.iter().enumerate() {
            if i > 0 {
                print!(" | ");
            }
            print!("{:width$}", name, width = widths[i]);
        }
        println!();

        // Print separator
        for (i, w) in widths.iter().enumerate() {
            if i > 0 {
                print!("-+-");
            }
            print!("{}", "-".repeat(*w));
        }
        println!();

        // Print rows
        for row in rows {
            for (i, val) in row.values.iter().enumerate() {
                if i > 0 {
                    print!(" | ");
                }
                print!("{:width$}", val, width = widths[i]);
            }
            println!();
        }

        if !self.quiet {
            eprintln!(
                "{} {}",
                rows.len(),
                if rows.len() == 1 { "row" } else { "rows" }
            );
        }

        Ok(())
    }

    fn print_csv(&self, schema: &[String], rows: &[Row]) -> Result<()> {
        let mut wtr = csv::Writer::from_writer(io::stdout());
        wtr.write_record(schema)?;
        for row in rows {
            let record: Vec<String> = row.values.iter().map(|v| v.to_string()).collect();
            wtr.write_record(&record)?;
        }
        wtr.flush()?;
        Ok(())
    }

    fn print_json(&self, schema: &[String], rows: &[Row]) -> Result<()> {
        let mut objects = Vec::new();
        for row in rows {
            let mut map = serde_json::Map::new();
            for (i, name) in schema.iter().enumerate() {
                let val = if i < row.values.len() {
                    match &row.values[i] {
                        Value::Null => serde_json::Value::Null,
                        Value::Int(v) => serde_json::Value::Number((*v).into()),
                        Value::Real(v) => serde_json::Number::from_f64(*v)
                            .map(serde_json::Value::Number)
                            .unwrap_or(serde_json::Value::Null),
                        Value::Text(v) => serde_json::Value::String(v.clone()),
                        Value::Bool(v) => serde_json::Value::Bool(*v),
                    }
                } else {
                    serde_json::Value::Null
                };
                map.insert(name.clone(), val);
            }
            objects.push(serde_json::Value::Object(map));
        }
        println!("{}", serde_json::to_string_pretty(&objects)?);
        Ok(())
    }
}

#[derive(Debug)]
pub enum ExecutionResult {
    Ok(String),
    DmlResult {
        rows_affected: usize,
        summary: String,
    },
    QueryResult {
        schema: Vec<String>,
        rows: Vec<Row>,
    },
    MetaOutput(String),
}
