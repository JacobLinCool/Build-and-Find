use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::exec::operators::*;
use crate::parser::ast::*;
use crate::plan::*;
use crate::storage::btree::{BTreeIndex, RowId};
use crate::storage::heap::HeapFile;
use crate::storage::page::{PageId, PageManager};
use crate::txn::TransactionManager;
use crate::types::*;

pub struct Executor {
    pub pm: PageManager,
    pub catalog: Catalog,
    pub txn: TransactionManager,
    db_path: std::path::PathBuf,
}

pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub affected: usize,
    pub is_query: bool,
}

impl Executor {
    pub fn open(db_path: &std::path::Path) -> Result<Self> {
        let mut pm = PageManager::open(db_path)?;
        let txn = TransactionManager::new(db_path)?;

        // Recover from WAL
        txn.recover(&mut pm)?;
        let catalog = Catalog::load(&mut pm)?;

        Ok(Self {
            pm,
            catalog,
            txn: txn,
            db_path: db_path.to_path_buf(),
        })
    }

    pub fn create(db_path: &std::path::Path) -> Result<Self> {
        let mut pm = PageManager::create(db_path)?;
        let catalog = Catalog::init(&mut pm)?;
        let txn = TransactionManager::create(db_path)?;
        pm.flush()?;
        Ok(Self {
            pm,
            catalog,
            txn,
            db_path: db_path.to_path_buf(),
        })
    }

    pub fn execute(&mut self, stmt: Statement) -> Result<QueryResult> {
        match stmt {
            Statement::CreateTable(ct) => self.exec_create_table(ct),
            Statement::DropTable(dt) => self.exec_drop_table(dt),
            Statement::CreateIndex(ci) => self.exec_create_index(ci),
            Statement::DropIndex(di) => self.exec_drop_index(di),
            Statement::Insert(ins) => self.exec_insert(ins),
            Statement::Select(sel) => self.exec_select(sel),
            Statement::Update(upd) => self.exec_update(upd),
            Statement::Delete(del) => self.exec_delete(del),
            Statement::Begin => {
                self.txn.begin()?;
                Ok(QueryResult {
                    columns: vec![],
                    rows: vec![],
                    affected: 0,
                    is_query: false,
                })
            }
            Statement::Commit => {
                self.txn.commit(&mut self.pm)?;
                self.catalog.save(&mut self.pm)?;
                Ok(QueryResult {
                    columns: vec![],
                    rows: vec![],
                    affected: 0,
                    is_query: false,
                })
            }
            Statement::Rollback => {
                self.txn.rollback()?;
                // Reload page manager and catalog from restored DB file
                self.pm.reopen(&self.db_path)?;
                self.catalog = Catalog::load(&mut self.pm)?;
                Ok(QueryResult {
                    columns: vec![],
                    rows: vec![],
                    affected: 0,
                    is_query: false,
                })
            }
            Statement::MetaSchema => {
                let schema = self.catalog.schema_string();
                Ok(QueryResult {
                    columns: vec!["schema".to_string()],
                    rows: vec![vec![Value::Text(schema)]],
                    affected: 0,
                    is_query: true,
                })
            }
            Statement::MetaTables => {
                let mut names = self.catalog.table_names();
                names.sort();
                let rows: Vec<Vec<Value>> =
                    names.into_iter().map(|n| vec![Value::Text(n)]).collect();
                Ok(QueryResult {
                    columns: vec!["table_name".to_string()],
                    rows,
                    affected: 0,
                    is_query: true,
                })
            }
            Statement::MetaIndexes => {
                let mut names = self.catalog.index_names();
                names.sort();
                let rows: Vec<Vec<Value>> = names
                    .into_iter()
                    .map(|n| {
                        let idx = self.catalog.get_index(&n).unwrap();
                        vec![
                            Value::Text(n),
                            Value::Text(idx.table.clone()),
                            Value::Text(idx.column.clone()),
                        ]
                    })
                    .collect();
                Ok(QueryResult {
                    columns: vec![
                        "index_name".to_string(),
                        "table_name".to_string(),
                        "column_name".to_string(),
                    ],
                    rows,
                    affected: 0,
                    is_query: true,
                })
            }
            Statement::MetaExit => Err(MiniDbError::Runtime("exit".to_string())),
            Statement::MetaFormat(_) => Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                affected: 0,
                is_query: false,
            }),
        }
    }

    fn exec_create_table(&mut self, stmt: CreateTableStmt) -> Result<QueryResult> {
        let columns: Vec<ColumnDef> = stmt
            .columns
            .into_iter()
            .map(|c| ColumnDef {
                name: c.name,
                col_type: c.col_type,
                primary_key: c.primary_key,
                not_null: c.not_null,
                unique: c.unique,
                default: c.default,
            })
            .collect();

        let check_expr = stmt.check.map(|e| format!("{e:?}"));

        let schema = TableSchema {
            name: stmt.name.clone(),
            columns,
            check_expr,
        };

        self.catalog.create_table(schema)?;
        self.catalog.save(&mut self.pm)?;
        self.pm.flush()?;

        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected: 0,
            is_query: false,
        })
    }

    fn exec_drop_table(&mut self, stmt: DropTableStmt) -> Result<QueryResult> {
        self.catalog.drop_table(&stmt.name)?;
        self.catalog.save(&mut self.pm)?;
        self.pm.flush()?;
        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected: 0,
            is_query: false,
        })
    }

    fn exec_create_index(&mut self, stmt: CreateIndexStmt) -> Result<QueryResult> {
        let btree = BTreeIndex::create(&mut self.pm, &stmt.name)?;

        // Index existing data
        let table = self.catalog.get_table(&stmt.table).ok_or_else(|| {
            MiniDbError::Catalog(format!("table '{}' does not exist", stmt.table))
        })?;
        let columns = table.columns.clone();
        let col_idx = columns
            .iter()
            .position(|c| c.name == stmt.column)
            .ok_or_else(|| {
                MiniDbError::Catalog(format!(
                    "column '{}' not found in table '{}'",
                    stmt.column, stmt.table
                ))
            })?;

        let pages = self.catalog.get_table_pages(&stmt.table).to_vec();
        let heap = HeapFile::new(&stmt.table);
        for &pid in &pages {
            let rows = heap.scan_page(&mut self.pm, pid, &columns)?;
            for (slot_id, row) in rows {
                if let Some(val) = row.values.get(col_idx) {
                    let rid = RowId {
                        page_id: pid.0 as u32,
                        slot_id: slot_id as u32,
                    };
                    btree.insert(&mut self.pm, val, &rid)?;
                }
            }
        }

        let index_schema = IndexSchema {
            name: stmt.name,
            table: stmt.table,
            column: stmt.column,
        };
        self.catalog.create_index(index_schema, btree.root_page())?;
        self.catalog.save(&mut self.pm)?;
        self.pm.flush()?;

        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected: 0,
            is_query: false,
        })
    }

    fn exec_drop_index(&mut self, stmt: DropIndexStmt) -> Result<QueryResult> {
        self.catalog.drop_index(&stmt.name)?;
        self.catalog.save(&mut self.pm)?;
        self.pm.flush()?;
        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected: 0,
            is_query: false,
        })
    }

    fn exec_insert(&mut self, stmt: InsertStmt) -> Result<QueryResult> {
        let table = self.catalog.get_table(&stmt.table).ok_or_else(|| {
            MiniDbError::Catalog(format!("table '{}' does not exist", stmt.table))
        })?;
        let table_columns = table.columns.clone();
        let table_name = stmt.table.clone();

        // Determine column mapping
        let col_indices: Vec<usize> = if let Some(ref cols) = stmt.columns {
            cols.iter()
                .map(|c| {
                    table_columns
                        .iter()
                        .position(|tc| tc.name == *c)
                        .ok_or_else(|| {
                            MiniDbError::Catalog(format!(
                                "column '{}' not found in table '{}'",
                                c, table_name
                            ))
                        })
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            (0..table_columns.len()).collect()
        };

        // Ensure we have a page for this table
        let pages = self.catalog.get_table_pages(&table_name).to_vec();
        let heap = HeapFile::new(&table_name);
        let page_id = if pages.is_empty() {
            let _pid = self.catalog.alloc_page();
            let allocated = heap.alloc_page(&mut self.pm)?;
            // Use catalog's page tracking
            self.catalog.add_table_page(&table_name, allocated);
            allocated
        } else {
            *pages.last().unwrap()
        };

        let mut affected = 0;
        for value_row in &stmt.values {
            // Build full row with defaults
            let mut full_row = vec![Value::Null; table_columns.len()];
            for (i, col_idx) in col_indices.iter().enumerate() {
                let val = if i < value_row.len() {
                    // Evaluate literal expression
                    match &value_row[i] {
                        Expr::Literal(v) => v.clone(),
                        _ => {
                            return Err(MiniDbError::Runtime(
                                "only literal values supported in INSERT".to_string(),
                            ));
                        }
                    }
                } else if let Some(ref default) = table_columns[*col_idx].default {
                    default.clone()
                } else {
                    Value::Null
                };
                full_row[*col_idx] = val;
            }

            // Check constraints
            for (i, col) in table_columns.iter().enumerate() {
                if col.not_null && full_row[i].is_null() {
                    return Err(MiniDbError::Constraint(format!(
                        "NOT NULL constraint violated for column '{}'",
                        col.name
                    )));
                }
                if col.primary_key || col.unique {
                    // Check uniqueness
                    let pages = self.catalog.get_table_pages(&table_name).to_vec();
                    for pid in pages {
                        let existing = heap.scan_page(&mut self.pm, pid, &table_columns)?;
                        for (_, existing_row) in existing {
                            if existing_row.values[i] == full_row[i] {
                                if col.primary_key {
                                    return Err(MiniDbError::Constraint(format!(
                                        "PRIMARY KEY constraint violated for column '{}'",
                                        col.name
                                    )));
                                } else {
                                    return Err(MiniDbError::Constraint(format!(
                                        "UNIQUE constraint violated for column '{}'",
                                        col.name
                                    )));
                                }
                            }
                        }
                    }
                }
            }

            // Type coercion
            for (i, col) in table_columns.iter().enumerate() {
                if !full_row[i].is_null() {
                    full_row[i] = full_row[i].coerce_to(col.col_type)?;
                }
            }

            let final_row = Row::new(full_row);
            let slot_id = heap.insert_row(&mut self.pm, page_id, &final_row, &table_columns)?;

            // Update indexes
            let indexes = self.catalog.get_indexes_for_table(&table_name);
            for (idx_schema, root_page) in indexes {
                let col_idx = table_columns
                    .iter()
                    .position(|c| c.name == idx_schema.column);
                if let Some(ci) = col_idx {
                    if let Some(val) = final_row.values.get(ci) {
                        let btree = BTreeIndex::new(&idx_schema.name, root_page);
                        let rid = RowId {
                            page_id: page_id.0 as u32,
                            slot_id: slot_id as u32,
                        };
                        btree.insert(&mut self.pm, val, &rid)?;
                    }
                }
            }

            affected += 1;
        }

        self.catalog.save(&mut self.pm)?;
        self.pm.flush()?;

        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected,
            is_query: false,
        })
    }

    fn exec_select(&mut self, stmt: SelectStmt) -> Result<QueryResult> {
        let planner = Planner::new(&self.catalog);
        let plan = planner.plan_select(&stmt)?;

        // Execute the plan
        let (columns, rows) = self.execute_plan(plan)?;

        Ok(QueryResult {
            columns,
            rows,
            affected: 0,
            is_query: true,
        })
    }

    fn execute_plan(&mut self, plan: LogicalPlan) -> Result<(Vec<String>, Vec<Vec<Value>>)> {
        match plan {
            LogicalPlan::SeqScan { table, filter } => {
                if table.is_empty() {
                    // SELECT without FROM - return one empty row so Project can evaluate
                    return Ok((vec![], vec![vec![]]));
                }
                let table_schema = self.catalog.get_table(&table).ok_or_else(|| {
                    MiniDbError::Catalog(format!("table '{table}' does not exist"))
                })?;
                let columns = table_schema.columns.clone();
                let pages = self.catalog.get_table_pages(&table).to_vec();
                let heap = HeapFile::new(&table);
                let aliases = vec![table];

                let mut result_rows = Vec::new();
                for &pid in &pages {
                    let rows = heap.scan_page(&mut self.pm, pid, &columns)?;
                    for (_, row) in rows {
                        if let Some(ref filter_expr) = filter {
                            let ctx = EvalContext {
                                columns: &columns,
                                table_aliases: &aliases,
                                current_row: &row,
                            };
                            let val = eval_expr(filter_expr, &ctx)?;
                            match val {
                                Value::Bool(true) => {}
                                _ => continue,
                            }
                        }
                        result_rows.push(row);
                    }
                }

                let col_names = columns.iter().map(|c| c.name.clone()).collect();
                let result_values: Vec<Vec<Value>> =
                    result_rows.into_iter().map(|r| r.values).collect();
                Ok((col_names, result_values))
            }
            LogicalPlan::IndexScan {
                table,
                index_name,
                key,
                filter,
            } => {
                let root_page = self.catalog.get_index_root(&index_name).ok_or_else(|| {
                    MiniDbError::Catalog(format!("index '{index_name}' not found"))
                })?;
                let btree = BTreeIndex::new(&index_name, root_page);
                let row_ids = btree.search(&mut self.pm, &key)?;

                let table_schema = self.catalog.get_table(&table).ok_or_else(|| {
                    MiniDbError::Catalog(format!("table '{table}' does not exist"))
                })?;
                let columns = table_schema.columns.clone();
                let heap = HeapFile::new(&table);
                let aliases = vec![table];

                let mut result_rows = Vec::new();
                for rid in row_ids {
                    let pid = PageId(rid.page_id as u64);
                    let page_rows = heap.scan_page(&mut self.pm, pid, &columns)?;
                    for (slot_id, row) in page_rows {
                        if slot_id == rid.slot_id as u16 {
                            if let Some(ref filter_expr) = filter {
                                let ctx = EvalContext {
                                    columns: &columns,
                                    table_aliases: &aliases,
                                    current_row: &row,
                                };
                                let val = eval_expr(filter_expr, &ctx)?;
                                match val {
                                    Value::Bool(true) => {}
                                    _ => continue,
                                }
                            }
                            result_rows.push(row);
                        }
                    }
                }

                let col_names = columns.iter().map(|c| c.name.clone()).collect();
                let result_values: Vec<Vec<Value>> =
                    result_rows.into_iter().map(|r| r.values).collect();
                Ok((col_names, result_values))
            }
            LogicalPlan::Filter { input, predicate } => {
                let (col_names, input_rows) = self.execute_plan(*input)?;
                let columns: Vec<ColumnDef> = col_names
                    .iter()
                    .map(|n| ColumnDef {
                        name: n.clone(),
                        col_type: ColumnType::Text, // doesn't matter for eval
                        primary_key: false,
                        not_null: false,
                        unique: false,
                        default: None,
                    })
                    .collect();
                let aliases = vec!["".to_string()];

                let mut result_rows = Vec::new();
                for values in input_rows {
                    let row = Row::new(values.clone());
                    let ctx = EvalContext {
                        columns: &columns,
                        table_aliases: &aliases,
                        current_row: &row,
                    };
                    let val = eval_expr(&predicate, &ctx)?;
                    match val {
                        Value::Bool(true) => result_rows.push(values),
                        _ => {}
                    }
                }
                Ok((col_names, result_rows))
            }
            LogicalPlan::Project { input, projections } => {
                let (input_cols, input_rows) = self.execute_plan(*input)?;
                let columns: Vec<ColumnDef> = input_cols
                    .iter()
                    .map(|n| ColumnDef {
                        name: n.clone(),
                        col_type: ColumnType::Text,
                        primary_key: false,
                        not_null: false,
                        unique: false,
                        default: None,
                    })
                    .collect();
                let aliases = vec!["".to_string()];

                let mut result_rows = Vec::new();
                for values in input_rows {
                    let row = Row::new(values.clone());
                    let ctx = EvalContext {
                        columns: &columns,
                        table_aliases: &aliases,
                        current_row: &row,
                    };
                    let mut proj_values = Vec::new();
                    for proj in &projections {
                        let val = eval_expr(&proj.expr, &ctx)?;
                        proj_values.push(val);
                    }
                    result_rows.push(proj_values);
                }

                let col_names = projections.iter().map(|p| p.alias.clone()).collect();
                Ok((col_names, result_rows))
            }
            LogicalPlan::NestedLoopJoin { left, right, on } => {
                let (left_cols, left_rows) = self.execute_plan(*left)?;
                let (right_cols, right_rows) = self.execute_plan(*right)?;

                let mut combined_cols = left_cols.clone();
                combined_cols.extend_from_slice(&right_cols);
                let columns: Vec<ColumnDef> = combined_cols
                    .iter()
                    .map(|n| ColumnDef {
                        name: n.clone(),
                        col_type: ColumnType::Text,
                        primary_key: false,
                        not_null: false,
                        unique: false,
                        default: None,
                    })
                    .collect();
                let aliases = vec!["".to_string()];

                let mut result_rows = Vec::new();
                for left_vals in &left_rows {
                    for right_vals in &right_rows {
                        let mut combined = left_vals.clone();
                        combined.extend_from_slice(right_vals);
                        let row = Row::new(combined.clone());
                        let ctx = EvalContext {
                            columns: &columns,
                            table_aliases: &aliases,
                            current_row: &row,
                        };
                        let val = eval_expr(&on, &ctx)?;
                        match val {
                            Value::Bool(true) => result_rows.push(combined),
                            _ => {}
                        }
                    }
                }
                Ok((combined_cols, result_rows))
            }
            LogicalPlan::Sort { input, keys } => {
                let (col_names, mut rows) = self.execute_plan(*input)?;
                let columns: Vec<ColumnDef> = col_names
                    .iter()
                    .map(|n| ColumnDef {
                        name: n.clone(),
                        col_type: ColumnType::Text,
                        primary_key: false,
                        not_null: false,
                        unique: false,
                        default: None,
                    })
                    .collect();
                let aliases = vec!["".to_string()];

                rows.sort_by(|a, b| {
                    let row_a = Row::new(a.clone());
                    let row_b = Row::new(b.clone());
                    let ctx_a = EvalContext {
                        columns: &columns,
                        table_aliases: &aliases,
                        current_row: &row_a,
                    };
                    let ctx_b = EvalContext {
                        columns: &columns,
                        table_aliases: &aliases,
                        current_row: &row_b,
                    };
                    for (expr, asc) in &keys {
                        let val_a = eval_expr(expr, &ctx_a).unwrap_or(Value::Null);
                        let val_b = eval_expr(expr, &ctx_b).unwrap_or(Value::Null);
                        let ord = val_a.compare_nulls_last(&val_b);
                        if ord != std::cmp::Ordering::Equal {
                            return if *asc { ord } else { ord.reverse() };
                        }
                    }
                    std::cmp::Ordering::Equal
                });
                Ok((col_names, rows))
            }
            LogicalPlan::Limit {
                input,
                limit,
                offset,
            } => {
                let (col_names, rows) = self.execute_plan(*input)?;
                let rows: Vec<Vec<Value>> = rows.into_iter().skip(offset).take(limit).collect();
                Ok((col_names, rows))
            }
            LogicalPlan::Insert { .. } => {
                // Should be handled by exec_insert
                unreachable!()
            }
            LogicalPlan::Update { .. } => {
                // Should be handled by exec_update
                unreachable!()
            }
            LogicalPlan::Delete { .. } => {
                // Should be handled by exec_delete
                unreachable!()
            }
        }
    }

    fn exec_update(&mut self, stmt: UpdateStmt) -> Result<QueryResult> {
        let table_name = stmt.table.clone();
        let table = self
            .catalog
            .get_table(&table_name)
            .ok_or_else(|| MiniDbError::Catalog(format!("table '{table_name}' does not exist")))?;
        let columns = table.columns.clone();
        let pages = self.catalog.get_table_pages(&table_name).to_vec();
        let heap = HeapFile::new(&table_name);
        let aliases = vec![table_name.clone()];

        // Build column index map for assignments
        let assign_indices: Vec<(usize, Expr)> = stmt
            .assignments
            .iter()
            .map(|(name, expr)| {
                let idx = columns
                    .iter()
                    .position(|c| c.name == *name)
                    .ok_or_else(|| MiniDbError::Catalog(format!("column '{name}' not found")))?;
                Ok((idx, expr.clone()))
            })
            .collect::<Result<Vec<_>>>()?;

        let mut affected = 0;
        for &pid in &pages {
            let rows = heap.scan_page(&mut self.pm, pid, &columns)?;
            for (slot_id, row) in rows {
                // Check filter
                if let Some(ref filter) = stmt.where_clause {
                    let ctx = EvalContext {
                        columns: &columns,
                        table_aliases: &aliases,
                        current_row: &row,
                    };
                    let val = eval_expr(filter, &ctx)?;
                    match val {
                        Value::Bool(true) => {}
                        _ => continue,
                    }
                }

                // Apply updates
                let mut new_values = row.values.clone();
                for (idx, expr) in &assign_indices {
                    let ctx = EvalContext {
                        columns: &columns,
                        table_aliases: &aliases,
                        current_row: &row,
                    };
                    let val = eval_expr(expr, &ctx)?;
                    new_values[*idx] = val;
                }

                let new_row = Row::new(new_values);

                // Check constraints
                for (i, col) in columns.iter().enumerate() {
                    if col.not_null && new_row.values[i].is_null() {
                        return Err(MiniDbError::Constraint(format!(
                            "NOT NULL constraint violated for column '{}'",
                            col.name
                        )));
                    }
                }

                heap.update_row(&mut self.pm, pid, slot_id, &new_row, &columns)?;
                affected += 1;
            }
        }

        self.pm.flush()?;

        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected,
            is_query: false,
        })
    }

    fn exec_delete(&mut self, stmt: DeleteStmt) -> Result<QueryResult> {
        let table_name = stmt.table.clone();
        let table = self
            .catalog
            .get_table(&table_name)
            .ok_or_else(|| MiniDbError::Catalog(format!("table '{table_name}' does not exist")))?;
        let columns = table.columns.clone();
        let pages = self.catalog.get_table_pages(&table_name).to_vec();
        let heap = HeapFile::new(&table_name);
        let aliases = vec![table_name.clone()];

        let mut affected = 0;
        for &pid in &pages {
            let rows = heap.scan_page(&mut self.pm, pid, &columns)?;
            for (slot_id, row) in rows {
                if let Some(ref filter) = stmt.where_clause {
                    let ctx = EvalContext {
                        columns: &columns,
                        table_aliases: &aliases,
                        current_row: &row,
                    };
                    let val = eval_expr(filter, &ctx)?;
                    match val {
                        Value::Bool(true) => {}
                        _ => continue,
                    }
                }
                heap.delete_row(&mut self.pm, pid, slot_id)?;
                affected += 1;
            }
        }

        self.pm.flush()?;

        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected,
            is_query: false,
        })
    }
}
