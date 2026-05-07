/// Volcano-style physical operators.
///
/// Each operator implements PhysicalOperator with open/next/close/schema.
use crate::catalog::{Catalog, IndexMeta, TableMeta};
use crate::error::{MiniDbError, Result};
use crate::exec::eval::{eval_expr, eval_predicate};
use crate::parser::ast::*;
use crate::plan::PhysicalPlan;
use crate::storage::{BTree, BTreeKey, Heap, PageId, Pager, WalRecord};
use crate::txn::TransactionManager;
use crate::types::{Column, DataType, Row, Schema, Value};
use std::collections::HashMap;

/// Shared execution context (passed by &mut to operators)
pub struct ExecutionContext {
    pub pager: Pager,
    pub catalog: Catalog,
    pub txn: TransactionManager,
}

impl ExecutionContext {
    pub fn new(pager: Pager, catalog: Catalog, txn: TransactionManager) -> Self {
        ExecutionContext {
            pager,
            catalog,
            txn,
        }
    }
}

/// The Volcano iterator trait
pub trait PhysicalOperator {
    fn schema(&self) -> &Schema;
    fn open(&mut self, ctx: &mut ExecutionContext) -> Result<()>;
    fn next(&mut self, ctx: &mut ExecutionContext) -> Result<Option<Row>>;
    fn close(&mut self, ctx: &mut ExecutionContext) -> Result<()>;
}

// ─── EmptyOperator ────────────────────────────────────────────────────────────

pub struct EmptyOperator {
    schema: Schema,
}

impl EmptyOperator {
    pub fn new() -> Self {
        EmptyOperator {
            schema: Schema::default(),
        }
    }
}

impl PhysicalOperator for EmptyOperator {
    fn schema(&self) -> &Schema {
        &self.schema
    }
    fn open(&mut self, _: &mut ExecutionContext) -> Result<()> {
        Ok(())
    }
    fn next(&mut self, _: &mut ExecutionContext) -> Result<Option<Row>> {
        Ok(None)
    }
    fn close(&mut self, _: &mut ExecutionContext) -> Result<()> {
        Ok(())
    }
}

// ─── SeqScan ──────────────────────────────────────────────────────────────────

pub struct SeqScan {
    table: String,
    alias: Option<String>,
    predicate: Option<Expr>,
    schema: Schema,
    rows: Vec<Row>,
    pos: usize,
}

impl SeqScan {
    pub fn new(
        table: String,
        alias: Option<String>,
        predicate: Option<Expr>,
        ctx: &ExecutionContext,
    ) -> Result<Self> {
        let table_meta = ctx.catalog.get_table(&table)?;
        let schema = table_meta.schema.clone();
        Ok(SeqScan {
            table,
            alias,
            predicate,
            schema,
            rows: Vec::new(),
            pos: 0,
        })
    }
}

impl PhysicalOperator for SeqScan {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn open(&mut self, ctx: &mut ExecutionContext) -> Result<()> {
        let table_meta = ctx.catalog.get_table(&self.table)?;
        let heap_root = PageId(table_meta.heap_root);
        let schema = table_meta.schema.clone();
        let predicate = self.predicate.clone();

        let heap = Heap::open(heap_root);
        let mut rows = Vec::new();

        heap.scan(&mut ctx.pager, |_pid, _slot, data| {
            if data.len() < 8 {
                return Ok(true);
            }
            let _row_id = u64::from_le_bytes(data[..8].try_into().unwrap());
            let row = schema.deserialize_row(&data[8..])?;

            if let Some(pred) = &predicate {
                if eval_predicate(pred, &row, &schema)? != Some(true) {
                    return Ok(true);
                }
            }
            rows.push(row);
            Ok(true)
        })?;

        self.rows = rows;
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self, _: &mut ExecutionContext) -> Result<Option<Row>> {
        if self.pos < self.rows.len() {
            let row = self.rows[self.pos].clone();
            self.pos += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self, _: &mut ExecutionContext) -> Result<()> {
        self.rows.clear();
        Ok(())
    }
}

// ─── IndexScan ────────────────────────────────────────────────────────────────

pub struct IndexScan {
    table: String,
    alias: Option<String>,
    index_name: String,
    index_col: String,
    eq_val: Option<Expr>,
    range_lo: Option<Expr>,
    range_hi: Option<Expr>,
    predicate: Option<Expr>,
    schema: Schema,
    rows: Vec<Row>,
    pos: usize,
}

impl IndexScan {
    pub fn new(
        table: String,
        alias: Option<String>,
        index_name: String,
        index_col: String,
        eq_val: Option<Expr>,
        range_lo: Option<Expr>,
        range_hi: Option<Expr>,
        predicate: Option<Expr>,
        ctx: &ExecutionContext,
    ) -> Result<Self> {
        let table_meta = ctx.catalog.get_table(&table)?;
        let schema = table_meta.schema.clone();
        Ok(IndexScan {
            table,
            alias,
            index_name,
            index_col,
            eq_val,
            range_lo,
            range_hi,
            predicate,
            schema,
            rows: Vec::new(),
            pos: 0,
        })
    }
}

impl PhysicalOperator for IndexScan {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn open(&mut self, ctx: &mut ExecutionContext) -> Result<()> {
        // Evaluate key expressions
        let empty_row: Vec<Value> = vec![];
        let empty_schema = Schema::default();

        let idx_meta = ctx.catalog.get_index(&self.index_name)?.clone();
        let table_meta = ctx.catalog.get_table(&self.table)?.clone();
        let heap_root = PageId(table_meta.heap_root);
        let btree_root = PageId(idx_meta.btree_root);
        let schema = table_meta.schema.clone();
        let predicate = self.predicate.clone();

        let row_ids: Vec<u64> = if let Some(eq_expr) = &self.eq_val {
            let eq_val = eval_expr(eq_expr, &empty_row, &empty_schema)?;
            let key_bytes = eq_val.to_bytes();
            let tree = BTree::open(btree_root);
            tree.search_exact(&mut ctx.pager, &key_bytes)?
        } else {
            let lo_bytes = if let Some(lo_expr) = &self.range_lo {
                let lo_val = eval_expr(lo_expr, &empty_row, &empty_schema)?;
                Some(lo_val.to_bytes())
            } else {
                None
            };
            let hi_bytes = if let Some(hi_expr) = &self.range_hi {
                let hi_val = eval_expr(hi_expr, &empty_row, &empty_schema)?;
                Some(hi_val.to_bytes())
            } else {
                None
            };
            let tree = BTree::open(btree_root);
            let results =
                tree.range_scan(&mut ctx.pager, lo_bytes.as_deref(), hi_bytes.as_deref())?;
            results.into_iter().map(|(_, rid)| rid).collect()
        };

        // Fetch rows by row_id
        let heap = Heap::open(heap_root);
        let mut rows = Vec::new();
        let row_id_set: std::collections::HashSet<u64> = row_ids.into_iter().collect();

        heap.scan(&mut ctx.pager, |_pid, _slot, data| {
            if data.len() < 8 {
                return Ok(true);
            }
            let row_id = u64::from_le_bytes(data[..8].try_into().unwrap());
            if !row_id_set.contains(&row_id) {
                return Ok(true);
            }

            let row = schema.deserialize_row(&data[8..])?;
            if let Some(pred) = &predicate {
                if eval_predicate(pred, &row, &schema)? != Some(true) {
                    return Ok(true);
                }
            }
            rows.push(row);
            Ok(true)
        })?;

        self.rows = rows;
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self, _: &mut ExecutionContext) -> Result<Option<Row>> {
        if self.pos < self.rows.len() {
            let row = self.rows[self.pos].clone();
            self.pos += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self, _: &mut ExecutionContext) -> Result<()> {
        self.rows.clear();
        Ok(())
    }
}

// ─── Filter ───────────────────────────────────────────────────────────────────

pub struct Filter {
    input: Box<dyn PhysicalOperator>,
    predicate: Expr,
    schema: Schema,
}

impl Filter {
    pub fn new(input: Box<dyn PhysicalOperator>, predicate: Expr) -> Self {
        let schema = input.schema().clone();
        Filter {
            input,
            predicate,
            schema,
        }
    }
}

impl PhysicalOperator for Filter {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn open(&mut self, ctx: &mut ExecutionContext) -> Result<()> {
        self.input.open(ctx)
    }

    fn next(&mut self, ctx: &mut ExecutionContext) -> Result<Option<Row>> {
        while let Some(row) = self.input.next(ctx)? {
            if eval_predicate(&self.predicate, &row, &self.schema)? == Some(true) {
                return Ok(Some(row));
            }
        }
        Ok(None)
    }

    fn close(&mut self, ctx: &mut ExecutionContext) -> Result<()> {
        self.input.close(ctx)
    }
}

// ─── Project ──────────────────────────────────────────────────────────────────

pub struct Project {
    input: Box<dyn PhysicalOperator>,
    items: Vec<SelectItem>,
    input_schema: Schema,
    output_schema: Schema,
}

impl Project {
    pub fn new(
        input: Box<dyn PhysicalOperator>,
        items: Vec<SelectItem>,
        ctx: &ExecutionContext,
    ) -> Result<Self> {
        let input_schema = input.schema().clone();
        let output_schema = compute_output_schema(&items, &input_schema)?;
        Ok(Project {
            input,
            items,
            input_schema,
            output_schema,
        })
    }
}

fn compute_output_schema(items: &[SelectItem], input: &Schema) -> Result<Schema> {
    let mut cols = Vec::new();
    for item in items {
        match item {
            SelectItem::Wildcard => {
                cols.extend(input.columns.clone());
            }
            SelectItem::QualifiedWildcard(tbl) => {
                // Include all columns (simplified: we don't track table aliases at schema level)
                cols.extend(input.columns.clone());
            }
            SelectItem::Expr { expr, alias } => {
                let name = alias.clone().unwrap_or_else(|| expr_name(expr));
                let dt = infer_expr_type(expr, input).unwrap_or(DataType::Text);
                cols.push(Column::new(name, dt));
            }
        }
    }
    Ok(Schema::new(cols))
}

fn expr_name(expr: &Expr) -> String {
    match expr {
        Expr::Column(c) => c.column.clone(),
        Expr::Literal(v) => v.to_string(),
        Expr::FunctionCall { name, .. } => name.clone(),
        Expr::BinOp { op, left, right } => format!("expr"),
        _ => "col".to_string(),
    }
}

fn infer_expr_type(expr: &Expr, schema: &Schema) -> Option<DataType> {
    match expr {
        Expr::Literal(v) => Some(v.data_type()),
        Expr::Column(c) => {
            let idx = schema.find_column(&c.column)?;
            Some(schema.columns[idx].data_type.clone())
        }
        Expr::BinOp { op, left, right } => match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => infer_expr_type(left, schema),
            _ => Some(DataType::Bool),
        },
        Expr::FunctionCall { name, .. } => match name.as_str() {
            "LENGTH" | "ABS" => Some(DataType::Int),
            "LOWER" | "UPPER" | "SUBSTR" | "COALESCE" => Some(DataType::Text),
            _ => None,
        },
        _ => None,
    }
}

impl PhysicalOperator for Project {
    fn schema(&self) -> &Schema {
        &self.output_schema
    }

    fn open(&mut self, ctx: &mut ExecutionContext) -> Result<()> {
        self.input.open(ctx)
    }

    fn next(&mut self, ctx: &mut ExecutionContext) -> Result<Option<Row>> {
        match self.input.next(ctx)? {
            None => Ok(None),
            Some(row) => {
                let projected = project_row(&self.items, &row, &self.input_schema)?;
                Ok(Some(projected))
            }
        }
    }

    fn close(&mut self, ctx: &mut ExecutionContext) -> Result<()> {
        self.input.close(ctx)
    }
}

fn project_row(items: &[SelectItem], row: &Row, schema: &Schema) -> Result<Row> {
    let mut result = Vec::new();
    for item in items {
        match item {
            SelectItem::Wildcard => result.extend(row.clone()),
            SelectItem::QualifiedWildcard(_) => result.extend(row.clone()),
            SelectItem::Expr { expr, .. } => {
                result.push(eval_expr(expr, row, schema)?);
            }
        }
    }
    Ok(result)
}

// ─── NestedLoopJoin ───────────────────────────────────────────────────────────

pub struct NestedLoopJoin {
    left: Box<dyn PhysicalOperator>,
    right: Box<dyn PhysicalOperator>,
    join_type: JoinType,
    condition: Option<Expr>,
    schema: Schema,
    left_row: Option<Row>,
    right_buffer: Vec<Row>,
    right_pos: usize,
    left_matched: bool,
    left_rows: Vec<Row>,
    left_pos: usize,
}

impl NestedLoopJoin {
    pub fn new(
        left: Box<dyn PhysicalOperator>,
        right: Box<dyn PhysicalOperator>,
        join_type: JoinType,
        condition: Option<Expr>,
    ) -> Self {
        let left_schema = left.schema();
        let right_schema = right.schema();
        let mut combined_cols = left_schema.columns.clone();
        combined_cols.extend(right_schema.columns.clone());
        let schema = Schema::new(combined_cols);
        NestedLoopJoin {
            left,
            right,
            join_type,
            condition,
            schema,
            left_row: None,
            right_buffer: Vec::new(),
            right_pos: 0,
            left_matched: false,
            left_rows: Vec::new(),
            left_pos: 0,
        }
    }
}

impl PhysicalOperator for NestedLoopJoin {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn open(&mut self, ctx: &mut ExecutionContext) -> Result<()> {
        self.left.open(ctx)?;
        self.right.open(ctx)?;

        // Materialize both sides
        while let Some(row) = self.left.next(ctx)? {
            self.left_rows.push(row);
        }
        while let Some(row) = self.right.next(ctx)? {
            self.right_buffer.push(row);
        }
        self.left_pos = 0;
        self.right_pos = 0;
        Ok(())
    }

    fn next(&mut self, ctx: &mut ExecutionContext) -> Result<Option<Row>> {
        let left_schema = self.left.schema().clone();
        let right_schema = self.right.schema().clone();

        loop {
            if self.left_pos >= self.left_rows.len() {
                return Ok(None);
            }

            if self.right_pos >= self.right_buffer.len() {
                // Move to next left row
                if self.join_type == JoinType::Left && !self.left_matched {
                    // Emit left row with NULLs for right
                    let lrow = self.left_rows[self.left_pos].clone();
                    let nulls: Row = right_schema.columns.iter().map(|_| Value::Null).collect();
                    let combined: Row = lrow.iter().chain(nulls.iter()).cloned().collect();
                    self.left_pos += 1;
                    self.right_pos = 0;
                    self.left_matched = false;
                    return Ok(Some(combined));
                }
                self.left_pos += 1;
                self.right_pos = 0;
                self.left_matched = false;
                continue;
            }

            let lrow = &self.left_rows[self.left_pos];
            let rrow = &self.right_buffer[self.right_pos];
            self.right_pos += 1;

            let combined: Row = lrow.iter().chain(rrow.iter()).cloned().collect();

            if let Some(cond) = &self.condition {
                if eval_predicate(cond, &combined, &self.schema)? != Some(true) {
                    continue;
                }
            }

            self.left_matched = true;
            return Ok(Some(combined));
        }
    }

    fn close(&mut self, ctx: &mut ExecutionContext) -> Result<()> {
        self.left.close(ctx)?;
        self.right.close(ctx)?;
        self.left_rows.clear();
        self.right_buffer.clear();
        Ok(())
    }
}

// ─── Sort ─────────────────────────────────────────────────────────────────────

pub struct Sort {
    input: Box<dyn PhysicalOperator>,
    order_by: Vec<OrderByItem>,
    schema: Schema,
    sorted_rows: Vec<Row>,
    pos: usize,
}

impl Sort {
    pub fn new(input: Box<dyn PhysicalOperator>, order_by: Vec<OrderByItem>) -> Self {
        let schema = input.schema().clone();
        Sort {
            input,
            order_by,
            schema,
            sorted_rows: Vec::new(),
            pos: 0,
        }
    }
}

impl PhysicalOperator for Sort {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn open(&mut self, ctx: &mut ExecutionContext) -> Result<()> {
        self.input.open(ctx)?;
        let mut rows = Vec::new();
        while let Some(row) = self.input.next(ctx)? {
            rows.push(row);
        }

        let schema = self.schema.clone();
        let order_by = self.order_by.clone();

        rows.sort_by(|a, b| {
            for item in &order_by {
                let av = eval_expr(&item.expr, a, &schema).unwrap_or(Value::Null);
                let bv = eval_expr(&item.expr, b, &schema).unwrap_or(Value::Null);
                let ord = av.compare(&bv).unwrap_or(std::cmp::Ordering::Equal);
                let ord = if item.ascending { ord } else { ord.reverse() };
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            std::cmp::Ordering::Equal
        });

        self.sorted_rows = rows;
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self, _: &mut ExecutionContext) -> Result<Option<Row>> {
        if self.pos < self.sorted_rows.len() {
            let row = self.sorted_rows[self.pos].clone();
            self.pos += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self, _: &mut ExecutionContext) -> Result<()> {
        self.sorted_rows.clear();
        Ok(())
    }
}

// ─── Limit ────────────────────────────────────────────────────────────────────

pub struct Limit {
    input: Box<dyn PhysicalOperator>,
    limit_expr: Expr,
    offset_expr: Option<Expr>,
    schema: Schema,
    count: usize,
    limit: usize,
    skipped: usize,
    offset: usize,
}

impl Limit {
    pub fn new(input: Box<dyn PhysicalOperator>, limit: Expr, offset: Option<Expr>) -> Self {
        let schema = input.schema().clone();
        Limit {
            input,
            limit_expr: limit,
            offset_expr: offset,
            schema,
            count: 0,
            limit: 0,
            skipped: 0,
            offset: 0,
        }
    }
}

impl PhysicalOperator for Limit {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn open(&mut self, ctx: &mut ExecutionContext) -> Result<()> {
        let empty_row: Vec<Value> = vec![];
        let empty_schema = Schema::default();
        let lv = eval_expr(&self.limit_expr, &empty_row, &empty_schema)?;
        self.limit = lv.coerce_to_int()? as usize;
        self.offset = if let Some(oe) = &self.offset_expr {
            eval_expr(oe, &empty_row, &empty_schema)?.coerce_to_int()? as usize
        } else {
            0
        };
        self.count = 0;
        self.skipped = 0;
        self.input.open(ctx)
    }

    fn next(&mut self, ctx: &mut ExecutionContext) -> Result<Option<Row>> {
        if self.count >= self.limit {
            return Ok(None);
        }
        loop {
            match self.input.next(ctx)? {
                None => return Ok(None),
                Some(row) => {
                    if self.skipped < self.offset {
                        self.skipped += 1;
                        continue;
                    }
                    self.count += 1;
                    return Ok(Some(row));
                }
            }
        }
    }

    fn close(&mut self, ctx: &mut ExecutionContext) -> Result<()> {
        self.input.close(ctx)
    }
}

// ─── DDL operators (not iterator-based) ────────────────────────────────────────

pub fn do_create_table(stmt: &CreateTableStmt, ctx: &mut ExecutionContext) -> Result<()> {
    use crate::catalog::TableMeta;
    use crate::parser::ast::TableConstraint;
    use crate::types::Column;

    if ctx.catalog.get_table(&stmt.name).is_ok() {
        if stmt.if_not_exists {
            return Ok(());
        }
        return Err(MiniDbError::AlreadyExists(format!(
            "table '{}' already exists",
            stmt.name
        )));
    }

    // Build columns from column defs
    let mut columns = Vec::new();
    for col_def in &stmt.columns {
        let mut col = Column::new(col_def.name.clone(), col_def.data_type.clone());
        col.nullable = !col_def.not_null && !col_def.primary_key;
        col.primary_key = col_def.primary_key;
        col.unique = col_def.unique || col_def.primary_key;
        if let Some(default_expr) = &col_def.default {
            let empty: Vec<Value> = vec![];
            let v = eval_expr(default_expr, &empty, &Schema::default())?;
            col.default = Some(v);
        }
        columns.push(col);
    }

    // Apply table-level constraints
    for constraint in &stmt.constraints {
        match constraint {
            TableConstraint::PrimaryKey(cols) => {
                for col_name in cols {
                    if let Some(col) = columns
                        .iter_mut()
                        .find(|c| c.name.eq_ignore_ascii_case(col_name))
                    {
                        col.primary_key = true;
                        col.unique = true;
                        col.nullable = false;
                    }
                }
            }
            TableConstraint::Unique(cols) => {
                for col_name in cols {
                    if let Some(col) = columns
                        .iter_mut()
                        .find(|c| c.name.eq_ignore_ascii_case(col_name))
                    {
                        col.unique = true;
                    }
                }
            }
            TableConstraint::Check(_) => {
                // Stored as serializable expr in check_constraints
            }
        }
    }

    let schema = Schema::new(columns);

    // Allocate heap
    let heap = Heap::create(&mut ctx.pager)?;

    let table_meta = TableMeta {
        name: stmt.name.clone(),
        schema,
        check_constraints: Vec::new(),
        heap_root: heap.root_page.as_u64(),
        next_row_id: 1,
    };

    let tid = ctx.txn.current_txn_id();
    let cat_bytes = ctx.catalog.serialize()?;

    ctx.catalog.create_table(table_meta)?;

    let new_cat_bytes = ctx.catalog.serialize()?;
    ctx.txn.log_record(WalRecord::CatalogChange {
        txn_id: tid,
        payload: new_cat_bytes,
    })?;

    ctx.pager.flush_all()?;
    Ok(())
}

pub fn do_drop_table(stmt: &DropTableStmt, ctx: &mut ExecutionContext) -> Result<()> {
    if ctx.catalog.get_table(&stmt.name).is_err() {
        if stmt.if_exists {
            return Ok(());
        }
        return Err(MiniDbError::NotFound(format!(
            "table '{}' not found",
            stmt.name
        )));
    }

    ctx.catalog.drop_table(&stmt.name)?;
    let tid = ctx.txn.current_txn_id();
    let new_cat_bytes = ctx.catalog.serialize()?;
    ctx.txn.log_record(WalRecord::CatalogChange {
        txn_id: tid,
        payload: new_cat_bytes,
    })?;
    Ok(())
}

pub fn do_create_index(stmt: &CreateIndexStmt, ctx: &mut ExecutionContext) -> Result<()> {
    use crate::catalog::IndexMeta;

    if ctx.catalog.get_index(&stmt.name).is_ok() {
        if stmt.if_not_exists {
            return Ok(());
        }
        return Err(MiniDbError::AlreadyExists(format!(
            "index '{}' already exists",
            stmt.name
        )));
    }

    // Allocate B-tree root
    let tree = BTree::create(&mut ctx.pager)?;

    let idx_meta = IndexMeta {
        name: stmt.name.clone(),
        table: stmt.table.clone(),
        columns: stmt.columns.clone(),
        unique: stmt.unique,
        btree_root: tree.root.as_u64(),
    };

    ctx.catalog.create_index(idx_meta)?;

    // Build the index from existing table rows
    let table_meta = ctx.catalog.get_table(&stmt.table)?.clone();
    let heap_root = PageId(table_meta.heap_root);
    let schema = table_meta.schema.clone();
    let col_idx = schema
        .find_column(&stmt.columns[0])
        .ok_or_else(|| MiniDbError::Schema(format!("column '{}' not found", stmt.columns[0])))?;

    let heap = Heap::open(heap_root);
    let idx_root = PageId(ctx.catalog.get_index(&stmt.name)?.btree_root);
    let mut btree = BTree::open(idx_root);

    let mut entries = Vec::new();
    heap.scan(&mut ctx.pager, |_pid, _slot, data| {
        if data.len() < 8 {
            return Ok(true);
        }
        let row_id = u64::from_le_bytes(data[..8].try_into().unwrap());
        let row = schema.deserialize_row(&data[8..])?;
        let key_val = &row[col_idx];
        entries.push(BTreeKey::new(key_val, row_id));
        Ok(true)
    })?;

    for key in entries {
        btree.insert(&mut ctx.pager, key)?;
    }

    // Update catalog with potentially new root (after splits)
    if let Some(idx) = ctx.catalog.indexes.get_mut(&stmt.name.to_lowercase()) {
        idx.btree_root = btree.root.as_u64();
    }

    let tid = ctx.txn.current_txn_id();
    let new_cat_bytes = ctx.catalog.serialize()?;
    ctx.txn.log_record(WalRecord::CatalogChange {
        txn_id: tid,
        payload: new_cat_bytes,
    })?;
    ctx.pager.flush_all()?;
    Ok(())
}

pub fn do_drop_index(stmt: &DropIndexStmt, ctx: &mut ExecutionContext) -> Result<()> {
    if ctx.catalog.get_index(&stmt.name).is_err() {
        if stmt.if_exists {
            return Ok(());
        }
        return Err(MiniDbError::NotFound(format!(
            "index '{}' not found",
            stmt.name
        )));
    }

    ctx.catalog.drop_index(&stmt.name)?;
    let tid = ctx.txn.current_txn_id();
    let new_cat_bytes = ctx.catalog.serialize()?;
    ctx.txn.log_record(WalRecord::CatalogChange {
        txn_id: tid,
        payload: new_cat_bytes,
    })?;
    Ok(())
}

pub fn do_insert(
    table: &str,
    columns: Option<&[String]>,
    values: &[Vec<Expr>],
    ctx: &mut ExecutionContext,
) -> Result<usize> {
    let table_meta = ctx.catalog.get_table(table)?.clone();
    let schema = table_meta.schema.clone();
    let heap_root = PageId(table_meta.heap_root);
    let heap = Heap::open(heap_root);

    // Gather index names for this table
    let idx_names = ctx.catalog.index_names_for_table(table);

    let mut count = 0;
    for row_values in values {
        // Build full row
        let full_row = build_full_row(&schema, columns, row_values)?;

        // Validate NOT NULL and constraints
        for (i, col) in schema.columns.iter().enumerate() {
            if !col.nullable && full_row[i].is_null() {
                return Err(MiniDbError::Constraint(format!(
                    "NOT NULL constraint violated for column '{}'",
                    col.name
                )));
            }
        }

        // Assign row_id
        let row_id = {
            let tm = ctx.catalog.get_table_mut(table)?;
            let rid = tm.next_row_id;
            tm.next_row_id += 1;
            rid
        };

        // Serialize: [row_id: u64][column data...]
        let mut row_data = row_id.to_le_bytes().to_vec();
        let col_data = schema.serialize_row(&full_row)?;
        row_data.extend_from_slice(&col_data);

        let (_pid, _slot) = heap.insert_row(&mut ctx.pager, &row_data)?;

        // Update indexes
        for idx_name in &idx_names {
            if let Ok(idx_meta) = ctx.catalog.get_index(idx_name) {
                let col_name = idx_meta.columns[0].clone();
                let btree_root = PageId(idx_meta.btree_root);
                if let Some(col_idx) = schema.find_column(&col_name) {
                    let key_val = &full_row[col_idx];
                    let key = BTreeKey::new(key_val, row_id);
                    let mut tree = BTree::open(btree_root);
                    tree.insert(&mut ctx.pager, key)?;
                    // Update root if changed
                    if let Some(idx) = ctx.catalog.indexes.get_mut(&idx_name.to_lowercase()) {
                        idx.btree_root = tree.root.as_u64();
                    }
                }
            }
        }

        // WAL record
        let tid = ctx.txn.current_txn_id();
        ctx.txn.log_record(WalRecord::InsertRow {
            txn_id: tid,
            table: table.to_string(),
            row_id,
            data: row_data,
        })?;

        count += 1;
    }

    Ok(count)
}

fn build_full_row(schema: &Schema, columns: Option<&[String]>, values: &[Expr]) -> Result<Row> {
    let empty_row: Vec<Value> = vec![];
    let empty_schema = Schema::default();

    match columns {
        None => {
            // Positional: must match column count
            if values.len() != schema.columns.len() {
                return Err(MiniDbError::Schema(format!(
                    "expected {} values, got {}",
                    schema.columns.len(),
                    values.len()
                )));
            }
            values
                .iter()
                .map(|e| eval_expr(e, &empty_row, &empty_schema))
                .collect()
        }
        Some(cols) => {
            // Named: fill in defaults for missing columns
            let mut row = Vec::with_capacity(schema.columns.len());
            for col in &schema.columns {
                if let Some(pos) = cols.iter().position(|c| c.eq_ignore_ascii_case(&col.name)) {
                    let v = eval_expr(&values[pos], &empty_row, &empty_schema)?;
                    row.push(v);
                } else if let Some(default) = &col.default {
                    row.push(default.clone());
                } else {
                    row.push(Value::Null);
                }
            }
            Ok(row)
        }
    }
}

pub fn do_update(
    table: &str,
    scan_plan: &PhysicalPlan,
    assignments: &[(String, Expr)],
    ctx: &mut ExecutionContext,
) -> Result<usize> {
    // Collect rows to update
    let table_meta = ctx.catalog.get_table(table)?.clone();
    let schema = table_meta.schema.clone();
    let heap_root = PageId(table_meta.heap_root);
    let heap = Heap::open(heap_root);
    let idx_names = ctx.catalog.index_names_for_table(table);

    // Scan to find rows
    let mut to_update: Vec<(PageId, u16, u64, Row)> = Vec::new();
    let predicate = extract_predicate(scan_plan);

    heap.scan(&mut ctx.pager, |pid, slot, data| {
        if data.len() < 8 {
            return Ok(true);
        }
        let row_id = u64::from_le_bytes(data[..8].try_into().unwrap());
        let row = schema.deserialize_row(&data[8..])?;
        if let Some(pred) = &predicate {
            if eval_predicate(pred, &row, &schema)? != Some(true) {
                return Ok(true);
            }
        }
        to_update.push((pid, slot, row_id, row));
        Ok(true)
    })?;

    let count = to_update.len();
    let empty_row: Vec<Value> = vec![];
    let empty_schema = Schema::default();

    for (pid, slot, row_id, old_row) in &to_update {
        let mut new_row = old_row.clone();

        for (col_name, expr) in assignments {
            if let Some(idx) = schema.find_column(col_name) {
                new_row[idx] = eval_expr(expr, old_row, &schema)?;
            } else {
                return Err(MiniDbError::Schema(format!(
                    "column '{}' not found in table '{}'",
                    col_name, table
                )));
            }
        }

        // NOT NULL checks
        for (i, col) in schema.columns.iter().enumerate() {
            if !col.nullable && new_row[i].is_null() {
                return Err(MiniDbError::Constraint(format!(
                    "NOT NULL constraint violated for column '{}'",
                    col.name
                )));
            }
        }

        let old_serialized = {
            let mut d = row_id.to_le_bytes().to_vec();
            d.extend(schema.serialize_row(old_row)?);
            d
        };
        let new_serialized = {
            let mut d = row_id.to_le_bytes().to_vec();
            d.extend(schema.serialize_row(&new_row)?);
            d
        };

        // Update the heap
        let new_loc = heap.update_row(&mut ctx.pager, *pid, *slot, &new_serialized)?;

        // Update indexes
        for idx_name in &idx_names {
            if let Ok(idx_meta) = ctx.catalog.get_index(idx_name) {
                let col_name_idx = idx_meta.columns[0].clone();
                let btree_root = PageId(idx_meta.btree_root);
                if let Some(col_idx) = schema.find_column(&col_name_idx) {
                    let old_key = BTreeKey::new(&old_row[col_idx], *row_id);
                    let new_key = BTreeKey::new(&new_row[col_idx], *row_id);
                    let mut tree = BTree::open(btree_root);
                    tree.delete(&mut ctx.pager, &old_key)?;
                    tree.insert(&mut ctx.pager, new_key)?;
                    if let Some(idx) = ctx.catalog.indexes.get_mut(&idx_name.to_lowercase()) {
                        idx.btree_root = tree.root.as_u64();
                    }
                }
            }
        }

        // WAL
        let tid = ctx.txn.current_txn_id();
        ctx.txn.log_record(WalRecord::UpdateRow {
            txn_id: tid,
            table: table.to_string(),
            row_id: *row_id,
            old_data: old_serialized,
            new_data: new_serialized,
        })?;
    }

    Ok(count)
}

pub fn do_delete(
    table: &str,
    scan_plan: &PhysicalPlan,
    ctx: &mut ExecutionContext,
) -> Result<usize> {
    let table_meta = ctx.catalog.get_table(table)?.clone();
    let schema = table_meta.schema.clone();
    let heap_root = PageId(table_meta.heap_root);
    let heap = Heap::open(heap_root);
    let idx_names = ctx.catalog.index_names_for_table(table);
    let predicate = extract_predicate(scan_plan);

    let mut to_delete: Vec<(PageId, u16, u64, Row)> = Vec::new();

    heap.scan(&mut ctx.pager, |pid, slot, data| {
        if data.len() < 8 {
            return Ok(true);
        }
        let row_id = u64::from_le_bytes(data[..8].try_into().unwrap());
        let row = schema.deserialize_row(&data[8..])?;
        if let Some(pred) = &predicate {
            if eval_predicate(pred, &row, &schema)? != Some(true) {
                return Ok(true);
            }
        }
        to_delete.push((pid, slot, row_id, row));
        Ok(true)
    })?;

    let count = to_delete.len();

    for (pid, slot, row_id, row) in &to_delete {
        let old_data = {
            let mut d = row_id.to_le_bytes().to_vec();
            d.extend(schema.serialize_row(row)?);
            d
        };

        heap.delete_row(&mut ctx.pager, *pid, *slot)?;

        // Update indexes
        for idx_name in &idx_names {
            if let Ok(idx_meta) = ctx.catalog.get_index(idx_name) {
                let col_name_idx = idx_meta.columns[0].clone();
                let btree_root = PageId(idx_meta.btree_root);
                if let Some(col_idx) = schema.find_column(&col_name_idx) {
                    let key = BTreeKey::new(&row[col_idx], *row_id);
                    let mut tree = BTree::open(btree_root);
                    tree.delete(&mut ctx.pager, &key)?;
                    if let Some(idx) = ctx.catalog.indexes.get_mut(&idx_name.to_lowercase()) {
                        idx.btree_root = tree.root.as_u64();
                    }
                }
            }
        }

        let tid = ctx.txn.current_txn_id();
        ctx.txn.log_record(WalRecord::DeleteRow {
            txn_id: tid,
            table: table.to_string(),
            row_id: *row_id,
            old_data,
        })?;
    }

    Ok(count)
}

fn extract_predicate(plan: &PhysicalPlan) -> Option<Expr> {
    match plan {
        PhysicalPlan::SeqScan { predicate, .. } => predicate.clone(),
        PhysicalPlan::IndexScan { predicate, .. } => predicate.clone(),
        PhysicalPlan::Filter { predicate, .. } => Some(predicate.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Catalog, TableMeta};
    use crate::storage::{Pager, Wal};
    use crate::txn::TransactionManager;
    use crate::types::{Column, DataType, Schema};
    use tempfile::TempDir;

    fn make_ctx(dir: &TempDir) -> ExecutionContext {
        let pager = Pager::open(dir.path().join("data.db")).unwrap();
        let wal = Wal::open(dir.path().join("data.wal")).unwrap();
        let catalog = Catalog::new();
        let txn = TransactionManager::new(wal);
        ExecutionContext::new(pager, catalog, txn)
    }

    #[test]
    fn test_create_and_insert_select() {
        let dir = TempDir::new().unwrap();
        let mut ctx = make_ctx(&dir);
        ctx.txn.begin().unwrap();

        let create_stmt =
            crate::parser::Parser::parse_from_str("CREATE TABLE users (id INT, name TEXT);")
                .unwrap();

        if let crate::parser::ast::Statement::CreateTable(stmt) = &create_stmt[0] {
            do_create_table(stmt, &mut ctx).unwrap();
        }

        let insert_stmt =
            crate::parser::Parser::parse_from_str("INSERT INTO users VALUES (1, 'Alice');")
                .unwrap();
        if let crate::parser::ast::Statement::Insert(s) = &insert_stmt[0] {
            let count = do_insert(&s.table, s.columns.as_deref(), &s.values, &mut ctx).unwrap();
            assert_eq!(count, 1);
        }

        ctx.txn.commit().unwrap();
        ctx.pager.flush_all().unwrap();

        // Scan via seq scan
        ctx.txn.begin().unwrap();
        let mut scan = SeqScan::new("users".into(), None, None, &ctx).unwrap();
        scan.open(&mut ctx).unwrap();
        let row = scan.next(&mut ctx).unwrap();
        assert!(row.is_some());
        let row = row.unwrap();
        assert_eq!(row[0], Value::Int(1));
        assert_eq!(row[1], Value::Text("Alice".into()));
    }
}
