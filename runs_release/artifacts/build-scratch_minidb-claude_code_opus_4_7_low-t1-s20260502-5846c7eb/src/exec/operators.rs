//! Volcano-style physical operators.
//!
//! Each operator implements `PhysicalOperator`. `open` materialises the
//! input lazily where possible; `next` returns one row at a time; `close`
//! releases buffered state.

use crate::error::Result;
use crate::exec::expr::{eval, predicate_truth};
use crate::parser::ast::{Expr, OrderBy, Projection};
use crate::plan::physical::{ColumnRef, Schema};
use crate::storage::btree::BTreeIndex;
use crate::storage::heap::{HeapIter, RowId};
use crate::storage::page::decode_tuple;
use crate::storage::pager::Pager;
use crate::types::{compare_values, Row, Value};

pub struct ExecCtx<'a> {
    pub pager: &'a mut Pager,
    pub catalog: &'a crate::catalog::Catalog,
    pub indexes: &'a std::collections::HashMap<String, BTreeIndex>,
}

pub trait PhysicalOperator {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()>;
    fn next(&mut self, ctx: &mut ExecCtx) -> Result<Option<Row>>;
    fn close(&mut self, ctx: &mut ExecCtx) -> Result<()>;
    fn schema(&self) -> &Schema;
}

// -------- SeqScan --------
pub struct SeqScan {
    pub table: String,
    pub schema: Schema,
    buffer: Vec<(RowId, Row)>,
    cursor: usize,
}

impl SeqScan {
    pub fn new(table: String, schema: Schema) -> Self {
        SeqScan {
            table,
            schema,
            buffer: Vec::new(),
            cursor: 0,
        }
    }
    pub fn rows_with_id(&self) -> &[(RowId, Row)] {
        &self.buffer
    }
}

impl PhysicalOperator for SeqScan {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        let t = ctx.catalog.get_table(&self.table)?;
        let first = t.first_page;
        let types = t.data_types();
        let mut buffer = Vec::new();
        for (rid, bytes) in HeapIter::new(ctx.pager, first) {
            let vals = decode_tuple(&bytes)?;
            let coerced: Result<Vec<Value>> = vals
                .into_iter()
                .zip(types.iter())
                .map(|(v, t)| v.coerce(*t))
                .collect();
            buffer.push((rid, Row::new(coerced?)));
        }
        self.buffer = buffer;
        self.cursor = 0;
        Ok(())
    }
    fn next(&mut self, _ctx: &mut ExecCtx) -> Result<Option<Row>> {
        if self.cursor < self.buffer.len() {
            let row = self.buffer[self.cursor].1.clone();
            self.cursor += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }
    fn close(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        self.buffer.clear();
        Ok(())
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

// -------- IndexScan --------
pub struct IndexScan {
    pub table: String,
    pub index_name: String,
    pub schema: Schema,
    pub eq: Option<Value>,
    pub lo: Option<Value>,
    pub lo_inc: bool,
    pub hi: Option<Value>,
    pub hi_inc: bool,
    rows: Vec<Row>,
    cursor: usize,
}

impl IndexScan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        table: String,
        index_name: String,
        schema: Schema,
        eq: Option<Value>,
        lo: Option<Value>,
        lo_inc: bool,
        hi: Option<Value>,
        hi_inc: bool,
    ) -> Self {
        IndexScan {
            table,
            index_name,
            schema,
            eq,
            lo,
            lo_inc,
            hi,
            hi_inc,
            rows: Vec::new(),
            cursor: 0,
        }
    }
}

impl PhysicalOperator for IndexScan {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        let idx = ctx
            .indexes
            .get(&self.index_name)
            .ok_or_else(|| crate::error::MiniDbError::NoSuchIndex(self.index_name.clone()))?;
        let row_ids = if let Some(eq) = &self.eq {
            idx.equal_range(eq)
        } else {
            idx.range_scan(self.lo.as_ref(), self.lo_inc, self.hi.as_ref(), self.hi_inc)
        };
        let t = ctx.catalog.get_table(&self.table)?;
        let types = t.data_types();
        for rid in row_ids {
            let pg = ctx.pager.read_page(rid.page)?;
            if let Some(buf) = crate::storage::heap::read_tuple(pg, rid.slot) {
                let vals = decode_tuple(buf)?;
                let coerced: Result<Vec<Value>> = vals
                    .into_iter()
                    .zip(types.iter())
                    .map(|(v, t)| v.coerce(*t))
                    .collect();
                self.rows.push(Row::new(coerced?));
            }
        }
        Ok(())
    }
    fn next(&mut self, _ctx: &mut ExecCtx) -> Result<Option<Row>> {
        if self.cursor < self.rows.len() {
            let r = self.rows[self.cursor].clone();
            self.cursor += 1;
            Ok(Some(r))
        } else {
            Ok(None)
        }
    }
    fn close(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        self.rows.clear();
        Ok(())
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

// -------- Filter --------
pub struct Filter {
    pub input: Box<dyn PhysicalOperator>,
    pub predicate: Expr,
    pub schema: Schema,
}

impl PhysicalOperator for Filter {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.input.open(ctx)
    }
    fn next(&mut self, ctx: &mut ExecCtx) -> Result<Option<Row>> {
        loop {
            let row = match self.input.next(ctx)? {
                Some(r) => r,
                None => return Ok(None),
            };
            let v = eval(&self.predicate, &row, self.input.schema())?;
            if predicate_truth(&v).is_true() {
                return Ok(Some(row));
            }
        }
    }
    fn close(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.input.close(ctx)
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

// -------- Project --------
pub struct Project {
    pub input: Box<dyn PhysicalOperator>,
    pub projections: Vec<Projection>,
    pub schema: Schema,
}

impl PhysicalOperator for Project {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.input.open(ctx)
    }
    fn next(&mut self, ctx: &mut ExecCtx) -> Result<Option<Row>> {
        let row = match self.input.next(ctx)? {
            Some(r) => r,
            None => return Ok(None),
        };
        let inschema = self.input.schema();
        let mut out = Vec::with_capacity(self.schema.columns.len());
        for p in &self.projections {
            match p {
                Projection::Star => {
                    out.extend(row.0.iter().cloned());
                }
                Projection::QualifiedStar(t) => {
                    for (i, c) in inschema.columns.iter().enumerate() {
                        if c.table.as_deref() == Some(t.as_str()) {
                            out.push(row.0[i].clone());
                        }
                    }
                }
                Projection::Expr(e, _) => {
                    out.push(eval(e, &row, inschema)?);
                }
            }
        }
        Ok(Some(Row::new(out)))
    }
    fn close(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.input.close(ctx)
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

// -------- NestedLoopJoin --------
pub struct NestedLoopJoin {
    pub left: Box<dyn PhysicalOperator>,
    pub right: Box<dyn PhysicalOperator>,
    pub on: Expr,
    pub schema: Schema,
    right_buf: Vec<Row>,
    cur_left: Option<Row>,
    right_idx: usize,
}

impl NestedLoopJoin {
    pub fn new(
        left: Box<dyn PhysicalOperator>,
        right: Box<dyn PhysicalOperator>,
        on: Expr,
        schema: Schema,
    ) -> Self {
        NestedLoopJoin {
            left,
            right,
            on,
            schema,
            right_buf: Vec::new(),
            cur_left: None,
            right_idx: 0,
        }
    }
}

impl PhysicalOperator for NestedLoopJoin {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.left.open(ctx)?;
        self.right.open(ctx)?;
        // Materialise right side
        self.right_buf.clear();
        while let Some(r) = self.right.next(ctx)? {
            self.right_buf.push(r);
        }
        self.cur_left = self.left.next(ctx)?;
        self.right_idx = 0;
        Ok(())
    }
    fn next(&mut self, ctx: &mut ExecCtx) -> Result<Option<Row>> {
        loop {
            let lr = match &self.cur_left {
                Some(r) => r.clone(),
                None => return Ok(None),
            };
            while self.right_idx < self.right_buf.len() {
                let rr = self.right_buf[self.right_idx].clone();
                self.right_idx += 1;
                let mut combined = Vec::with_capacity(lr.len() + rr.len());
                combined.extend_from_slice(&lr.0);
                combined.extend_from_slice(&rr.0);
                let row = Row::new(combined);
                let v = eval(&self.on, &row, &self.schema)?;
                if predicate_truth(&v).is_true() {
                    return Ok(Some(row));
                }
            }
            self.cur_left = self.left.next(ctx)?;
            self.right_idx = 0;
            if self.cur_left.is_none() {
                return Ok(None);
            }
        }
    }
    fn close(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.left.close(ctx)?;
        self.right.close(ctx)?;
        self.right_buf.clear();
        Ok(())
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

// -------- Sort --------
pub struct Sort {
    pub input: Box<dyn PhysicalOperator>,
    pub keys: Vec<OrderBy>,
    pub schema: Schema,
    sorted: Vec<Row>,
    cursor: usize,
}

impl Sort {
    pub fn new(input: Box<dyn PhysicalOperator>, keys: Vec<OrderBy>, schema: Schema) -> Self {
        Sort {
            input,
            keys,
            schema,
            sorted: Vec::new(),
            cursor: 0,
        }
    }
}

impl PhysicalOperator for Sort {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.input.open(ctx)?;
        let inschema = self.input.schema().clone();
        let mut rows = Vec::new();
        while let Some(r) = self.input.next(ctx)? {
            rows.push(r);
        }
        let keys = self.keys.clone();
        rows.sort_by(|a, b| {
            for k in &keys {
                let va = eval(&k.expr, a, &inschema).unwrap_or(Value::Null);
                let vb = eval(&k.expr, b, &inschema).unwrap_or(Value::Null);
                let cmp = match (va.is_null(), vb.is_null()) {
                    (true, true) => std::cmp::Ordering::Equal,
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    (false, false) => compare_values(&va, &vb).unwrap_or(std::cmp::Ordering::Equal),
                };
                let cmp = if k.desc { cmp.reverse() } else { cmp };
                if cmp != std::cmp::Ordering::Equal {
                    return cmp;
                }
            }
            std::cmp::Ordering::Equal
        });
        self.sorted = rows;
        self.cursor = 0;
        Ok(())
    }
    fn next(&mut self, _ctx: &mut ExecCtx) -> Result<Option<Row>> {
        if self.cursor < self.sorted.len() {
            let r = self.sorted[self.cursor].clone();
            self.cursor += 1;
            Ok(Some(r))
        } else {
            Ok(None)
        }
    }
    fn close(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.sorted.clear();
        self.input.close(ctx)
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

// -------- Limit --------
pub struct Limit {
    pub input: Box<dyn PhysicalOperator>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub schema: Schema,
    skipped: i64,
    emitted: i64,
}

impl Limit {
    pub fn new(
        input: Box<dyn PhysicalOperator>,
        limit: Option<i64>,
        offset: Option<i64>,
        schema: Schema,
    ) -> Self {
        Limit {
            input,
            limit,
            offset,
            schema,
            skipped: 0,
            emitted: 0,
        }
    }
}

impl PhysicalOperator for Limit {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.input.open(ctx)?;
        self.skipped = 0;
        self.emitted = 0;
        Ok(())
    }
    fn next(&mut self, ctx: &mut ExecCtx) -> Result<Option<Row>> {
        let off = self.offset.unwrap_or(0);
        while self.skipped < off {
            if self.input.next(ctx)?.is_none() {
                return Ok(None);
            }
            self.skipped += 1;
        }
        if let Some(l) = self.limit {
            if self.emitted >= l {
                return Ok(None);
            }
        }
        match self.input.next(ctx)? {
            Some(r) => {
                self.emitted += 1;
                Ok(Some(r))
            }
            None => Ok(None),
        }
    }
    fn close(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.input.close(ctx)
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

// -------- Build operator tree from physical plan --------
use crate::plan::physical::PhysicalPlan;

pub fn build(plan: PhysicalPlan) -> Box<dyn PhysicalOperator> {
    match plan {
        PhysicalPlan::SeqScan { table, schema, .. } => Box::new(SeqScan::new(table, schema)),
        PhysicalPlan::IndexScan {
            table,
            index_name,
            eq,
            lo,
            lo_inclusive,
            hi,
            hi_inclusive,
            schema,
            ..
        } => Box::new(IndexScan::new(
            table,
            index_name,
            schema,
            eq,
            lo,
            lo_inclusive,
            hi,
            hi_inclusive,
        )),
        PhysicalPlan::Filter {
            input,
            predicate,
            schema,
        } => Box::new(Filter {
            input: build(*input),
            predicate,
            schema,
        }),
        PhysicalPlan::Project {
            input,
            projections,
            schema,
        } => Box::new(Project {
            input: build(*input),
            projections,
            schema,
        }),
        PhysicalPlan::NestedLoopJoin {
            left,
            right,
            on,
            schema,
        } => Box::new(NestedLoopJoin::new(build(*left), build(*right), on, schema)),
        PhysicalPlan::Sort {
            input,
            keys,
            schema,
        } => Box::new(Sort::new(build(*input), keys, schema)),
        PhysicalPlan::Limit {
            input,
            limit,
            offset,
            schema,
        } => Box::new(Limit::new(build(*input), limit, offset, schema)),
    }
}

// -------- Insert/Update/Delete operators (tree-shape but driven by engine) --------
pub struct Insert {
    pub table: String,
    pub schema: Schema,
}
impl PhysicalOperator for Insert {
    fn open(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        Ok(())
    }
    fn next(&mut self, _ctx: &mut ExecCtx) -> Result<Option<Row>> {
        Ok(None)
    }
    fn close(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

pub struct Update {
    pub table: String,
    pub schema: Schema,
}
impl PhysicalOperator for Update {
    fn open(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        Ok(())
    }
    fn next(&mut self, _ctx: &mut ExecCtx) -> Result<Option<Row>> {
        Ok(None)
    }
    fn close(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

pub struct Delete {
    pub table: String,
    pub schema: Schema,
}
impl PhysicalOperator for Delete {
    fn open(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        Ok(())
    }
    fn next(&mut self, _ctx: &mut ExecCtx) -> Result<Option<Row>> {
        Ok(None)
    }
    fn close(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

/// Build the schema for a base table (all columns, qualified by table name).
pub fn schema_for_table(table_name: &str, table: &crate::catalog::TableSchema) -> Schema {
    Schema {
        columns: table
            .columns
            .iter()
            .map(|c| ColumnRef {
                table: Some(table_name.to_string()),
                name: c.name.clone(),
                data_type: c.data_type,
            })
            .collect(),
    }
}
