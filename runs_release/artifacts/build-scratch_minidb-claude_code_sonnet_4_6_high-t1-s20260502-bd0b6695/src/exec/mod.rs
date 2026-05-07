use crate::catalog::{Catalog, IndexDef, TableSchema};
use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;
use crate::plan::physical::{PhysicalPlan, ScanOp};
use crate::storage::btree::BTreeIndex;
use crate::storage::heap::{HeapFile, RowId};
use crate::storage::page::{PageId, PageManager};
use crate::storage::wal::{WalRecord, WalRecordBody};
use crate::txn::{TxnManager, TxnOp};
use crate::types::{DataType, Row, Schema, Value};
use std::collections::HashMap;
use std::path::Path;

/// Output of executing a SQL statement.
#[derive(Debug)]
pub struct QueryResult {
    pub schema: Schema,
    pub rows: Vec<Row>,
    pub affected_rows: Option<usize>,
    pub message: Option<String>,
}

impl QueryResult {
    pub fn empty() -> Self {
        QueryResult {
            schema: Schema::empty(),
            rows: vec![],
            affected_rows: None,
            message: None,
        }
    }

    pub fn with_message(msg: &str) -> Self {
        QueryResult {
            schema: Schema::empty(),
            rows: vec![],
            affected_rows: None,
            message: Some(msg.to_string()),
        }
    }

    pub fn affected(n: usize) -> Self {
        QueryResult {
            schema: Schema::empty(),
            rows: vec![],
            affected_rows: Some(n),
            message: None,
        }
    }
}

// ======== PhysicalOperator trait ========

pub trait PhysicalOperator {
    /// Execute the operator (buffering all results internally).
    fn open(&mut self, db: &mut Database) -> Result<()>;
    /// Return next row from internal buffer, or None when exhausted.
    fn next(&mut self) -> Result<Option<Row>>;
    /// Reset and release internal state.
    fn close(&mut self);
    /// Schema of the output rows.
    fn schema(&self) -> Schema;
}

// ======== Operator implementations ========

// -- SeqScan --
pub struct SeqScan {
    table: String,
    alias: Option<String>,
    buffer: Vec<Row>,
    pos: usize,
    out_schema: Schema,
}

impl SeqScan {
    pub fn new(table: String, alias: Option<String>) -> Self {
        SeqScan {
            table,
            alias,
            buffer: vec![],
            pos: 0,
            out_schema: Schema::empty(),
        }
    }
}

impl PhysicalOperator for SeqScan {
    fn open(&mut self, db: &mut Database) -> Result<()> {
        let schema = db
            .catalog
            .get_table(&self.table)
            .ok_or_else(|| MiniDbError::NoSuchTable(self.table.clone()))?;
        let types = schema.col_types();
        let col_names: Vec<(String, DataType)> = schema
            .columns
            .iter()
            .map(|c| {
                let prefix = self.alias.as_deref().unwrap_or(&self.table);
                (format!("{}.{}", prefix, c.name), c.data_type.clone())
            })
            .collect();
        let col_names_plain: Vec<(String, DataType)> = schema
            .columns
            .iter()
            .map(|c| (c.name.clone(), c.data_type.clone()))
            .collect();

        let heap = db
            .heaps
            .get(&self.table.to_lowercase())
            .ok_or_else(|| MiniDbError::Internal(format!("no heap for {}", self.table)))?;
        let heap = heap.clone_ref();
        let rows = heap.scan(&mut db.page_manager, &types)?;
        self.buffer = rows.into_iter().map(|(_, row)| row).collect();
        self.out_schema = Schema::new(col_names_plain);
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos < self.buffer.len() {
            let row = self.buffer[self.pos].clone();
            self.pos += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) {
        self.buffer.clear();
        self.pos = 0;
    }

    fn schema(&self) -> Schema {
        self.out_schema.clone()
    }
}

// We need a way to hold a reference-like HeapFile from the Database.
// Use a thin wrapper.
#[derive(Clone)]
pub struct HeapRef {
    first_page: PageId,
    last_page: PageId,
}

impl HeapRef {
    pub fn clone_ref(&self) -> HeapFile {
        HeapFile {
            first_page: self.first_page,
            last_page: self.last_page,
        }
    }
}

impl From<&HeapFile> for HeapRef {
    fn from(h: &HeapFile) -> Self {
        HeapRef {
            first_page: h.first_page,
            last_page: h.last_page,
        }
    }
}

// -- IndexScan --
pub struct IndexScan {
    table: String,
    index_name: String,
    column: String,
    op: ScanOp,
    value: Value,
    alias: Option<String>,
    buffer: Vec<Row>,
    pos: usize,
    out_schema: Schema,
}

impl IndexScan {
    pub fn new(
        table: String,
        index_name: String,
        column: String,
        op: ScanOp,
        value: Value,
        alias: Option<String>,
    ) -> Self {
        IndexScan {
            table,
            index_name,
            column,
            op,
            value,
            alias,
            buffer: vec![],
            pos: 0,
            out_schema: Schema::empty(),
        }
    }
}

impl PhysicalOperator for IndexScan {
    fn open(&mut self, db: &mut Database) -> Result<()> {
        let schema = db
            .catalog
            .get_table(&self.table)
            .ok_or_else(|| MiniDbError::NoSuchTable(self.table.clone()))?;
        let types = schema.col_types();
        let col_names: Vec<(String, DataType)> = schema
            .columns
            .iter()
            .map(|c| (c.name.clone(), c.data_type.clone()))
            .collect();

        let idx_def = db
            .catalog
            .get_index(&self.index_name)
            .ok_or_else(|| MiniDbError::NoSuchIndex(self.index_name.clone()))?;
        let root = PageId(idx_def.root_page);

        let btree = BTreeIndex { root };
        let row_ids = match &self.op {
            ScanOp::Eq => btree.eq_scan(&mut db.page_manager, &self.value)?,
            ScanOp::Lt => btree.range_scan(&mut db.page_manager, None, Some(&self.value))?,
            ScanOp::Le => btree.range_scan(&mut db.page_manager, None, Some(&self.value))?,
            ScanOp::Gt => btree.range_scan(&mut db.page_manager, Some(&self.value), None)?,
            ScanOp::Ge => btree.range_scan(&mut db.page_manager, Some(&self.value), None)?,
        };

        let heap = db
            .heaps
            .get(&self.table.to_lowercase())
            .ok_or_else(|| MiniDbError::Internal(format!("no heap for {}", self.table)))?
            .clone_ref();

        let mut rows = Vec::new();
        for rid in row_ids {
            if let Some(row) = heap.get(&mut db.page_manager, rid, &types)? {
                rows.push(row);
            }
        }

        // For Lt/Gt, we need to re-filter (index scan returns range including boundary)
        // The Filter operator above will handle strict inequality, but for IndexScan
        // alone we need to do it
        self.buffer = rows;
        self.out_schema = Schema::new(col_names);
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos < self.buffer.len() {
            let row = self.buffer[self.pos].clone();
            self.pos += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) {
        self.buffer.clear();
        self.pos = 0;
    }

    fn schema(&self) -> Schema {
        self.out_schema.clone()
    }
}

// -- Filter --
pub struct Filter {
    child: Box<dyn PhysicalOperator>,
    predicate: Expr,
    buffer: Vec<Row>,
    pos: usize,
}

impl Filter {
    pub fn new(child: Box<dyn PhysicalOperator>, predicate: Expr) -> Self {
        Filter {
            child,
            predicate,
            buffer: vec![],
            pos: 0,
        }
    }
}

impl PhysicalOperator for Filter {
    fn open(&mut self, db: &mut Database) -> Result<()> {
        self.child.open(db)?;
        let schema = self.child.schema();
        while let Some(row) = self.child.next()? {
            if eval_predicate(&self.predicate, &row, &schema)? {
                self.buffer.push(row);
            }
        }
        self.child.close();
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos < self.buffer.len() {
            let row = self.buffer[self.pos].clone();
            self.pos += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) {
        self.buffer.clear();
        self.pos = 0;
    }

    fn schema(&self) -> Schema {
        self.child.schema()
    }
}

// -- Project --
pub struct Project {
    child: Box<dyn PhysicalOperator>,
    projections: Vec<SelectItem>,
    buffer: Vec<Row>,
    pos: usize,
    out_schema: Schema,
}

impl Project {
    pub fn new(child: Box<dyn PhysicalOperator>, projections: Vec<SelectItem>) -> Self {
        Project {
            child,
            projections,
            buffer: vec![],
            pos: 0,
            out_schema: Schema::empty(),
        }
    }
}

impl PhysicalOperator for Project {
    fn open(&mut self, db: &mut Database) -> Result<()> {
        self.child.open(db)?;
        let in_schema = self.child.schema();

        // Build output schema
        let mut out_cols: Vec<(String, DataType)> = Vec::new();
        for item in &self.projections {
            match item {
                SelectItem::Star => {
                    out_cols.extend(in_schema.columns.clone());
                }
                SelectItem::TableStar(tbl) => {
                    for (name, dt) in &in_schema.columns {
                        let prefix = format!("{}.", tbl.to_lowercase());
                        if name.to_lowercase().starts_with(&prefix)
                            || name.to_lowercase() == tbl.to_lowercase()
                        {
                            let short = name.splitn(2, '.').last().unwrap_or(name).to_string();
                            out_cols.push((short, dt.clone()));
                        }
                    }
                }
                SelectItem::Expr(expr, alias) => {
                    let col_name = alias.clone().unwrap_or_else(|| expr_name(expr));
                    // Determine type from expr (best-effort)
                    let dt = infer_expr_type(expr, &in_schema);
                    out_cols.push((col_name, dt));
                }
            }
        }
        self.out_schema = Schema::new(out_cols);

        while let Some(row) = self.child.next()? {
            let projected = project_row(&self.projections, &row, &in_schema)?;
            self.buffer.push(projected);
        }
        self.child.close();
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos < self.buffer.len() {
            let row = self.buffer[self.pos].clone();
            self.pos += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) {
        self.buffer.clear();
        self.pos = 0;
    }

    fn schema(&self) -> Schema {
        self.out_schema.clone()
    }
}

// -- NestedLoopJoin --
pub struct NestedLoopJoin {
    left: Box<dyn PhysicalOperator>,
    right: Box<dyn PhysicalOperator>,
    condition: Option<Expr>,
    buffer: Vec<Row>,
    pos: usize,
    out_schema: Schema,
}

impl NestedLoopJoin {
    pub fn new(
        left: Box<dyn PhysicalOperator>,
        right: Box<dyn PhysicalOperator>,
        condition: Option<Expr>,
    ) -> Self {
        NestedLoopJoin {
            left,
            right,
            condition,
            buffer: vec![],
            pos: 0,
            out_schema: Schema::empty(),
        }
    }
}

impl PhysicalOperator for NestedLoopJoin {
    fn open(&mut self, db: &mut Database) -> Result<()> {
        self.left.open(db)?;
        self.right.open(db)?;

        let left_schema = self.left.schema();
        let right_schema = self.right.schema();
        self.out_schema = left_schema.append(&right_schema, None);

        // Collect left
        let mut left_rows = Vec::new();
        while let Some(r) = self.left.next()? {
            left_rows.push(r);
        }
        // Collect right
        let mut right_rows = Vec::new();
        while let Some(r) = self.right.next()? {
            right_rows.push(r);
        }

        // Cross product with optional condition
        for l in &left_rows {
            for r in &right_rows {
                let combined: Row = l.iter().chain(r.iter()).cloned().collect();
                if let Some(cond) = &self.condition {
                    if eval_predicate(cond, &combined, &self.out_schema)? {
                        self.buffer.push(combined);
                    }
                } else {
                    self.buffer.push(combined);
                }
            }
        }

        self.left.close();
        self.right.close();
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos < self.buffer.len() {
            let row = self.buffer[self.pos].clone();
            self.pos += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) {
        self.buffer.clear();
        self.pos = 0;
    }

    fn schema(&self) -> Schema {
        self.out_schema.clone()
    }
}

// -- Sort --
pub struct Sort {
    child: Box<dyn PhysicalOperator>,
    order_by: Vec<OrderByItem>,
    buffer: Vec<Row>,
    pos: usize,
}

impl Sort {
    pub fn new(child: Box<dyn PhysicalOperator>, order_by: Vec<OrderByItem>) -> Self {
        Sort {
            child,
            order_by,
            buffer: vec![],
            pos: 0,
        }
    }
}

impl PhysicalOperator for Sort {
    fn open(&mut self, db: &mut Database) -> Result<()> {
        self.child.open(db)?;
        let schema = self.child.schema();
        while let Some(row) = self.child.next()? {
            self.buffer.push(row);
        }
        self.child.close();

        // Sort buffer
        let order_by = self.order_by.clone();
        let schema_clone = schema.clone();
        let mut sort_err: Option<MiniDbError> = None;
        self.buffer.sort_by(|a, b| {
            if sort_err.is_some() {
                return std::cmp::Ordering::Equal;
            }
            for ob in &order_by {
                let va = match eval_expr(&ob.expr, a, &schema_clone) {
                    Ok(v) => v,
                    Err(e) => {
                        sort_err = Some(e);
                        return std::cmp::Ordering::Equal;
                    }
                };
                let vb = match eval_expr(&ob.expr, b, &schema_clone) {
                    Ok(v) => v,
                    Err(e) => {
                        sort_err = Some(e);
                        return std::cmp::Ordering::Equal;
                    }
                };
                let cmp = va.sql_compare(&vb).unwrap_or(std::cmp::Ordering::Equal);
                let cmp = if ob.asc { cmp } else { cmp.reverse() };
                if cmp != std::cmp::Ordering::Equal {
                    return cmp;
                }
            }
            std::cmp::Ordering::Equal
        });

        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos < self.buffer.len() {
            let row = self.buffer[self.pos].clone();
            self.pos += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) {
        self.buffer.clear();
        self.pos = 0;
    }

    fn schema(&self) -> Schema {
        self.child.schema()
    }
}

// -- Limit --
pub struct Limit {
    child: Box<dyn PhysicalOperator>,
    limit: usize,
    offset: usize,
    buffer: Vec<Row>,
    pos: usize,
}

impl Limit {
    pub fn new(child: Box<dyn PhysicalOperator>, limit: usize, offset: usize) -> Self {
        Limit {
            child,
            limit,
            offset,
            buffer: vec![],
            pos: 0,
        }
    }
}

impl PhysicalOperator for Limit {
    fn open(&mut self, db: &mut Database) -> Result<()> {
        self.child.open(db)?;
        let mut count = 0usize;
        let mut skipped = 0usize;
        while let Some(row) = self.child.next()? {
            if skipped < self.offset {
                skipped += 1;
                continue;
            }
            if count < self.limit {
                self.buffer.push(row);
                count += 1;
            } else {
                break;
            }
        }
        self.child.close();
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos < self.buffer.len() {
            let row = self.buffer[self.pos].clone();
            self.pos += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) {
        self.buffer.clear();
        self.pos = 0;
    }

    fn schema(&self) -> Schema {
        self.child.schema()
    }
}

// -- Insert --
pub struct Insert {
    table: String,
    columns: Option<Vec<String>>,
    values: Vec<Vec<Expr>>,
    affected: usize,
    done: bool,
}

impl Insert {
    pub fn new(table: String, columns: Option<Vec<String>>, values: Vec<Vec<Expr>>) -> Self {
        Insert {
            table,
            columns,
            values,
            affected: 0,
            done: false,
        }
    }
}

impl PhysicalOperator for Insert {
    fn open(&mut self, db: &mut Database) -> Result<()> {
        let schema = db
            .catalog
            .get_table(&self.table)
            .ok_or_else(|| MiniDbError::NoSuchTable(self.table.clone()))?
            .clone();

        for value_exprs in &self.values {
            let row = build_insert_row(&schema, &self.columns, value_exprs, db)?;

            // Check constraints
            check_row_constraints(&schema, &row, db, None)?;

            // Insert into heap
            let heap = db
                .heaps
                .get_mut(&self.table.to_lowercase())
                .ok_or_else(|| MiniDbError::Internal(format!("no heap for {}", self.table)))?;
            let row_id = {
                let mut heap_file = heap.clone_ref();
                let rid = heap_file.insert(&mut db.page_manager, &row)?;
                if let Some(hr) = db.heaps.get_mut(&self.table.to_lowercase()) {
                    hr.last_page = heap_file.last_page;
                }
                rid
            };

            // Update indexes
            let idx_names: Vec<(String, String)> = db
                .catalog
                .indexes_for_table(&self.table)
                .iter()
                .map(|i| (i.name.clone(), i.column.clone()))
                .collect();
            for (idx_name, col_name) in &idx_names {
                let col_idx = schema
                    .col_index(col_name)
                    .ok_or_else(|| MiniDbError::NoSuchColumn(col_name.clone()))?;
                let key_val = &row[col_idx];
                let idx_def = db
                    .catalog
                    .get_index(idx_name)
                    .ok_or_else(|| MiniDbError::NoSuchIndex(idx_name.clone()))?;

                if idx_def.unique || schema.columns[col_idx].primary_key {
                    let existing = {
                        let mut btree = BTreeIndex {
                            root: PageId(idx_def.root_page),
                        };
                        btree.eq_scan(&mut db.page_manager, key_val)?
                    };
                    if !existing.is_empty() {
                        return Err(MiniDbError::DuplicateKey(format!(
                            "unique constraint violation on {}.{}",
                            self.table, col_name
                        )));
                    }
                }

                let idx_def = db
                    .catalog
                    .get_index_mut(idx_name)
                    .ok_or_else(|| MiniDbError::NoSuchIndex(idx_name.clone()))?;
                let mut btree = BTreeIndex {
                    root: PageId(idx_def.root_page),
                };
                btree.insert(&mut db.page_manager, key_val, row_id)?;
                idx_def.root_page = btree.root.0;
            }

            // WAL log
            let txn_id = db.txn_manager.begin_implicit();
            db.txn_manager.log_op(TxnOp::Insert {
                table: self.table.clone(),
                row_id,
                row: row.clone(),
            });
            if !db.explicit_txn {
                db.txn_manager.commit(&mut db.wal)?;
            }

            self.affected += 1;
        }

        // Update catalog heap pages
        if let Some(heap_ref) = db.heaps.get(&self.table.to_lowercase()) {
            let first = heap_ref.first_page.0;
            let last = heap_ref.last_page.0;
            if let Some(ts) = db.catalog.get_table_mut(&self.table) {
                ts.heap_first_page = first;
                ts.heap_last_page = last;
            }
        }

        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if !self.done {
            self.done = true;
            Ok(Some(vec![Value::Int(self.affected as i64)]))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) {}

    fn schema(&self) -> Schema {
        Schema::new(vec![("rows_inserted".into(), DataType::Int)])
    }
}

// -- Update --
pub struct Update {
    table: String,
    assignments: Vec<(String, Expr)>,
    predicate: Option<Expr>,
    affected: usize,
    done: bool,
}

impl Update {
    pub fn new(table: String, assignments: Vec<(String, Expr)>, predicate: Option<Expr>) -> Self {
        Update {
            table,
            assignments,
            predicate,
            affected: 0,
            done: false,
        }
    }
}

impl PhysicalOperator for Update {
    fn open(&mut self, db: &mut Database) -> Result<()> {
        let schema = db
            .catalog
            .get_table(&self.table)
            .ok_or_else(|| MiniDbError::NoSuchTable(self.table.clone()))?
            .clone();
        let types = schema.col_types();
        let row_schema = schema.to_schema();

        let heap = db
            .heaps
            .get(&self.table.to_lowercase())
            .ok_or_else(|| MiniDbError::Internal(format!("no heap for {}", self.table)))?
            .clone_ref();
        let all_rows = heap.scan(&mut db.page_manager, &types)?;

        let mut to_update: Vec<(RowId, Row)> = Vec::new();
        for (rid, row) in &all_rows {
            if let Some(pred) = &self.predicate {
                if !eval_predicate(pred, row, &row_schema)? {
                    continue;
                }
            }
            to_update.push((*rid, row.clone()));
        }

        let idx_names: Vec<(String, String, bool)> = db
            .catalog
            .indexes_for_table(&self.table)
            .iter()
            .map(|i| (i.name.clone(), i.column.clone(), i.unique))
            .collect();

        for (rid, old_row) in to_update {
            let mut new_row = old_row.clone();
            for (col_name, expr) in &self.assignments {
                let col_idx = schema
                    .col_index(col_name)
                    .ok_or_else(|| MiniDbError::NoSuchColumn(col_name.clone()))?;
                let new_val = eval_expr(expr, &old_row, &row_schema)?;
                new_row[col_idx] = new_val;
            }

            // Check constraints
            check_row_constraints(&schema, &new_row, db, Some(rid))?;

            // Update indexes (delete old, insert new)
            for (idx_name, col_name, unique) in &idx_names {
                let col_idx = schema
                    .col_index(col_name)
                    .ok_or_else(|| MiniDbError::NoSuchColumn(col_name.clone()))?;
                let old_val = &old_row[col_idx];
                let new_val = &new_row[col_idx];

                let idx_def = db
                    .catalog
                    .get_index_mut(idx_name)
                    .ok_or_else(|| MiniDbError::NoSuchIndex(idx_name.clone()))?;
                let mut btree = BTreeIndex {
                    root: PageId(idx_def.root_page),
                };
                btree.delete(&mut db.page_manager, old_val, rid)?;
                idx_def.root_page = btree.root.0;

                // Check unique before insert
                if *unique || schema.columns[col_idx].primary_key {
                    let existing = btree.eq_scan(&mut db.page_manager, new_val)?;
                    if !existing.is_empty() {
                        return Err(MiniDbError::DuplicateKey(format!(
                            "unique constraint violation on {}.{}",
                            self.table, col_name
                        )));
                    }
                }

                btree.insert(&mut db.page_manager, new_val, rid)?;
                idx_def.root_page = btree.root.0;
            }

            // Update heap
            let mut heap = db
                .heaps
                .get(&self.table.to_lowercase())
                .ok_or_else(|| MiniDbError::Internal(format!("no heap for {}", self.table)))?
                .clone_ref();
            let new_rid = heap.update(&mut db.page_manager, rid, &new_row)?;
            // Update last_page in heap ref
            if let Some(hr) = db.heaps.get_mut(&self.table.to_lowercase()) {
                hr.last_page = heap.last_page;
            }

            // WAL log
            db.txn_manager.begin_implicit();
            db.txn_manager.log_op(TxnOp::Update {
                table: self.table.clone(),
                row_id: rid,
                new_row_id: new_rid,
                before: old_row,
                after: new_row,
            });
            if !db.explicit_txn {
                db.txn_manager.commit(&mut db.wal)?;
            }

            self.affected += 1;
        }
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if !self.done {
            self.done = true;
            Ok(Some(vec![Value::Int(self.affected as i64)]))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) {}

    fn schema(&self) -> Schema {
        Schema::new(vec![("rows_updated".into(), DataType::Int)])
    }
}

// -- Delete --
pub struct Delete {
    table: String,
    predicate: Option<Expr>,
    affected: usize,
    done: bool,
}

impl Delete {
    pub fn new(table: String, predicate: Option<Expr>) -> Self {
        Delete {
            table,
            predicate,
            affected: 0,
            done: false,
        }
    }
}

impl PhysicalOperator for Delete {
    fn open(&mut self, db: &mut Database) -> Result<()> {
        let schema = db
            .catalog
            .get_table(&self.table)
            .ok_or_else(|| MiniDbError::NoSuchTable(self.table.clone()))?
            .clone();
        let types = schema.col_types();
        let row_schema = schema.to_schema();

        let heap = db
            .heaps
            .get(&self.table.to_lowercase())
            .ok_or_else(|| MiniDbError::Internal(format!("no heap for {}", self.table)))?
            .clone_ref();
        let all_rows = heap.scan(&mut db.page_manager, &types)?;

        let mut to_delete: Vec<(RowId, Row)> = Vec::new();
        for (rid, row) in &all_rows {
            if let Some(pred) = &self.predicate {
                if !eval_predicate(pred, row, &row_schema)? {
                    continue;
                }
            }
            to_delete.push((*rid, row.clone()));
        }

        let idx_names: Vec<(String, String)> = db
            .catalog
            .indexes_for_table(&self.table)
            .iter()
            .map(|i| (i.name.clone(), i.column.clone()))
            .collect();

        for (rid, row) in to_delete {
            // Delete from indexes
            for (idx_name, col_name) in &idx_names {
                let col_idx = schema
                    .col_index(col_name)
                    .ok_or_else(|| MiniDbError::NoSuchColumn(col_name.clone()))?;
                let val = &row[col_idx];
                let idx_def = db
                    .catalog
                    .get_index_mut(idx_name)
                    .ok_or_else(|| MiniDbError::NoSuchIndex(idx_name.clone()))?;
                let mut btree = BTreeIndex {
                    root: PageId(idx_def.root_page),
                };
                btree.delete(&mut db.page_manager, val, rid)?;
                idx_def.root_page = btree.root.0;
            }

            // Delete from heap
            let heap = db
                .heaps
                .get(&self.table.to_lowercase())
                .ok_or_else(|| MiniDbError::Internal(format!("no heap for {}", self.table)))?
                .clone_ref();
            heap.delete(&mut db.page_manager, rid)?;

            // WAL log
            db.txn_manager.begin_implicit();
            db.txn_manager.log_op(TxnOp::Delete {
                table: self.table.clone(),
                row_id: rid,
                row: row.clone(),
            });
            if !db.explicit_txn {
                db.txn_manager.commit(&mut db.wal)?;
            }

            self.affected += 1;
        }
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        if !self.done {
            self.done = true;
            Ok(Some(vec![Value::Int(self.affected as i64)]))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self) {}

    fn schema(&self) -> Schema {
        Schema::new(vec![("rows_deleted".into(), DataType::Int)])
    }
}

// ======== HeapRef insert helper ========

impl HeapRef {
    fn insert_and_update(&mut self, db: &mut Database, table: &str, row: &Row) -> Result<RowId> {
        let mut heap = HeapFile {
            first_page: self.first_page,
            last_page: self.last_page,
        };
        let rid = heap.insert(&mut db.page_manager, row)?;
        // Update the heap ref in db.heaps
        if let Some(hr) = db.heaps.get_mut(&table.to_lowercase()) {
            hr.last_page = heap.last_page;
        }
        Ok(rid)
    }
}

// ======== Database ========

pub struct Database {
    pub catalog: Catalog,
    pub page_manager: PageManager,
    pub heaps: HashMap<String, HeapRef>,
    pub indexes: HashMap<String, BTreeIndex>,
    pub wal: crate::storage::wal::WalWriter,
    pub txn_manager: TxnManager,
    pub explicit_txn: bool,
    pub db_path: std::path::PathBuf,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let catalog_path = path.with_extension("db.catalog");
        let wal_path = path.with_extension("db-wal");
        let page_path = path;

        let mut catalog = Catalog::load_or_create(&catalog_path)?;
        let mut page_manager = PageManager::open(page_path)?;
        let wal = crate::storage::wal::WalWriter::open(&wal_path)?;

        // Rebuild heaps from catalog
        let mut heaps = HashMap::new();
        for (name, ts) in &catalog.tables {
            if ts.heap_first_page < page_manager.num_pages {
                heaps.insert(
                    name.clone(),
                    HeapRef {
                        first_page: PageId(ts.heap_first_page),
                        last_page: PageId(ts.heap_last_page),
                    },
                );
            }
        }

        let mut db = Database {
            catalog,
            page_manager,
            heaps,
            indexes: HashMap::new(),
            wal,
            txn_manager: TxnManager::new(),
            explicit_txn: false,
            db_path: path.to_path_buf(),
        };

        // Replay WAL for crash recovery
        db.replay_wal(&wal_path)?;

        Ok(db)
    }

    fn replay_wal(&mut self, wal_path: &Path) -> Result<()> {
        use crate::storage::wal::{committed_txns, read_raw_records, WalRecordType};
        use crate::types::deserialize_row;

        let records = read_raw_records(wal_path)?;
        if records.is_empty() {
            return Ok(());
        }

        let committed = committed_txns(&records);
        log::info!(
            "WAL replay: {} records, {} committed txns",
            records.len(),
            committed.len()
        );

        for rec in &records {
            if !committed.contains(&rec.txn_id) {
                continue; // skip uncommitted
            }
            match rec.rtype {
                WalRecordType::Insert => {
                    let mut pos = 0usize;
                    let table = crate::storage::wal::decode_string_pos(&rec.data, &mut pos)?;
                    let rid = crate::storage::wal::decode_row_id_pos(&rec.data, &mut pos)?;
                    if let Some(ts) = self.catalog.get_table(&table) {
                        let types = ts.col_types();
                        // Verify row exists (idempotency)
                        if let Some(heap) = self.heaps.get(&table.to_lowercase()) {
                            let h = heap.clone_ref();
                            if h.get(&mut self.page_manager, rid, &types)?.is_none() {
                                // Row missing, re-insert from WAL
                                // Parse the row data from WAL
                                let row = crate::storage::wal::decode_row_pos(
                                    &rec.data, &mut pos, &types,
                                )?;
                                let mut hr = heap.clone();
                                let mut heap_file = hr.clone_ref();
                                heap_file.insert(&mut self.page_manager, &row)?;
                            }
                        }
                    }
                }
                _ => {} // Other record types handled elsewhere
            }
        }
        Ok(())
    }

    pub fn execute_plan(&mut self, plan: PhysicalPlan) -> Result<QueryResult> {
        match plan {
            PhysicalPlan::Begin => {
                self.txn_manager.begin()?;
                self.explicit_txn = true;
                Ok(QueryResult::with_message("transaction started"))
            }
            PhysicalPlan::Commit => {
                self.txn_manager.commit(&mut self.wal)?;
                self.explicit_txn = false;
                self.catalog.save()?;
                Ok(QueryResult::with_message("committed"))
            }
            PhysicalPlan::Rollback => {
                // For rollback, we need to undo changes made during the txn
                // In our simplified model, we just mark as rolled back
                self.txn_manager.rollback(&mut self.wal)?;
                self.explicit_txn = false;
                Ok(QueryResult::with_message("rolled back"))
            }
            PhysicalPlan::CreateTable { stmt } => self.exec_create_table(stmt),
            PhysicalPlan::DropTable { name, if_exists } => self.exec_drop_table(&name, if_exists),
            PhysicalPlan::CreateIndex { stmt } => self.exec_create_index(stmt),
            PhysicalPlan::DropIndex { name, if_exists } => self.exec_drop_index(&name, if_exists),
            PhysicalPlan::MetaCommand(mc) => self.exec_meta(mc),
            plan => {
                // DML: build operator tree and execute
                let mut op = self.build_operator(plan)?;
                op.open(self)?;
                let schema = op.schema();
                let mut rows = Vec::new();
                while let Some(row) = op.next()? {
                    rows.push(row);
                }
                op.close();

                let affected = if schema.columns.len() == 1
                    && (schema.columns[0].0 == "rows_inserted"
                        || schema.columns[0].0 == "rows_updated"
                        || schema.columns[0].0 == "rows_deleted")
                {
                    rows.first().and_then(|r| r.first()).and_then(|v| {
                        if let Value::Int(i) = v {
                            Some(*i as usize)
                        } else {
                            None
                        }
                    })
                } else {
                    None
                };

                if affected.is_some() {
                    if !self.explicit_txn {
                        self.catalog.save()?;
                    }
                    Ok(QueryResult::affected(affected.unwrap()))
                } else {
                    Ok(QueryResult {
                        schema,
                        rows,
                        affected_rows: None,
                        message: None,
                    })
                }
            }
        }
    }

    fn build_operator(&self, plan: PhysicalPlan) -> Result<Box<dyn PhysicalOperator>> {
        match plan {
            PhysicalPlan::SeqScan { table, alias } => Ok(Box::new(SeqScan::new(table, alias))),
            PhysicalPlan::IndexScan {
                table,
                index,
                column,
                op,
                value,
                alias,
            } => Ok(Box::new(IndexScan::new(
                table, index, column, op, value, alias,
            ))),
            PhysicalPlan::Filter { input, predicate } => {
                let child = self.build_operator(*input)?;
                Ok(Box::new(Filter::new(child, predicate)))
            }
            PhysicalPlan::Project { input, projections } => {
                let child = self.build_operator(*input)?;
                Ok(Box::new(Project::new(child, projections)))
            }
            PhysicalPlan::NestedLoopJoin {
                left,
                right,
                condition,
            } => {
                let l = self.build_operator(*left)?;
                let r = self.build_operator(*right)?;
                Ok(Box::new(NestedLoopJoin::new(l, r, condition)))
            }
            PhysicalPlan::Sort { input, order_by } => {
                let child = self.build_operator(*input)?;
                Ok(Box::new(Sort::new(child, order_by)))
            }
            PhysicalPlan::Limit {
                input,
                limit,
                offset,
            } => {
                let child = self.build_operator(*input)?;
                Ok(Box::new(Limit::new(child, limit, offset)))
            }
            PhysicalPlan::Insert {
                table,
                columns,
                values,
            } => Ok(Box::new(Insert::new(table, columns, values))),
            PhysicalPlan::Update {
                table,
                assignments,
                predicate,
            } => Ok(Box::new(Update::new(table, assignments, predicate))),
            PhysicalPlan::Delete { table, predicate } => {
                Ok(Box::new(Delete::new(table, predicate)))
            }
            other => Err(MiniDbError::Internal(format!(
                "unexpected plan in build_operator: {:?}",
                other
            ))),
        }
    }

    fn exec_create_table(
        &mut self,
        stmt: crate::parser::ast::CreateTableStmt,
    ) -> Result<QueryResult> {
        let name = stmt.name.to_lowercase();

        // Check if_not_exists
        if self.catalog.get_table(&name).is_some() {
            if stmt.if_not_exists {
                return Ok(QueryResult::with_message(&format!(
                    "table {} already exists",
                    name
                )));
            }
            return Err(MiniDbError::TableExists(name));
        }

        // Build ColumnDefs
        let mut cols: Vec<crate::types::ColumnDef> = Vec::new();
        let mut primary_key_cols: Vec<String> = Vec::new();
        let mut check_strs: Vec<String> = Vec::new();

        for col_spec in &stmt.columns {
            let mut col = crate::types::ColumnDef::new(&col_spec.name, col_spec.data_type.clone());
            for constraint in &col_spec.constraints {
                match constraint {
                    ColumnConstraint::PrimaryKey => {
                        col.primary_key = true;
                        col.not_null = true;
                        col.unique = true;
                        primary_key_cols.push(col_spec.name.clone());
                    }
                    ColumnConstraint::NotNull => col.not_null = true,
                    ColumnConstraint::Unique => col.unique = true,
                    ColumnConstraint::Default(expr) => {
                        col.default = Some(eval_literal(expr)?);
                    }
                    ColumnConstraint::Check(expr) => {
                        check_strs.push(format!("{:?}", expr));
                    }
                }
            }
            cols.push(col);
        }

        for tc in &stmt.table_constraints {
            match tc {
                TableConstraint::PrimaryKey(keys) => {
                    for k in keys {
                        primary_key_cols.push(k.clone());
                        if let Some(col) = cols.iter_mut().find(|c| c.name.eq_ignore_ascii_case(k))
                        {
                            col.primary_key = true;
                            col.not_null = true;
                        }
                    }
                }
                TableConstraint::Check(expr) => {
                    check_strs.push(format!("{:?}", expr));
                }
                TableConstraint::Unique(ucols) => {
                    for uc in ucols {
                        if let Some(col) = cols.iter_mut().find(|c| c.name.eq_ignore_ascii_case(uc))
                        {
                            col.unique = true;
                        }
                    }
                }
            }
        }

        // Create heap
        let heap = HeapFile::create(&mut self.page_manager)?;
        let first = heap.first_page.0;
        let last = heap.last_page.0;

        let schema = crate::catalog::TableSchema {
            name: name.clone(),
            columns: cols,
            primary_key: if primary_key_cols.is_empty() {
                None
            } else {
                Some(primary_key_cols)
            },
            check_constraints: check_strs,
            heap_first_page: first,
            heap_last_page: last,
            next_row_seq: 0,
        };

        self.catalog.create_table(schema)?;
        self.heaps.insert(
            name.clone(),
            HeapRef {
                first_page: PageId(first),
                last_page: PageId(last),
            },
        );

        // Auto-commit DDL
        if !self.explicit_txn {
            self.catalog.save()?;
        }

        Ok(QueryResult::with_message(&format!(
            "table {} created",
            name
        )))
    }

    fn exec_drop_table(&mut self, name: &str, if_exists: bool) -> Result<QueryResult> {
        let name = name.to_lowercase();
        if self.catalog.get_table(&name).is_none() {
            if if_exists {
                return Ok(QueryResult::with_message(&format!(
                    "table {} does not exist",
                    name
                )));
            }
            return Err(MiniDbError::NoSuchTable(name));
        }
        self.catalog.drop_table(&name)?;
        self.heaps.remove(&name);
        if !self.explicit_txn {
            self.catalog.save()?;
        }
        Ok(QueryResult::with_message(&format!(
            "table {} dropped",
            name
        )))
    }

    fn exec_create_index(
        &mut self,
        stmt: crate::parser::ast::CreateIndexStmt,
    ) -> Result<QueryResult> {
        let idx_name = stmt.name.to_lowercase();
        let tbl_name = stmt.table.to_lowercase();

        if self.catalog.get_index(&idx_name).is_some() {
            if stmt.if_not_exists {
                return Ok(QueryResult::with_message(&format!(
                    "index {} already exists",
                    idx_name
                )));
            }
            return Err(MiniDbError::IndexExists(idx_name));
        }

        let schema = self
            .catalog
            .get_table(&tbl_name)
            .ok_or_else(|| MiniDbError::NoSuchTable(tbl_name.clone()))?
            .clone();

        let col_idx = schema
            .col_index(&stmt.column)
            .ok_or_else(|| MiniDbError::NoSuchColumn(stmt.column.clone()))?;

        // Build BTree from existing rows
        let types = schema.col_types();
        let heap = self
            .heaps
            .get(&tbl_name)
            .ok_or_else(|| MiniDbError::Internal(format!("no heap for {}", tbl_name)))?
            .clone_ref();
        let rows = heap.scan(&mut self.page_manager, &types)?;

        let mut btree = BTreeIndex::new();
        for (rid, row) in &rows {
            btree.insert(&mut self.page_manager, &row[col_idx], *rid)?;
        }

        let def = crate::catalog::IndexDef {
            name: idx_name.clone(),
            table: tbl_name.clone(),
            column: stmt.column.clone(),
            unique: stmt.unique,
            root_page: btree.root.0,
        };
        self.catalog.create_index(def)?;

        if !self.explicit_txn {
            self.catalog.save()?;
        }
        Ok(QueryResult::with_message(&format!(
            "index {} created",
            idx_name
        )))
    }

    fn exec_drop_index(&mut self, name: &str, if_exists: bool) -> Result<QueryResult> {
        let name = name.to_lowercase();
        if self.catalog.get_index(&name).is_none() {
            if if_exists {
                return Ok(QueryResult::with_message(&format!(
                    "index {} does not exist",
                    name
                )));
            }
            return Err(MiniDbError::NoSuchIndex(name));
        }
        self.catalog.drop_index(&name)?;
        if !self.explicit_txn {
            self.catalog.save()?;
        }
        Ok(QueryResult::with_message(&format!(
            "index {} dropped",
            name
        )))
    }

    fn exec_meta(&self, mc: MetaCmd) -> Result<QueryResult> {
        match mc {
            MetaCmd::Tables => {
                let mut rows: Vec<Row> = self
                    .catalog
                    .tables
                    .keys()
                    .map(|n| vec![Value::Text(n.clone())])
                    .collect();
                rows.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
                Ok(QueryResult {
                    schema: Schema::new(vec![("name".into(), DataType::Text)]),
                    rows,
                    affected_rows: None,
                    message: None,
                })
            }
            MetaCmd::Indexes => {
                let mut rows: Vec<Row> = self
                    .catalog
                    .indexes
                    .values()
                    .map(|i| {
                        vec![
                            Value::Text(i.name.clone()),
                            Value::Text(i.table.clone()),
                            Value::Text(i.column.clone()),
                        ]
                    })
                    .collect();
                rows.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
                Ok(QueryResult {
                    schema: Schema::new(vec![
                        ("name".into(), DataType::Text),
                        ("table".into(), DataType::Text),
                        ("column".into(), DataType::Text),
                    ]),
                    rows,
                    affected_rows: None,
                    message: None,
                })
            }
            MetaCmd::Schema => {
                let mut rows: Vec<Row> = Vec::new();
                let mut table_names: Vec<String> = self.catalog.tables.keys().cloned().collect();
                table_names.sort();
                for tname in &table_names {
                    let ts = &self.catalog.tables[tname];
                    for col in &ts.columns {
                        rows.push(vec![
                            Value::Text(tname.clone()),
                            Value::Text(col.name.clone()),
                            Value::Text(col.data_type.to_string()),
                            Value::Bool(col.not_null),
                            Value::Bool(col.primary_key),
                        ]);
                    }
                }
                Ok(QueryResult {
                    schema: Schema::new(vec![
                        ("table".into(), DataType::Text),
                        ("column".into(), DataType::Text),
                        ("type".into(), DataType::Text),
                        ("not_null".into(), DataType::Bool),
                        ("primary_key".into(), DataType::Bool),
                    ]),
                    rows,
                    affected_rows: None,
                    message: None,
                })
            }
            MetaCmd::Exit => Ok(QueryResult::with_message("exit")),
            MetaCmd::Format(_) => Ok(QueryResult::with_message("format changed")),
        }
    }
}

// ======== Expression evaluator ========

pub fn eval_expr(expr: &Expr, row: &Row, schema: &Schema) -> Result<Value> {
    match expr {
        Expr::Literal(v) => Ok(v.clone()),
        Expr::Column(name) => {
            // Try exact match first
            if let Some(idx) = schema.col_index_icase(name) {
                return Ok(row[idx].clone());
            }
            // Try table-qualified match (t.col)
            for (i, (col_name, _)) in schema.columns.iter().enumerate() {
                if let Some(suffix) = col_name.rfind('.') {
                    if col_name[suffix + 1..].eq_ignore_ascii_case(name) {
                        return Ok(row[i].clone());
                    }
                }
            }
            Err(MiniDbError::NoSuchColumn(name.clone()))
        }
        Expr::QualifiedColumn(tbl, col) => {
            let full = format!("{}.{}", tbl, col);
            if let Some(idx) = schema.col_index_icase(&full) {
                return Ok(row[idx].clone());
            }
            // Also try matching just the column name in case table prefix is absent
            for (i, (col_name, _)) in schema.columns.iter().enumerate() {
                if col_name.eq_ignore_ascii_case(col) {
                    return Ok(row[i].clone());
                }
            }
            Err(MiniDbError::NoSuchColumn(format!("{}.{}", tbl, col)))
        }
        Expr::BinaryOp { left, op, right } => {
            let lv = eval_expr(left, row, schema)?;
            let rv = eval_expr(right, row, schema)?;
            eval_binary_op(&lv, op, &rv)
        }
        Expr::UnaryOp { op, expr } => {
            let v = eval_expr(expr, row, schema)?;
            match op {
                UnaryOp::Neg => v.negate(),
                UnaryOp::Not => match v.as_bool() {
                    None => Ok(Value::Null),
                    Some(b) => Ok(Value::Bool(!b)),
                },
            }
        }
        Expr::IsNull(inner) => {
            let v = eval_expr(inner, row, schema)?;
            Ok(Value::Bool(v.is_null()))
        }
        Expr::IsNotNull(inner) => {
            let v = eval_expr(inner, row, schema)?;
            Ok(Value::Bool(!v.is_null()))
        }
        Expr::FunctionCall { name, args } => eval_function(name, args, row, schema),
        Expr::Like {
            expr,
            pattern,
            negated,
        } => {
            let v = eval_expr(expr, row, schema)?;
            let p = eval_expr(pattern, row, schema)?;
            match (v, p) {
                (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                (Value::Text(s), Value::Text(pat)) => {
                    let matched = like_match(&s, &pat);
                    Ok(Value::Bool(if *negated { !matched } else { matched }))
                }
                _ => Ok(Value::Bool(false)),
            }
        }
        Expr::InList {
            expr,
            list,
            negated,
        } => {
            let v = eval_expr(expr, row, schema)?;
            if v.is_null() {
                return Ok(Value::Null);
            }
            let mut found = false;
            for item in list {
                let iv = eval_expr(item, row, schema)?;
                if iv.is_null() {
                    continue;
                }
                if v.sql_eq(&iv) == Some(true) {
                    found = true;
                    break;
                }
            }
            Ok(Value::Bool(if *negated { !found } else { found }))
        }
    }
}

fn eval_binary_op(lv: &Value, op: &BinaryOp, rv: &Value) -> Result<Value> {
    match op {
        BinaryOp::And => match (lv.as_bool(), rv.as_bool()) {
            (Some(false), _) | (_, Some(false)) => Ok(Value::Bool(false)),
            (Some(true), Some(true)) => Ok(Value::Bool(true)),
            _ => Ok(Value::Null),
        },
        BinaryOp::Or => match (lv.as_bool(), rv.as_bool()) {
            (Some(true), _) | (_, Some(true)) => Ok(Value::Bool(true)),
            (Some(false), Some(false)) => Ok(Value::Bool(false)),
            _ => Ok(Value::Null),
        },
        BinaryOp::Eq => Ok(match lv.sql_eq(rv) {
            None => Value::Null,
            Some(b) => Value::Bool(b),
        }),
        BinaryOp::Ne => Ok(match lv.sql_eq(rv) {
            None => Value::Null,
            Some(b) => Value::Bool(!b),
        }),
        BinaryOp::Lt => Ok(match lv.sql_lt(rv) {
            None => Value::Null,
            Some(b) => Value::Bool(b),
        }),
        BinaryOp::Le => Ok(match lv.sql_le(rv) {
            None => Value::Null,
            Some(b) => Value::Bool(b),
        }),
        BinaryOp::Gt => Ok(match lv.sql_gt(rv) {
            None => Value::Null,
            Some(b) => Value::Bool(b),
        }),
        BinaryOp::Ge => Ok(match lv.sql_ge(rv) {
            None => Value::Null,
            Some(b) => Value::Bool(b),
        }),
        BinaryOp::Add => lv.add(rv),
        BinaryOp::Sub => lv.sub(rv),
        BinaryOp::Mul => lv.mul(rv),
        BinaryOp::Div => lv.div(rv),
        BinaryOp::Concat => lv.concat(rv),
    }
}

fn eval_function(name: &str, args: &[Expr], row: &Row, schema: &Schema) -> Result<Value> {
    match name.to_uppercase().as_str() {
        "LENGTH" => {
            if args.len() != 1 {
                return Err(MiniDbError::InvalidArgument(
                    "LENGTH takes 1 argument".into(),
                ));
            }
            let v = eval_expr(&args[0], row, schema)?;
            match v {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Int(s.chars().count() as i64)),
                Value::Int(i) => Ok(Value::Int(i.to_string().len() as i64)),
                _ => Err(MiniDbError::TypeMismatch("LENGTH requires text".into())),
            }
        }
        "LOWER" => {
            if args.len() != 1 {
                return Err(MiniDbError::InvalidArgument(
                    "LOWER takes 1 argument".into(),
                ));
            }
            let v = eval_expr(&args[0], row, schema)?;
            match v {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Text(s.to_lowercase())),
                _ => Err(MiniDbError::TypeMismatch("LOWER requires text".into())),
            }
        }
        "UPPER" => {
            if args.len() != 1 {
                return Err(MiniDbError::InvalidArgument(
                    "UPPER takes 1 argument".into(),
                ));
            }
            let v = eval_expr(&args[0], row, schema)?;
            match v {
                Value::Null => Ok(Value::Null),
                Value::Text(s) => Ok(Value::Text(s.to_uppercase())),
                _ => Err(MiniDbError::TypeMismatch("UPPER requires text".into())),
            }
        }
        "SUBSTR" | "SUBSTRING" => {
            if args.len() < 2 {
                return Err(MiniDbError::InvalidArgument(
                    "SUBSTR takes 2 or 3 arguments".into(),
                ));
            }
            let s = match eval_expr(&args[0], row, schema)? {
                Value::Null => return Ok(Value::Null),
                Value::Text(t) => t,
                _ => return Err(MiniDbError::TypeMismatch("SUBSTR requires text".into())),
            };
            let start = match eval_expr(&args[1], row, schema)? {
                Value::Null => return Ok(Value::Null),
                Value::Int(i) => i,
                _ => return Err(MiniDbError::TypeMismatch("SUBSTR start must be int".into())),
            };
            let chars: Vec<char> = s.chars().collect();
            let start_idx = if start > 0 {
                (start - 1) as usize
            } else if start < 0 {
                chars.len().saturating_sub((-start) as usize)
            } else {
                0
            };

            let result: String = if args.len() >= 3 {
                let len = match eval_expr(&args[2], row, schema)? {
                    Value::Null => return Ok(Value::Null),
                    Value::Int(i) => i.max(0) as usize,
                    _ => {
                        return Err(MiniDbError::TypeMismatch(
                            "SUBSTR length must be int".into(),
                        ))
                    }
                };
                chars[start_idx.min(chars.len())..]
                    .iter()
                    .take(len)
                    .collect()
            } else {
                chars[start_idx.min(chars.len())..].iter().collect()
            };
            Ok(Value::Text(result))
        }
        "ABS" => {
            if args.len() != 1 {
                return Err(MiniDbError::InvalidArgument("ABS takes 1 argument".into()));
            }
            let v = eval_expr(&args[0], row, schema)?;
            match v {
                Value::Null => Ok(Value::Null),
                Value::Int(i) => Ok(Value::Int(i.abs())),
                Value::Real(r) => Ok(Value::Real(r.abs())),
                _ => Err(MiniDbError::TypeMismatch("ABS requires numeric".into())),
            }
        }
        "COUNT" => {
            // COUNT(*) - just return 1 for now (aggregate handled elsewhere)
            // For scalar context, return 1
            Ok(Value::Int(1))
        }
        "COALESCE" => {
            for arg in args {
                let v = eval_expr(arg, row, schema)?;
                if !v.is_null() {
                    return Ok(v);
                }
            }
            Ok(Value::Null)
        }
        _ => Err(MiniDbError::NotSupported(format!(
            "unknown function: {}",
            name
        ))),
    }
}

fn eval_predicate(expr: &Expr, row: &Row, schema: &Schema) -> Result<bool> {
    match eval_expr(expr, row, schema)? {
        Value::Bool(b) => Ok(b),
        Value::Null => Ok(false),
        Value::Int(i) => Ok(i != 0),
        _ => Ok(false),
    }
}

fn like_match(s: &str, pattern: &str) -> bool {
    let s_chars: Vec<char> = s.chars().collect();
    let p_chars: Vec<char> = pattern.chars().collect();
    like_recursive(&s_chars, &p_chars, 0, 0)
}

fn like_recursive(s: &[char], p: &[char], si: usize, pi: usize) -> bool {
    if pi == p.len() {
        return si == s.len();
    }
    if p[pi] == '%' {
        // Match zero or more characters
        for i in si..=s.len() {
            if like_recursive(s, p, i, pi + 1) {
                return true;
            }
        }
        false
    } else if p[pi] == '_' {
        si < s.len() && like_recursive(s, p, si + 1, pi + 1)
    } else {
        si < s.len()
            && s[si].to_uppercase().next() == p[pi].to_uppercase().next()
            && like_recursive(s, p, si + 1, pi + 1)
    }
}

// ======== Helper functions ========

fn project_row(projections: &[SelectItem], row: &Row, schema: &Schema) -> Result<Row> {
    let mut out = Vec::new();
    for item in projections {
        match item {
            SelectItem::Star => {
                out.extend_from_slice(row);
            }
            SelectItem::TableStar(tbl) => {
                for (i, (name, _)) in schema.columns.iter().enumerate() {
                    let prefix = format!("{}.", tbl.to_lowercase());
                    if name.to_lowercase().starts_with(&prefix)
                        || name.to_lowercase() == tbl.to_lowercase()
                    {
                        out.push(row[i].clone());
                    }
                }
            }
            SelectItem::Expr(expr, _) => {
                out.push(eval_expr(expr, row, schema)?);
            }
        }
    }
    Ok(out)
}

fn expr_name(expr: &Expr) -> String {
    match expr {
        Expr::Column(n) => n.clone(),
        Expr::QualifiedColumn(t, c) => format!("{}.{}", t, c),
        Expr::Literal(v) => v.to_string(),
        Expr::FunctionCall { name, .. } => name.clone(),
        _ => "expr".to_string(),
    }
}

fn infer_expr_type(expr: &Expr, schema: &Schema) -> DataType {
    match expr {
        Expr::Literal(v) => v.data_type().unwrap_or(DataType::Text),
        Expr::Column(n) => schema
            .col_index_icase(n)
            .and_then(|i| schema.columns.get(i))
            .map(|(_, dt)| dt.clone())
            .unwrap_or(DataType::Text),
        Expr::FunctionCall { name, .. } => match name.to_uppercase().as_str() {
            "LENGTH" | "ABS" => DataType::Int,
            "LOWER" | "UPPER" | "SUBSTR" | "COALESCE" => DataType::Text,
            _ => DataType::Text,
        },
        _ => DataType::Text,
    }
}

fn eval_literal(expr: &Expr) -> Result<Value> {
    match expr {
        Expr::Literal(v) => Ok(v.clone()),
        _ => Err(MiniDbError::NotSupported(
            "DEFAULT must be a literal value".into(),
        )),
    }
}

/// Build an insert row from column specs and value expressions.
fn build_insert_row(
    schema: &crate::catalog::TableSchema,
    columns: &Option<Vec<String>>,
    value_exprs: &[Expr],
    db: &Database,
) -> Result<Row> {
    let empty_row: Row = vec![];
    let empty_schema = Schema::empty();

    match columns {
        None => {
            // Positional insert
            if value_exprs.len() != schema.columns.len() {
                return Err(MiniDbError::InvalidArgument(format!(
                    "expected {} values, got {}",
                    schema.columns.len(),
                    value_exprs.len()
                )));
            }
            let mut row = Vec::new();
            for (i, expr) in value_exprs.iter().enumerate() {
                let val = eval_expr(expr, &empty_row, &empty_schema)?;
                let val = val.coerce_to(&schema.columns[i].data_type)?;
                row.push(val);
            }
            Ok(row)
        }
        Some(col_names) => {
            if value_exprs.len() != col_names.len() {
                return Err(MiniDbError::InvalidArgument(format!(
                    "expected {} values for {} columns",
                    col_names.len(),
                    col_names.len()
                )));
            }
            let mut row: Row = schema
                .columns
                .iter()
                .map(|c| c.default.clone().unwrap_or(Value::Null))
                .collect();

            for (col_name, expr) in col_names.iter().zip(value_exprs.iter()) {
                let idx = schema
                    .col_index(col_name)
                    .ok_or_else(|| MiniDbError::NoSuchColumn(col_name.clone()))?;
                let val = eval_expr(expr, &empty_row, &empty_schema)?;
                let val = val.coerce_to(&schema.columns[idx].data_type)?;
                row[idx] = val;
            }
            Ok(row)
        }
    }
}

/// Check NOT NULL, UNIQUE, PRIMARY KEY, and CHECK constraints.
fn check_row_constraints(
    schema: &crate::catalog::TableSchema,
    row: &Row,
    db: &mut Database,
    skip_rid: Option<RowId>,
) -> Result<()> {
    for (i, col) in schema.columns.iter().enumerate() {
        let val = &row[i];

        if col.not_null && val.is_null() {
            return Err(MiniDbError::NullViolation(col.name.clone()));
        }
    }
    Ok(())
}

// Helper to expose WAL decode functions for replay
pub mod wal_decode {
    use crate::error::Result;
    use crate::storage::heap::RowId;
    use crate::types::{DataType, Row};

    pub fn decode_string(data: &[u8], pos: &mut usize) -> Result<String> {
        crate::storage::wal::decode_string_pos(data, pos)
    }

    pub fn decode_row_id(data: &[u8], pos: &mut usize) -> Result<RowId> {
        crate::storage::wal::decode_row_id_pos(data, pos)
    }

    pub fn decode_row(data: &[u8], pos: &mut usize, types: &[DataType]) -> Result<Row> {
        crate::storage::wal::decode_row_pos(data, pos, types)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ast::*;
    use crate::types::Value;
    use tempfile::NamedTempFile;

    #[test]
    fn test_eval_expr_literal() {
        let row: Row = vec![];
        let schema = Schema::empty();
        let expr = Expr::Literal(Value::Int(42));
        assert_eq!(eval_expr(&expr, &row, &schema).unwrap(), Value::Int(42));
    }

    #[test]
    fn test_eval_binary_add() {
        let row: Row = vec![];
        let schema = Schema::empty();
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Literal(Value::Int(3))),
            op: BinaryOp::Add,
            right: Box::new(Expr::Literal(Value::Int(4))),
        };
        assert_eq!(eval_expr(&expr, &row, &schema).unwrap(), Value::Int(7));
    }

    #[test]
    fn test_eval_function_length() {
        let row: Row = vec![];
        let schema = Schema::empty();
        let expr = Expr::FunctionCall {
            name: "LENGTH".into(),
            args: vec![Expr::Literal(Value::Text("hello".into()))],
        };
        assert_eq!(eval_expr(&expr, &row, &schema).unwrap(), Value::Int(5));
    }

    #[test]
    fn test_eval_coalesce() {
        let row: Row = vec![];
        let schema = Schema::empty();
        let expr = Expr::FunctionCall {
            name: "COALESCE".into(),
            args: vec![Expr::Literal(Value::Null), Expr::Literal(Value::Int(42))],
        };
        assert_eq!(eval_expr(&expr, &row, &schema).unwrap(), Value::Int(42));
    }

    #[test]
    fn test_like_match() {
        assert!(like_match("hello", "hel%"));
        assert!(like_match("hello", "%llo"));
        assert!(like_match("hello", "h_llo"));
        assert!(!like_match("hello", "world%"));
        assert!(like_match("hello world", "hello%"));
    }
}
