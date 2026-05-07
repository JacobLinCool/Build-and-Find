use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::exec::operator::*;
use crate::plan::logical::*;
use crate::plan::physical::*;
use crate::plan::planner::Planner;
use crate::storage::{BTree, HeapFile, PageId, Pager};
use crate::txn::TransactionManager;
use crate::types::*;

pub struct Engine {
    pub pager: Pager,
    pub catalog: Catalog,
    pub txn: TransactionManager,
    pub output_format: OutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

#[derive(Debug)]
pub enum QueryResult {
    Rows {
        schema: Schema,
        rows: Vec<Vec<Value>>,
    },
    RowsAffected(u64),
    Success,
}

impl Engine {
    pub fn new(pager: Pager, catalog: Catalog, txn: TransactionManager) -> Self {
        Self {
            pager,
            catalog,
            txn,
            output_format: OutputFormat::Table,
        }
    }

    pub fn execute(&mut self, stmt: &crate::parser::ast::Statement) -> Result<QueryResult> {
        match stmt {
            crate::parser::ast::Statement::Select(sel) => self.execute_select(sel),
            crate::parser::ast::Statement::Insert(ins) => self.execute_insert(ins),
            crate::parser::ast::Statement::Update(upd) => self.execute_update(upd),
            crate::parser::ast::Statement::Delete(del) => self.execute_delete(del),
            crate::parser::ast::Statement::CreateTable(ct) => self.execute_create_table(ct),
            crate::parser::ast::Statement::DropTable { name } => self.execute_drop_table(name),
            crate::parser::ast::Statement::CreateIndex(ci) => self.execute_create_index(ci),
            crate::parser::ast::Statement::DropIndex { name } => self.execute_drop_index(name),
            crate::parser::ast::Statement::Begin => {
                self.txn.begin()?;
                Ok(QueryResult::Success)
            }
            crate::parser::ast::Statement::Commit => {
                self.flush_dirty_pages()?;
                self.txn.commit()?;
                Ok(QueryResult::Success)
            }
            crate::parser::ast::Statement::Rollback => {
                self.txn.rollback()?;
                // Clear page cache to restore committed state from disk
                self.pager.clear_cache();
                // Reload catalog from disk
                self.load_catalog()?;
                Ok(QueryResult::Success)
            }
            crate::parser::ast::Statement::MetaSchema => self.execute_meta_schema(),
            crate::parser::ast::Statement::MetaTables => self.execute_meta_tables(),
            crate::parser::ast::Statement::MetaIndexes => self.execute_meta_indexes(),
            crate::parser::ast::Statement::MetaFormat(kind) => {
                match kind.to_lowercase().as_str() {
                    "table" => self.output_format = OutputFormat::Table,
                    "csv" => self.output_format = OutputFormat::Csv,
                    "json" => self.output_format = OutputFormat::Json,
                    _ => return Err(MiniDbError::Runtime(format!("unknown format: {}", kind))),
                }
                Ok(QueryResult::Success)
            }
            crate::parser::ast::Statement::MetaExit => {
                std::process::exit(0);
            }
        }
    }

    fn execute_select(&mut self, sel: &crate::parser::ast::SelectStmt) -> Result<QueryResult> {
        // Check for aggregates before creating planner (to avoid borrow conflicts)
        let has_agg = sel.projection.iter().any(|item| {
            if let crate::parser::ast::SelectItem::Expr { expr, .. } = item {
                is_aggregate_expr(expr)
            } else {
                false
            }
        });

        if has_agg && sel.group_by.is_none() {
            // For aggregate queries without GROUP BY, bypass the Project
            // and compute aggregates directly from raw input rows
            let planner = Planner::new(&self.catalog);
            let logical = planner.plan(&crate::parser::ast::Statement::Select(sel.clone()))?;
            let logical = planner.optimize(logical)?;

            // Build the input plan (strip the final Project if present)
            let input_logical = match &logical {
                LogicalPlan::Project { input, .. } => input.as_ref().clone(),
                other => other.clone(),
            };

            let physical = self.create_physical_plan(&input_logical)?;
            let mut op = self.build_operator(physical)?;

            op.open()?;
            let mut rows = Vec::new();
            loop {
                match op.next()? {
                    None => break,
                    Some(row) => rows.push(row),
                }
            }
            op.close()?;

            let input_schema = op.schema().clone();

            // Build output schema from projection aliases
            let mut out_cols = Vec::new();
            for item in &sel.projection {
                match item {
                    crate::parser::ast::SelectItem::Expr { alias, .. } => {
                        let name = alias.clone().unwrap_or_else(|| "expr".to_string());
                        out_cols.push(crate::types::ColumnDef {
                            name,
                            col_type: crate::types::ColumnType::Text,
                            constraints: vec![],
                        });
                    }
                    _ => out_cols.push(crate::types::ColumnDef {
                        name: "expr".into(),
                        col_type: crate::types::ColumnType::Text,
                        constraints: vec![],
                    }),
                }
            }
            let schema = Schema::new(out_cols);

            // Evaluate aggregates
            let mut agg_row = Vec::new();
            for item in &sel.projection {
                match item {
                    crate::parser::ast::SelectItem::Expr { expr, .. } => {
                        let val = self.eval_aggregate(expr, &rows, &input_schema)?;
                        agg_row.push(val);
                    }
                    _ => agg_row.push(Value::Null),
                }
            }
            Ok(QueryResult::Rows {
                schema,
                rows: vec![agg_row],
            })
        } else {
            let planner = Planner::new(&self.catalog);
            let logical = planner.plan(&crate::parser::ast::Statement::Select(sel.clone()))?;
            let logical = planner.optimize(logical)?;
            let physical = self.create_physical_plan(&logical)?;
            let mut op = self.build_operator(physical)?;

            op.open()?;
            let mut rows = Vec::new();
            loop {
                match op.next()? {
                    None => break,
                    Some(row) => rows.push(row),
                }
            }
            op.close()?;

            let schema = op.schema().clone();
            Ok(QueryResult::Rows { schema, rows })
        }
    }

    fn eval_aggregate(
        &self,
        expr: &crate::parser::ast::Expr,
        rows: &[Vec<Value>],
        schema: &Schema,
    ) -> Result<Value> {
        match expr {
            crate::parser::ast::Expr::FunctionCall { name, args } => {
                match name.to_uppercase().as_str() {
                    "COUNT" => {
                        // COUNT(*) or COUNT(expr)
                        if args.len() == 1 {
                            if let crate::parser::ast::Expr::ColumnRef { name: col_name, .. } =
                                &args[0]
                            {
                                if col_name == "*" {
                                    return Ok(Value::Int(rows.len() as i64));
                                }
                            }
                        }
                        // COUNT(expr) - count non-null values
                        let planner = Planner::new(&self.catalog);
                        let star_ref = crate::parser::ast::Expr::ColumnRef {
                            table: None,
                            name: "*".to_string(),
                        };
                        let count_arg = if args.is_empty() { &star_ref } else { &args[0] };
                        let compiled = planner.compile_expr_for_schema(count_arg, schema)?;
                        let mut count = 0i64;
                        for row in rows {
                            let val = eval_expr(&compiled, row, schema)?;
                            if !val.is_null() {
                                count += 1;
                            }
                        }
                        Ok(Value::Int(count))
                    }
                    "SUM" => {
                        if args.is_empty() {
                            return Ok(Value::Null);
                        }
                        let planner = Planner::new(&self.catalog);
                        let compiled = planner.compile_expr_for_schema(&args[0], schema)?;
                        let mut sum_int: Option<i64> = None;
                        let mut sum_real: Option<f64> = None;
                        for row in rows {
                            let val = eval_expr(&compiled, row, schema)?;
                            match val {
                                Value::Int(i) => {
                                    sum_int = Some(sum_int.unwrap_or(0) + i);
                                }
                                Value::Real(r) => {
                                    sum_real = Some(sum_real.unwrap_or(0.0) + r);
                                }
                                _ => {}
                            }
                        }
                        if let Some(r) = sum_real {
                            Ok(Value::Real(r + sum_int.unwrap_or(0) as f64))
                        } else if let Some(i) = sum_int {
                            Ok(Value::Int(i))
                        } else {
                            Ok(Value::Null)
                        }
                    }
                    "MIN" => {
                        if args.is_empty() {
                            return Ok(Value::Null);
                        }
                        let planner = Planner::new(&self.catalog);
                        let compiled = planner.compile_expr_for_schema(&args[0], schema)?;
                        let mut min_val: Option<Value> = None;
                        for row in rows {
                            let val = eval_expr(&compiled, row, schema)?;
                            if !val.is_null() {
                                min_val = Some(match min_val {
                                    None => val,
                                    Some(current) => {
                                        if val.partial_cmp(&current)
                                            == Some(std::cmp::Ordering::Less)
                                        {
                                            val
                                        } else {
                                            current
                                        }
                                    }
                                });
                            }
                        }
                        Ok(min_val.unwrap_or(Value::Null))
                    }
                    "MAX" => {
                        if args.is_empty() {
                            return Ok(Value::Null);
                        }
                        let planner = Planner::new(&self.catalog);
                        let compiled = planner.compile_expr_for_schema(&args[0], schema)?;
                        let mut max_val: Option<Value> = None;
                        for row in rows {
                            let val = eval_expr(&compiled, row, schema)?;
                            if !val.is_null() {
                                max_val = Some(match max_val {
                                    None => val,
                                    Some(current) => {
                                        if val.partial_cmp(&current)
                                            == Some(std::cmp::Ordering::Greater)
                                        {
                                            val
                                        } else {
                                            current
                                        }
                                    }
                                });
                            }
                        }
                        Ok(max_val.unwrap_or(Value::Null))
                    }
                    "AVG" => {
                        if args.is_empty() {
                            return Ok(Value::Null);
                        }
                        let planner = Planner::new(&self.catalog);
                        let compiled = planner.compile_expr_for_schema(&args[0], schema)?;
                        let mut sum = 0.0f64;
                        let mut count = 0i64;
                        for row in rows {
                            let val = eval_expr(&compiled, row, schema)?;
                            match val {
                                Value::Int(i) => {
                                    sum += i as f64;
                                    count += 1;
                                }
                                Value::Real(r) => {
                                    sum += r;
                                    count += 1;
                                }
                                _ => {}
                            }
                        }
                        if count > 0 {
                            Ok(Value::Real(sum / count as f64))
                        } else {
                            Ok(Value::Null)
                        }
                    }
                    _ => {
                        // Non-aggregate function - evaluate per row (shouldn't happen in aggregate context)
                        let planner = Planner::new(&self.catalog);
                        let compiled = planner.compile_expr_for_schema(expr, schema)?;
                        if let Some(row) = rows.first() {
                            eval_expr(&compiled, row, schema)
                        } else {
                            Ok(Value::Null)
                        }
                    }
                }
            }
            _ => {
                // Non-aggregate expression - evaluate with first row
                let planner = Planner::new(&self.catalog);
                let compiled = planner.compile_expr_for_schema(expr, schema)?;
                if let Some(row) = rows.first() {
                    eval_expr(&compiled, row, schema)
                } else {
                    Ok(Value::Null)
                }
            }
        }
    }

    fn execute_insert(&mut self, ins: &crate::parser::ast::InsertStmt) -> Result<QueryResult> {
        self.txn.begin_auto()?;

        let table = self.catalog.get_table(&ins.table)?.clone();
        let schema = table.schema();
        let columns = if let Some(cols) = &ins.columns {
            cols.clone()
        } else {
            schema
                .column_names()
                .iter()
                .map(|s| s.to_string())
                .collect()
        };

        let root_page = self.catalog.table_root_page(&ins.table).ok_or_else(|| {
            MiniDbError::Catalog(format!("no root page for table '{}'", ins.table))
        })?;

        let mut count = 0u64;
        for value_row in &ins.values {
            // Build full row with defaults
            let mut row = schema.default_row();

            for (i, expr) in value_row.iter().enumerate() {
                let col_name = columns
                    .get(i)
                    .ok_or_else(|| MiniDbError::Runtime("too many values for columns".into()))?;

                let col_idx = schema.column_index(col_name).ok_or_else(|| {
                    MiniDbError::Catalog(format!("column '{}' not found", col_name))
                })?;

                let val = self.eval_const_or_default(expr, &schema)?;
                let coerced = val.coerce_to(schema.columns[col_idx].col_type)?;
                row[col_idx] = coerced;
            }

            // Validate constraints
            self.validate_constraints(&table, &row)?;

            // Insert into heap
            let row_id = self.catalog.next_row_id(&ins.table);
            let page = self.pager.read_page_mut(PageId(root_page))?;
            let (_page_id, slot_idx, _row_id) = HeapFile::insert_row(page, &row, row_id)?;

            // Update indexes using slot_idx as the row identifier
            let indexes = self.catalog.indexes_for_table(&ins.table).to_vec();
            for idx in &indexes {
                if let Some(idx_root) = self.catalog.index_root_page(&idx.name) {
                    let col_idx = schema.column_index(&idx.column_name).ok_or_else(|| {
                        MiniDbError::Index(format!("column '{}' not found", idx.column_name))
                    })?;
                    let key = row[col_idx].clone();
                    let idx_page = self.pager.read_page_mut(PageId(idx_root))?;
                    BTree::new(PageId(idx_root)).insert(idx_page, &key, slot_idx as u64)?;
                }
            }

            count += 1;
        }

        if !self.txn.in_transaction() {
            self.flush_dirty_pages()?;
        }
        self.txn.auto_commit_if_needed()?;

        Ok(QueryResult::RowsAffected(count))
    }

    fn execute_update(&mut self, upd: &crate::parser::ast::UpdateStmt) -> Result<QueryResult> {
        self.txn.begin_auto()?;

        let table = self.catalog.get_table(&upd.table)?.clone();
        let schema = table.schema();
        let root_page = self.catalog.table_root_page(&upd.table).ok_or_else(|| {
            MiniDbError::Catalog(format!("no root page for table '{}'", upd.table))
        })?;

        let planner = Planner::new(&self.catalog);
        let compiled_plan = if let Some(pred) = &upd.where_clause {
            Some(planner.plan(&crate::parser::ast::Statement::Select(
                crate::parser::ast::SelectStmt {
                    distinct: false,
                    projection: vec![crate::parser::ast::SelectItem::Wildcard],
                    from: Some(crate::parser::ast::FromClause::Table {
                        name: upd.table.clone(),
                        alias: None,
                    }),
                    where_clause: Some(pred.clone()),
                    group_by: None,
                    having: None,
                    order_by: None,
                    limit: None,
                    offset: None,
                },
            ))?)
        } else {
            None
        };

        let filter = match &compiled_plan {
            Some(LogicalPlan::Filter { predicate, .. }) => Some(predicate.clone()),
            _ => None,
        };

        // Get all rows, apply filter, update matching ones
        let page = self.pager.read_page_mut(PageId(root_page))?;
        let all_rows = HeapFile::scan_page(page)?;

        let mut count = 0u64;

        // Compile assignment expressions
        let mut compiled_assignments = Vec::new();
        for (col_name, expr) in &upd.assignments {
            let compiled = planner.compile_expr_for_schema(expr, &schema)?;
            compiled_assignments.push((col_name.clone(), compiled));
        }

        for (slot_idx, row) in &all_rows {
            // Apply filter
            if let Some(ref pred) = filter {
                let result = eval_expr(pred, row, &schema)?;
                if !matches!(result, Value::Bool(true)) {
                    continue;
                }
            }

            // Build updated row
            let mut new_row = row.clone();
            for (col_name, expr) in &compiled_assignments {
                let col_idx = schema.column_index(col_name).ok_or_else(|| {
                    MiniDbError::Catalog(format!("column '{}' not found", col_name))
                })?;
                let val = eval_expr(expr, row, &schema)?;
                let coerced = val.coerce_to(schema.columns[col_idx].col_type)?;
                new_row[col_idx] = coerced;
            }

            HeapFile::update_row(page, *slot_idx, &new_row)?;
            count += 1;
        }

        if !self.txn.in_transaction() {
            self.flush_dirty_pages()?;
        }
        self.txn.auto_commit_if_needed()?;

        Ok(QueryResult::RowsAffected(count))
    }

    fn execute_delete(&mut self, del: &crate::parser::ast::DeleteStmt) -> Result<QueryResult> {
        self.txn.begin_auto()?;

        let table = self.catalog.get_table(&del.table)?.clone();
        let schema = table.schema();
        let root_page = self.catalog.table_root_page(&del.table).ok_or_else(|| {
            MiniDbError::Catalog(format!("no root page for table '{}'", del.table))
        })?;

        let planner = Planner::new(&self.catalog);
        let compiled_plan = if let Some(pred) = &del.where_clause {
            Some(planner.plan(&crate::parser::ast::Statement::Select(
                crate::parser::ast::SelectStmt {
                    distinct: false,
                    projection: vec![crate::parser::ast::SelectItem::Wildcard],
                    from: Some(crate::parser::ast::FromClause::Table {
                        name: del.table.clone(),
                        alias: None,
                    }),
                    where_clause: Some(pred.clone()),
                    group_by: None,
                    having: None,
                    order_by: None,
                    limit: None,
                    offset: None,
                },
            ))?)
        } else {
            None
        };

        let filter = match &compiled_plan {
            Some(LogicalPlan::Filter { predicate, .. }) => Some(predicate.clone()),
            _ => None,
        };

        let page = self.pager.read_page_mut(PageId(root_page))?;
        let all_rows = HeapFile::scan_page(page)?;

        let mut count = 0u64;

        for (slot_idx, row) in &all_rows {
            if let Some(ref pred) = filter {
                let result = eval_expr(pred, row, &schema)?;
                if !matches!(result, Value::Bool(true)) {
                    continue;
                }
            }

            HeapFile::delete_row(page, *slot_idx)?;
            count += 1;
        }

        if !self.txn.in_transaction() {
            self.flush_dirty_pages()?;
        }
        self.txn.auto_commit_if_needed()?;

        Ok(QueryResult::RowsAffected(count))
    }

    fn execute_create_table(
        &mut self,
        ct: &crate::parser::ast::CreateTableStmt,
    ) -> Result<QueryResult> {
        self.txn.begin_auto()?;

        let columns: Vec<ColumnDef> = ct
            .columns
            .iter()
            .map(|c| {
                let constraints = c
                    .constraints
                    .iter()
                    .map(|cc| match cc {
                        crate::parser::ast::ColumnConstraintAst::PrimaryKey => {
                            ColumnConstraint::PrimaryKey
                        }
                        crate::parser::ast::ColumnConstraintAst::NotNull => {
                            ColumnConstraint::NotNull
                        }
                        crate::parser::ast::ColumnConstraintAst::Unique => ColumnConstraint::Unique,
                        crate::parser::ast::ColumnConstraintAst::Default(expr) => {
                            // For now, only support literal defaults
                            let val = match expr {
                                crate::parser::ast::Expr::Literal(v) => v.clone(),
                                _ => Value::Null,
                            };
                            ColumnConstraint::Default(val)
                        }
                    })
                    .collect();

                ColumnDef {
                    name: c.name.clone(),
                    col_type: c.col_type,
                    constraints,
                }
            })
            .collect();

        let check_constraints: Vec<String> = ct
            .check_constraints
            .iter()
            .map(|e| format!("{:?}", e))
            .collect();

        let table_def = TableDef {
            name: ct.name.clone(),
            columns,
            check_constraints,
        };

        let root_page = self.pager.allocate_page()?;
        {
            let page = self.pager.read_page_mut(root_page)?;
            HeapFile::init_page(page);
        }

        self.catalog.create_table(table_def, root_page.0)?;
        self.save_catalog()?;

        if !self.txn.in_transaction() {
            self.flush_dirty_pages()?;
        }
        self.txn.auto_commit_if_needed()?;

        Ok(QueryResult::Success)
    }

    fn execute_drop_table(&mut self, name: &str) -> Result<QueryResult> {
        self.txn.begin_auto()?;

        self.catalog.drop_table(name)?;
        self.save_catalog()?;

        if !self.txn.in_transaction() {
            self.flush_dirty_pages()?;
        }
        self.txn.auto_commit_if_needed()?;

        Ok(QueryResult::Success)
    }

    fn execute_create_index(
        &mut self,
        ci: &crate::parser::ast::CreateIndexStmt,
    ) -> Result<QueryResult> {
        self.txn.begin_auto()?;

        let table = self.catalog.get_table(&ci.table)?;
        let _col_idx = table
            .schema()
            .column_index(&ci.column)
            .ok_or_else(|| MiniDbError::Catalog(format!("column '{}' not found", ci.column)))?;

        let root_page = self.pager.allocate_page()?;
        {
            let page = self.pager.read_page_mut(root_page)?;
            BTree::init_leaf(page);
        }

        // Build index from existing data
        let table_root = self
            .catalog
            .table_root_page(&ci.table)
            .ok_or_else(|| MiniDbError::Catalog("table has no root page".into()))?;
        let page = self.pager.read_page(PageId(table_root))?;
        if let Ok(all_rows) = HeapFile::scan_page(page) {
            let btree = BTree::new(root_page);
            for (slot_idx, row) in &all_rows {
                let key = row[_col_idx].clone();
                let idx_page = self.pager.read_page_mut(root_page)?;
                btree.insert(idx_page, &key, *slot_idx as u64)?;
            }
        }

        let index_def = IndexDef {
            name: ci.name.clone(),
            table_name: ci.table.clone(),
            column_name: ci.column.clone(),
        };

        self.catalog.create_index(index_def, root_page.0)?;
        self.save_catalog()?;

        if !self.txn.in_transaction() {
            self.flush_dirty_pages()?;
        }
        self.txn.auto_commit_if_needed()?;

        Ok(QueryResult::Success)
    }

    fn execute_drop_index(&mut self, name: &str) -> Result<QueryResult> {
        self.txn.begin_auto()?;

        self.catalog.drop_index(name)?;
        self.save_catalog()?;

        if !self.txn.in_transaction() {
            self.flush_dirty_pages()?;
        }
        self.txn.auto_commit_if_needed()?;

        Ok(QueryResult::Success)
    }

    fn execute_meta_schema(&self) -> Result<QueryResult> {
        let schema = Schema::new(vec![
            ColumnDef {
                name: "table".into(),
                col_type: ColumnType::Text,
                constraints: vec![],
            },
            ColumnDef {
                name: "column".into(),
                col_type: ColumnType::Text,
                constraints: vec![],
            },
            ColumnDef {
                name: "type".into(),
                col_type: ColumnType::Text,
                constraints: vec![],
            },
            ColumnDef {
                name: "constraints".into(),
                col_type: ColumnType::Text,
                constraints: vec![],
            },
        ]);

        let mut rows = Vec::new();
        for table in self.catalog.tables.values() {
            for col in &table.columns {
                let constraints: Vec<String> =
                    col.constraints.iter().map(|c| format!("{:?}", c)).collect();
                rows.push(vec![
                    Value::Text(table.name.clone()),
                    Value::Text(col.name.clone()),
                    Value::Text(col.col_type.to_string()),
                    Value::Text(constraints.join(", ")),
                ]);
            }
        }

        Ok(QueryResult::Rows { schema, rows })
    }

    fn execute_meta_tables(&self) -> Result<QueryResult> {
        let schema = Schema::new(vec![ColumnDef {
            name: "name".into(),
            col_type: ColumnType::Text,
            constraints: vec![],
        }]);

        let rows: Vec<Vec<Value>> = self
            .catalog
            .table_names()
            .iter()
            .map(|name| vec![Value::Text(name.to_string())])
            .collect();

        Ok(QueryResult::Rows { schema, rows })
    }

    fn execute_meta_indexes(&self) -> Result<QueryResult> {
        let schema = Schema::new(vec![
            ColumnDef {
                name: "name".into(),
                col_type: ColumnType::Text,
                constraints: vec![],
            },
            ColumnDef {
                name: "table".into(),
                col_type: ColumnType::Text,
                constraints: vec![],
            },
            ColumnDef {
                name: "column".into(),
                col_type: ColumnType::Text,
                constraints: vec![],
            },
        ]);

        let rows: Vec<Vec<Value>> = self
            .catalog
            .index_names()
            .iter()
            .filter_map(|name| self.catalog.get_index(name))
            .map(|idx| {
                vec![
                    Value::Text(idx.name.clone()),
                    Value::Text(idx.table_name.clone()),
                    Value::Text(idx.column_name.clone()),
                ]
            })
            .collect();

        Ok(QueryResult::Rows { schema, rows })
    }

    fn create_physical_plan(&self, plan: &LogicalPlan) -> Result<PhysicalPlan> {
        match plan {
            LogicalPlan::SeqScan {
                table,
                alias: _,
                filter,
            } => {
                let schema = self.catalog.get_schema(table)?;
                Ok(PhysicalPlan::SeqScan {
                    table: table.clone(),
                    schema,
                    filter: filter.clone(),
                })
            }
            LogicalPlan::IndexScan {
                table,
                alias: _,
                index_name,
                key,
            } => {
                let schema = self.catalog.get_schema(table)?;
                Ok(PhysicalPlan::IndexScan {
                    table: table.clone(),
                    schema,
                    index_name: index_name.clone(),
                    key: key.clone(),
                })
            }
            LogicalPlan::Filter { input, predicate } => {
                let phys_input = self.create_physical_plan(input)?;
                let schema = match &phys_input {
                    PhysicalPlan::SeqScan { schema, .. } => schema.clone(),
                    PhysicalPlan::IndexScan { schema, .. } => schema.clone(),
                    PhysicalPlan::Filter { schema, .. } => schema.clone(),
                    PhysicalPlan::Project { schema, .. } => schema.clone(),
                    PhysicalPlan::NestedLoopJoin { schema, .. } => schema.clone(),
                    PhysicalPlan::Sort { schema, .. } => schema.clone(),
                    PhysicalPlan::Limit { schema, .. } => schema.clone(),
                    PhysicalPlan::Empty { schema } => schema.clone(),
                    _ => Schema::new(vec![]),
                };
                Ok(PhysicalPlan::Filter {
                    input: Box::new(phys_input),
                    predicate: predicate.clone(),
                    schema,
                })
            }
            LogicalPlan::Project { input, exprs } => {
                let phys_input = self.create_physical_plan(input)?;
                let cols: Vec<ColumnDef> = exprs
                    .iter()
                    .map(|(_expr, alias)| {
                        let name = alias.clone().unwrap_or_else(|| "expr".to_string());
                        ColumnDef {
                            name,
                            col_type: ColumnType::Text,
                            constraints: vec![],
                        }
                    })
                    .collect();
                Ok(PhysicalPlan::Project {
                    input: Box::new(phys_input),
                    exprs: exprs.clone(),
                    schema: Schema::new(cols),
                })
            }
            LogicalPlan::NestedLoopJoin {
                left,
                right,
                condition,
                join_type,
            } => {
                let phys_left = self.create_physical_plan(left)?;
                let phys_right = self.create_physical_plan(right)?;

                let left_schema = self.physical_plan_schema(&phys_left);
                let right_schema = self.physical_plan_schema(&phys_right);

                // Qualify column names with table names to avoid ambiguity
                let left_table = Self::extract_table_name(left);
                let right_table = Self::extract_table_name(right);

                let mut cols: Vec<crate::types::ColumnDef> = left_schema
                    .columns
                    .iter()
                    .map(|c| {
                        let qualified_name = if let Some(ref tbl) = left_table {
                            format!("{}.{}", tbl, c.name)
                        } else {
                            c.name.clone()
                        };
                        crate::types::ColumnDef {
                            name: qualified_name,
                            col_type: c.col_type,
                            constraints: vec![],
                        }
                    })
                    .collect();
                for c in &right_schema.columns {
                    let qualified_name = if let Some(ref tbl) = right_table {
                        format!("{}.{}", tbl, c.name)
                    } else {
                        c.name.clone()
                    };
                    cols.push(crate::types::ColumnDef {
                        name: qualified_name,
                        col_type: c.col_type,
                        constraints: vec![],
                    });
                }

                Ok(PhysicalPlan::NestedLoopJoin {
                    left: Box::new(phys_left),
                    right: Box::new(phys_right),
                    condition: condition.clone(),
                    join_type: join_type.clone(),
                    schema: Schema::new(cols),
                })
            }
            LogicalPlan::Sort { input, keys } => {
                let phys_input = self.create_physical_plan(input)?;
                let schema = self.physical_plan_schema(&phys_input);
                Ok(PhysicalPlan::Sort {
                    input: Box::new(phys_input),
                    keys: keys.clone(),
                    schema,
                })
            }
            LogicalPlan::Limit {
                input,
                count,
                offset,
            } => {
                let phys_input = self.create_physical_plan(input)?;
                let schema = self.physical_plan_schema(&phys_input);
                Ok(PhysicalPlan::Limit {
                    input: Box::new(phys_input),
                    count: *count,
                    offset: *offset,
                    schema,
                })
            }
            LogicalPlan::Insert { .. } => Ok(PhysicalPlan::Empty {
                schema: Schema::new(vec![]),
            }),
            LogicalPlan::Update { .. } => Ok(PhysicalPlan::Empty {
                schema: Schema::new(vec![]),
            }),
            LogicalPlan::Delete { .. } => Ok(PhysicalPlan::Empty {
                schema: Schema::new(vec![]),
            }),
            LogicalPlan::Empty => Ok(PhysicalPlan::Empty {
                schema: Schema::new(vec![]),
            }),
        }
    }

    fn build_operator(&mut self, plan: PhysicalPlan) -> Result<Box<dyn PhysicalOperator>> {
        match plan {
            PhysicalPlan::SeqScan {
                table,
                schema,
                filter,
            } => {
                let root_page = self
                    .catalog
                    .table_root_page(&table)
                    .ok_or_else(|| MiniDbError::Catalog(format!("no root page for '{}'", table)))?;

                let page = self.pager.read_page(PageId(root_page))?;
                let rows = if HeapFile::scan_page(page).is_ok() {
                    HeapFile::scan_page(page)?
                        .into_iter()
                        .map(|(_, row)| row)
                        .collect()
                } else {
                    Vec::new()
                };

                Ok(Box::new(SeqScan {
                    table,
                    schema,
                    rows,
                    pos: 0,
                    filter,
                    alias: None,
                }))
            }
            PhysicalPlan::IndexScan {
                table,
                schema,
                index_name,
                key,
            } => {
                let idx_root = self.catalog.index_root_page(&index_name).ok_or_else(|| {
                    MiniDbError::Index(format!("no root page for '{}'", index_name))
                })?;

                let table_root = self
                    .catalog
                    .table_root_page(&table)
                    .ok_or_else(|| MiniDbError::Catalog(format!("no root page for '{}'", table)))?;

                // Evaluate the key
                let key_val = eval_expr(&key, &[], &schema)?;

                // Search the index
                let idx_page = self.pager.read_page(PageId(idx_root))?;
                let row_ids = BTree::search_eq(idx_page, &key_val)?;

                // Fetch the actual rows by slot index
                let heap_page = self.pager.read_page(PageId(table_root))?;
                let all_rows = HeapFile::scan_page(heap_page)?;
                let rows: Vec<Vec<Value>> = row_ids
                    .iter()
                    .filter_map(|rid| {
                        all_rows
                            .iter()
                            .find(|(slot, _)| *slot == *rid as u32)
                            .map(|(_, row)| row.clone())
                    })
                    .collect();

                Ok(Box::new(IndexScan {
                    table,
                    schema,
                    row_ids,
                    rows,
                    pos: 0,
                }))
            }
            PhysicalPlan::Filter {
                input,
                predicate,
                schema,
            } => {
                let input_op = self.build_operator(*input)?;
                Ok(Box::new(Filter {
                    input: input_op,
                    predicate,
                    schema,
                }))
            }
            PhysicalPlan::Project {
                input,
                exprs,
                schema,
            } => {
                let input_op = self.build_operator(*input)?;
                Ok(Box::new(Project {
                    input: input_op,
                    exprs,
                    schema,
                }))
            }
            PhysicalPlan::NestedLoopJoin {
                left,
                right,
                condition,
                join_type,
                schema,
            } => {
                let left_op = self.build_operator(*left)?;
                let right_op = self.build_operator(*right)?;
                Ok(Box::new(NestedLoopJoin {
                    left: left_op,
                    right: right_op,
                    condition,
                    schema,
                    left_row: None,
                    right_rows: Vec::new(),
                    right_pos: 0,
                    exhausted: false,
                    join_type,
                    matched_left: false,
                }))
            }
            PhysicalPlan::Sort {
                input,
                keys,
                schema,
            } => {
                let input_op = self.build_operator(*input)?;
                Ok(Box::new(Sort {
                    input: input_op,
                    keys,
                    schema,
                    sorted_rows: Vec::new(),
                    pos: 0,
                }))
            }
            PhysicalPlan::Limit {
                input,
                count,
                offset,
                schema,
            } => {
                let input_op = self.build_operator(*input)?;
                Ok(Box::new(Limit {
                    input: input_op,
                    count,
                    offset,
                    schema,
                    consumed: 0,
                    skipped: false,
                }))
            }
            PhysicalPlan::Insert { .. } => Ok(Box::new(InsertOp {
                table: String::new(),
                schema: Schema::new(vec![]),
                rows: vec![],
                rows_affected: 0,
            })),
            PhysicalPlan::Update { .. } => Ok(Box::new(UpdateOp {
                table: String::new(),
                schema: Schema::new(vec![]),
                rows_affected: 0,
            })),
            PhysicalPlan::Delete { .. } => Ok(Box::new(DeleteOp {
                table: String::new(),
                schema: Schema::new(vec![]),
                rows_affected: 0,
            })),
            PhysicalPlan::Empty { schema } => Ok(Box::new(SeqScan {
                table: String::new(),
                schema,
                rows: vec![vec![]], // single dummy row for SELECT without FROM
                pos: 0,
                filter: None,
                alias: None,
            })),
        }
    }

    fn extract_table_name(plan: &LogicalPlan) -> Option<String> {
        match plan {
            LogicalPlan::SeqScan { table, .. } => Some(table.clone()),
            LogicalPlan::IndexScan { table, .. } => Some(table.clone()),
            LogicalPlan::Filter { input, .. } => Self::extract_table_name(input),
            LogicalPlan::Sort { input, .. } => Self::extract_table_name(input),
            LogicalPlan::Limit { input, .. } => Self::extract_table_name(input),
            _ => None,
        }
    }

    fn physical_plan_schema(&self, plan: &PhysicalPlan) -> Schema {
        match plan {
            PhysicalPlan::SeqScan { schema, .. } => schema.clone(),
            PhysicalPlan::IndexScan { schema, .. } => schema.clone(),
            PhysicalPlan::Filter { schema, .. } => schema.clone(),
            PhysicalPlan::Project { schema, .. } => schema.clone(),
            PhysicalPlan::NestedLoopJoin { schema, .. } => schema.clone(),
            PhysicalPlan::Sort { schema, .. } => schema.clone(),
            PhysicalPlan::Limit { schema, .. } => schema.clone(),
            PhysicalPlan::Insert { schema, .. } => schema.clone(),
            PhysicalPlan::Update { schema, .. } => schema.clone(),
            PhysicalPlan::Delete { schema, .. } => schema.clone(),
            PhysicalPlan::Empty { schema } => schema.clone(),
        }
    }

    fn validate_constraints(&mut self, table: &TableDef, row: &[Value]) -> Result<()> {
        let schema = table.schema();

        for (i, col) in schema.columns.iter().enumerate() {
            for constraint in &col.constraints {
                match constraint {
                    ColumnConstraint::NotNull => {
                        if row[i].is_null() {
                            return Err(MiniDbError::Constraint(format!(
                                "NOT NULL constraint violated for column '{}'",
                                col.name
                            )));
                        }
                    }
                    ColumnConstraint::PrimaryKey => {
                        if row[i].is_null() {
                            return Err(MiniDbError::Constraint(format!(
                                "PRIMARY KEY constraint violated for column '{}': NULL",
                                col.name
                            )));
                        }
                        // Check uniqueness
                        if let Some(root) = self.catalog.table_root_page(&table.name) {
                            let page = self.pager.read_page(PageId(root))?;
                            if let Ok(existing) = HeapFile::scan_page(page) {
                                for (_, existing_row) in &existing {
                                    if existing_row[i] == row[i] {
                                        return Err(MiniDbError::Constraint(format!(
                                            "PRIMARY KEY constraint violated for column '{}': duplicate value",
                                            col.name
                                        )));
                                    }
                                }
                            }
                        }
                    }
                    ColumnConstraint::Unique => {
                        if let Some(root) = self.catalog.table_root_page(&table.name) {
                            let page = self.pager.read_page(PageId(root))?;
                            if let Ok(existing) = HeapFile::scan_page(page) {
                                for (_, existing_row) in &existing {
                                    if existing_row[i] == row[i] && !row[i].is_null() {
                                        return Err(MiniDbError::Constraint(format!(
                                            "UNIQUE constraint violated for column '{}'",
                                            col.name
                                        )));
                                    }
                                }
                            }
                        }
                    }
                    ColumnConstraint::Default(_) => {}
                }
            }
        }

        Ok(())
    }

    fn save_catalog(&mut self) -> Result<()> {
        let data = self.catalog.serialize();
        self.txn.log_catalog_update(&data)?;

        // Write catalog to page 0
        let catalog_page = self.pager.read_page_mut(PageId(0))?;
        catalog_page.write_bytes(0, &data);
        Ok(())
    }

    pub fn load_catalog(&mut self) -> Result<()> {
        if self.pager.page_count() > 0 {
            let page = self.pager.read_page(PageId(0))?;
            let data = page.read_bytes(0, PAGE_SIZE);
            // Find the end of the catalog data (null-terminated or first zero byte)
            let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
            if end > 0 {
                if let Ok(catalog) = Catalog::deserialize(&data[..end]) {
                    self.catalog = catalog;
                }
            }
        }
        Ok(())
    }

    pub fn recover_from_wal(&mut self) -> Result<()> {
        let replay = self.txn.replay_wal()?;

        for (_txn_id, page_id, data) in &replay.page_writes {
            let page = self.pager.read_page_mut(PageId(*page_id))?;
            let copy_len = data.len().min(PAGE_SIZE);
            page.write_bytes(0, &data[..copy_len]);
        }

        if let Some(catalog_data) = &replay.catalog_data {
            if let Ok(catalog) = Catalog::deserialize(catalog_data) {
                self.catalog = catalog;
            }
        }

        self.pager.flush()?;
        self.txn.checkpoint()?;

        Ok(())
    }

    fn flush_dirty_pages(&mut self) -> Result<()> {
        self.pager.flush()?;
        Ok(())
    }

    fn eval_const_or_default(
        &self,
        expr: &crate::parser::ast::Expr,
        _schema: &Schema,
    ) -> Result<Value> {
        match expr {
            crate::parser::ast::Expr::Literal(v) => Ok(v.clone()),
            _ => Ok(Value::Null),
        }
    }
}

/// Helper to evaluate simple expressions (for UPDATE SET clauses)
fn eval_simple_expr(
    expr: &crate::plan::CompiledExpr,
    row: &[Value],
    schema: &Schema,
) -> Result<Value> {
    eval_expr(expr, row, schema)
}

use crate::storage::page::PAGE_SIZE;

fn is_aggregate_expr(expr: &crate::parser::ast::Expr) -> bool {
    match expr {
        crate::parser::ast::Expr::FunctionCall { name, .. } => {
            matches!(
                name.to_uppercase().as_str(),
                "COUNT" | "SUM" | "AVG" | "MIN" | "MAX"
            )
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_engine() -> (Engine, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let wal_path = dir.path().join("test.wal");

        let pager = Pager::create(db_path.to_str().unwrap()).unwrap();
        let wal = crate::storage::Wal::create(wal_path.to_str().unwrap()).unwrap();
        let txn = TransactionManager::new(wal);
        let catalog = Catalog::new();

        let mut engine = Engine::new(pager, catalog, txn);

        // Allocate page 0 for catalog
        engine.pager.allocate_page().unwrap();

        (engine, dir)
    }

    #[test]
    fn test_create_table_and_insert() {
        let (mut engine, _dir) = create_test_engine();

        let ct = crate::parser::ast::CreateTableStmt {
            name: "users".into(),
            columns: vec![
                crate::parser::ast::ColumnDefAst {
                    name: "id".into(),
                    col_type: ColumnType::Int,
                    constraints: vec![crate::parser::ast::ColumnConstraintAst::PrimaryKey],
                },
                crate::parser::ast::ColumnDefAst {
                    name: "name".into(),
                    col_type: ColumnType::Text,
                    constraints: vec![],
                },
            ],
            check_constraints: vec![],
        };

        engine
            .execute(&crate::parser::ast::Statement::CreateTable(ct))
            .unwrap();

        let ins = crate::parser::ast::InsertStmt {
            table: "users".into(),
            columns: None,
            values: vec![vec![
                crate::parser::ast::Expr::Literal(Value::Int(1)),
                crate::parser::ast::Expr::Literal(Value::Text("Alice".into())),
            ]],
        };

        let result = engine
            .execute(&crate::parser::ast::Statement::Insert(ins))
            .unwrap();
        match result {
            QueryResult::RowsAffected(n) => assert_eq!(n, 1),
            _ => panic!("expected RowsAffected"),
        }
    }

    #[test]
    fn test_select_from_table() {
        let (mut engine, _dir) = create_test_engine();

        // Create table
        let ct = crate::parser::ast::CreateTableStmt {
            name: "items".into(),
            columns: vec![crate::parser::ast::ColumnDefAst {
                name: "val".into(),
                col_type: ColumnType::Int,
                constraints: vec![],
            }],
            check_constraints: vec![],
        };
        engine
            .execute(&crate::parser::ast::Statement::CreateTable(ct))
            .unwrap();

        // Insert
        for i in 1..=3 {
            let ins = crate::parser::ast::InsertStmt {
                table: "items".into(),
                columns: None,
                values: vec![vec![crate::parser::ast::Expr::Literal(Value::Int(i))]],
            };
            engine
                .execute(&crate::parser::ast::Statement::Insert(ins))
                .unwrap();
        }

        // Select
        let sel = crate::parser::ast::SelectStmt {
            distinct: false,
            projection: vec![crate::parser::ast::SelectItem::Wildcard],
            from: Some(crate::parser::ast::FromClause::Table {
                name: "items".into(),
                alias: None,
            }),
            where_clause: None,
            group_by: None,
            having: None,
            order_by: None,
            limit: None,
            offset: None,
        };

        let result = engine
            .execute(&crate::parser::ast::Statement::Select(sel))
            .unwrap();
        match result {
            QueryResult::Rows { rows, .. } => {
                assert_eq!(rows.len(), 3);
            }
            _ => panic!("expected Rows"),
        }
    }
}
