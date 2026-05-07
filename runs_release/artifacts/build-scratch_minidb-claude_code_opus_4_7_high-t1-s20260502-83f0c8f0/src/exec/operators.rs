//! Physical operator implementations.

use crate::engine::{Engine, RowRef, VisibleRow};
use crate::error::{MiniDbError, Result};
use crate::exec::expr::{bind, eval, eval_predicate, BoundExpr};
use crate::parser::ast::{Expr, OrderItem, Projection};
use crate::plan::physical::IndexBound;
use crate::types::{DataType, Schema, SchemaColumn, Tri, Value};
use std::cmp::Ordering;
use std::collections::VecDeque;

/// Execution context shared between operators in a single plan run.
pub struct ExecCtx<'a> {
    pub engine: &'a mut Engine,
}

/// A row flowing through the operator pipeline.
#[derive(Debug, Clone)]
pub struct ExecRow {
    pub rref: Option<RowRef>,
    pub source_table: Option<String>,
    pub values: Vec<Value>,
}

impl ExecRow {
    pub fn from_visible(table: &str, vr: VisibleRow) -> Self {
        ExecRow {
            rref: Some(vr.rref),
            source_table: Some(table.to_string()),
            values: vr.values,
        }
    }
    pub fn from_values(values: Vec<Value>) -> Self {
        ExecRow {
            rref: None,
            source_table: None,
            values,
        }
    }
}

pub trait PhysicalOperator {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()>;
    fn next(&mut self, ctx: &mut ExecCtx) -> Result<Option<ExecRow>>;
    fn close(&mut self, ctx: &mut ExecCtx) -> Result<()>;
    fn schema(&self) -> &Schema;
}

// ---- SeqScan ----
pub struct SeqScan {
    pub table: String,
    pub alias: Option<String>,
    schema: Schema,
    rows: Option<VecDeque<ExecRow>>,
}

impl SeqScan {
    pub fn new(table: String, alias: Option<String>, engine: &Engine) -> Result<Self> {
        let table_def = engine.catalog().get_table(&table)?;
        let qualifier = alias.clone().unwrap_or_else(|| table.clone());
        let schema = Schema::new(
            table_def
                .columns
                .iter()
                .map(|c| SchemaColumn {
                    name: c.name.clone(),
                    qualifier: Some(qualifier.clone()),
                    data_type: Some(c.data_type),
                })
                .collect(),
        );
        Ok(SeqScan {
            table,
            alias,
            schema,
            rows: None,
        })
    }
}

impl PhysicalOperator for SeqScan {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        let visible = ctx.engine.scan_table(&self.table)?;
        let table_name = self.table.clone();
        let mut q = VecDeque::with_capacity(visible.len());
        for v in visible {
            q.push_back(ExecRow::from_visible(&table_name, v));
        }
        self.rows = Some(q);
        Ok(())
    }
    fn next(&mut self, _ctx: &mut ExecCtx) -> Result<Option<ExecRow>> {
        Ok(self.rows.as_mut().and_then(|q| q.pop_front()))
    }
    fn close(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        self.rows = None;
        Ok(())
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

// ---- IndexScan ----
pub struct IndexScan {
    pub table: String,
    pub alias: Option<String>,
    pub column: String,
    pub bound: IndexBound,
    schema: Schema,
    rows: Option<VecDeque<ExecRow>>,
}

impl IndexScan {
    pub fn new(
        table: String,
        alias: Option<String>,
        column: String,
        bound: IndexBound,
        engine: &Engine,
    ) -> Result<Self> {
        let table_def = engine.catalog().get_table(&table)?;
        let qualifier = alias.clone().unwrap_or_else(|| table.clone());
        let schema = Schema::new(
            table_def
                .columns
                .iter()
                .map(|c| SchemaColumn {
                    name: c.name.clone(),
                    qualifier: Some(qualifier.clone()),
                    data_type: Some(c.data_type),
                })
                .collect(),
        );
        Ok(IndexScan {
            table,
            alias,
            column,
            bound,
            schema,
            rows: None,
        })
    }
}

impl PhysicalOperator for IndexScan {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        let mut q = VecDeque::new();
        match &self.bound {
            IndexBound::Equal(v) => {
                for vr in ctx.engine.index_lookup_eq(&self.table, &self.column, v)? {
                    q.push_back(ExecRow::from_visible(&self.table, vr));
                }
            }
            _ => {
                // For range scans, fall back to a filtered table scan to honour
                // the buffered transaction state. Equality-only is the
                // hot-path; range support is structural and works correctly via
                // the seq-scan fallback below.
                let col = self.column.clone();
                let bound = self.bound.clone();
                let visible = ctx.engine.scan_table(&self.table)?;
                let table_def = ctx.engine.catalog().get_table(&self.table)?.clone();
                let col_idx = table_def
                    .column_index(&col)
                    .ok_or_else(|| MiniDbError::ty(format!("no column: {}", col)))?;
                for vr in visible {
                    if matches_range(&vr.values[col_idx], &bound) {
                        q.push_back(ExecRow::from_visible(&self.table, vr));
                    }
                }
            }
        }
        self.rows = Some(q);
        Ok(())
    }
    fn next(&mut self, _ctx: &mut ExecCtx) -> Result<Option<ExecRow>> {
        Ok(self.rows.as_mut().and_then(|q| q.pop_front()))
    }
    fn close(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        self.rows = None;
        Ok(())
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

fn matches_range(v: &Value, bound: &IndexBound) -> bool {
    match bound {
        IndexBound::Equal(k) => Value::cmp_sql(v, k)
            .map(|o| o == Ordering::Equal)
            .unwrap_or(false),
        IndexBound::Range {
            lo,
            hi,
            lo_inclusive,
            hi_inclusive,
        } => {
            if let Some(l) = lo {
                match Value::cmp_sql(v, l) {
                    None => return false,
                    Some(Ordering::Less) => return false,
                    Some(Ordering::Equal) if !*lo_inclusive => return false,
                    _ => {}
                }
            }
            if let Some(h) = hi {
                match Value::cmp_sql(v, h) {
                    None => return false,
                    Some(Ordering::Greater) => return false,
                    Some(Ordering::Equal) if !*hi_inclusive => return false,
                    _ => {}
                }
            }
            true
        }
    }
}

// ---- Filter ----
pub struct Filter {
    pub predicate: Expr,
    pub child: Box<dyn PhysicalOperator>,
    bound: Option<BoundExpr>,
}

impl Filter {
    pub fn new(predicate: Expr, child: Box<dyn PhysicalOperator>) -> Self {
        Filter {
            predicate,
            child,
            bound: None,
        }
    }
}

impl PhysicalOperator for Filter {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)?;
        self.bound = Some(bind(&self.predicate, self.child.schema())?);
        Ok(())
    }
    fn next(&mut self, ctx: &mut ExecCtx) -> Result<Option<ExecRow>> {
        let bound = self.bound.as_ref().expect("filter not opened");
        loop {
            let row = match self.child.next(ctx)? {
                Some(r) => r,
                None => return Ok(None),
            };
            if eval_predicate(bound, &row.values)?.is_true() {
                return Ok(Some(row));
            }
        }
    }
    fn close(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.close(ctx)?;
        self.bound = None;
        Ok(())
    }
    fn schema(&self) -> &Schema {
        self.child.schema()
    }
}

// ---- Project ----
pub struct Project {
    pub child: Box<dyn PhysicalOperator>,
    pub projections: Vec<Projection>,
    output_schema: Schema,
    /// One bound expression and an output column name per output column.
    plan: Vec<(BoundExpr, String)>,
}

impl Project {
    pub fn new(projections: Vec<Projection>, child: Box<dyn PhysicalOperator>) -> Self {
        Project {
            child,
            projections,
            output_schema: Schema::empty(),
            plan: Vec::new(),
        }
    }

    fn build_plan(&mut self) -> Result<()> {
        let input = self.child.schema().clone();
        let mut output_cols = Vec::new();
        let mut plan = Vec::new();
        for proj in &self.projections {
            match proj {
                Projection::Star => {
                    for (i, col) in input.columns.iter().enumerate() {
                        plan.push((BoundExpr::Column(i), col.name.clone()));
                        output_cols.push(col.clone());
                    }
                }
                Projection::QualifiedStar(q) => {
                    for (i, col) in input.columns.iter().enumerate() {
                        if col
                            .qualifier
                            .as_ref()
                            .map(|cq| cq.eq_ignore_ascii_case(q))
                            .unwrap_or(false)
                        {
                            plan.push((BoundExpr::Column(i), col.name.clone()));
                            output_cols.push(col.clone());
                        }
                    }
                }
                Projection::Expr { expr, alias } => {
                    let bound = bind(expr, &input)?;
                    let name = alias.clone().unwrap_or_else(|| display_expr(expr));
                    plan.push((bound, name.clone()));
                    output_cols.push(SchemaColumn {
                        name,
                        qualifier: None,
                        data_type: infer_type(expr, &input),
                    });
                }
            }
        }
        self.output_schema = Schema::new(output_cols);
        self.plan = plan;
        Ok(())
    }
}

fn display_expr(e: &Expr) -> String {
    match e {
        Expr::Column { qualifier, name } => match qualifier {
            Some(q) => format!("{}.{}", q, name),
            None => name.clone(),
        },
        Expr::Literal(v) => format!("{}", v),
        _ => "expr".to_string(),
    }
}

fn infer_type(e: &Expr, input: &Schema) -> Option<DataType> {
    match e {
        Expr::Literal(v) => v.data_type(),
        Expr::Column { qualifier, name } => input
            .resolve(qualifier.as_deref(), name)
            .ok()
            .and_then(|i| input.columns[i].data_type),
        _ => None,
    }
}

impl PhysicalOperator for Project {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)?;
        self.build_plan()?;
        Ok(())
    }
    fn next(&mut self, ctx: &mut ExecCtx) -> Result<Option<ExecRow>> {
        let row = match self.child.next(ctx)? {
            Some(r) => r,
            None => return Ok(None),
        };
        let mut values = Vec::with_capacity(self.plan.len());
        for (be, _) in &self.plan {
            values.push(eval(be, &row.values)?);
        }
        Ok(Some(ExecRow {
            rref: None,
            source_table: None,
            values,
        }))
    }
    fn close(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.close(ctx)
    }
    fn schema(&self) -> &Schema {
        &self.output_schema
    }
}

// ---- NestedLoopJoin ----
pub struct NestedLoopJoin {
    pub left: Box<dyn PhysicalOperator>,
    pub right: Box<dyn PhysicalOperator>,
    pub on: Expr,
    schema: Schema,
    bound: Option<BoundExpr>,
    right_rows: Vec<ExecRow>,
    left_current: Option<ExecRow>,
    right_pos: usize,
}

impl NestedLoopJoin {
    pub fn new(
        left: Box<dyn PhysicalOperator>,
        right: Box<dyn PhysicalOperator>,
        on: Expr,
    ) -> Self {
        NestedLoopJoin {
            left,
            right,
            on,
            schema: Schema::empty(),
            bound: None,
            right_rows: Vec::new(),
            left_current: None,
            right_pos: 0,
        }
    }
}

impl PhysicalOperator for NestedLoopJoin {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.left.open(ctx)?;
        self.right.open(ctx)?;
        // Materialise right side so we can replay it for every left row.
        self.right_rows.clear();
        while let Some(r) = self.right.next(ctx)? {
            self.right_rows.push(r);
        }
        self.right.close(ctx)?;
        self.schema = Schema::concat(self.left.schema(), self.right.schema());
        self.bound = Some(bind(&self.on, &self.schema)?);
        self.left_current = self.left.next(ctx)?;
        self.right_pos = 0;
        Ok(())
    }
    fn next(&mut self, ctx: &mut ExecCtx) -> Result<Option<ExecRow>> {
        let bound = self.bound.as_ref().expect("join not opened");
        loop {
            let l = match &self.left_current {
                Some(r) => r.clone(),
                None => return Ok(None),
            };
            while self.right_pos < self.right_rows.len() {
                let r = &self.right_rows[self.right_pos];
                self.right_pos += 1;
                let mut values = Vec::with_capacity(l.values.len() + r.values.len());
                values.extend_from_slice(&l.values);
                values.extend_from_slice(&r.values);
                let tri = eval_predicate(bound, &values)?;
                if tri.is_true() {
                    return Ok(Some(ExecRow {
                        rref: None,
                        source_table: None,
                        values,
                    }));
                }
            }
            self.left_current = self.left.next(ctx)?;
            self.right_pos = 0;
        }
    }
    fn close(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.left.close(ctx)?;
        self.right_rows.clear();
        self.left_current = None;
        Ok(())
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

// ---- Sort ----
pub struct Sort {
    pub child: Box<dyn PhysicalOperator>,
    pub items: Vec<OrderItem>,
    rows: Option<VecDeque<ExecRow>>,
}

impl Sort {
    pub fn new(items: Vec<OrderItem>, child: Box<dyn PhysicalOperator>) -> Self {
        Sort {
            child,
            items,
            rows: None,
        }
    }
}

impl PhysicalOperator for Sort {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)?;
        let bound: Vec<(BoundExpr, bool)> = self
            .items
            .iter()
            .map(|i| Ok::<_, MiniDbError>((bind(&i.expr, self.child.schema())?, i.descending)))
            .collect::<Result<Vec<_>>>()?;
        let mut all = Vec::new();
        while let Some(r) = self.child.next(ctx)? {
            let keys: Result<Vec<_>> = bound.iter().map(|(b, _)| eval(b, &r.values)).collect();
            all.push((keys?, r));
        }
        all.sort_by(|a, b| {
            for ((va, _), (vb, _)) in
                a.0.iter()
                    .zip(b.0.iter())
                    .map(|(x, y)| ((x, &b.1), (y, &a.1)))
            {
                let _ = (va, vb);
                break;
            }
            // Stable, multi-key comparison.
            for (i, (av, bv)) in a.0.iter().zip(b.0.iter()).enumerate() {
                let ord = sort_cmp(av, bv);
                let descending = bound[i].1;
                let ord = if descending { ord.reverse() } else { ord };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            Ordering::Equal
        });
        self.rows = Some(all.into_iter().map(|(_, r)| r).collect());
        Ok(())
    }
    fn next(&mut self, _ctx: &mut ExecCtx) -> Result<Option<ExecRow>> {
        Ok(self.rows.as_mut().and_then(|q| q.pop_front()))
    }
    fn close(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.close(ctx)?;
        self.rows = None;
        Ok(())
    }
    fn schema(&self) -> &Schema {
        self.child.schema()
    }
}

fn sort_cmp(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        _ => Value::cmp_sql(a, b).unwrap_or(Ordering::Equal),
    }
}

// ---- Limit ----
pub struct Limit {
    pub child: Box<dyn PhysicalOperator>,
    pub limit: i64,
    pub offset: i64,
    skipped: i64,
    yielded: i64,
}

impl Limit {
    pub fn new(limit: i64, offset: i64, child: Box<dyn PhysicalOperator>) -> Self {
        Limit {
            child,
            limit,
            offset,
            skipped: 0,
            yielded: 0,
        }
    }
}

impl PhysicalOperator for Limit {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)?;
        self.skipped = 0;
        self.yielded = 0;
        Ok(())
    }
    fn next(&mut self, ctx: &mut ExecCtx) -> Result<Option<ExecRow>> {
        while self.skipped < self.offset {
            if self.child.next(ctx)?.is_none() {
                return Ok(None);
            }
            self.skipped += 1;
        }
        if self.yielded >= self.limit {
            return Ok(None);
        }
        match self.child.next(ctx)? {
            Some(r) => {
                self.yielded += 1;
                Ok(Some(r))
            }
            None => Ok(None),
        }
    }
    fn close(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.close(ctx)
    }
    fn schema(&self) -> &Schema {
        self.child.schema()
    }
}

// ---- Insert ----
pub struct Insert {
    pub table: String,
    pub columns: Option<Vec<String>>,
    pub rows: Vec<Vec<Expr>>,
    schema: Schema,
    affected: i64,
    done: bool,
}

impl Insert {
    pub fn new(table: String, columns: Option<Vec<String>>, rows: Vec<Vec<Expr>>) -> Self {
        Insert {
            table,
            columns,
            rows,
            schema: count_schema(),
            affected: 0,
            done: false,
        }
    }
}

fn count_schema() -> Schema {
    Schema::new(vec![SchemaColumn {
        name: "count".into(),
        qualifier: None,
        data_type: Some(DataType::Int),
    }])
}

impl PhysicalOperator for Insert {
    fn open(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        Ok(())
    }
    fn next(&mut self, ctx: &mut ExecCtx) -> Result<Option<ExecRow>> {
        if self.done {
            return Ok(None);
        }
        let table_def = ctx.engine.catalog().get_table(&self.table)?.clone();
        for raw_row in &self.rows {
            // Evaluate each expression with no row context (literal-only).
            let evald: Result<Vec<Value>> = raw_row
                .iter()
                .map(|e| {
                    let bound = bind(e, &Schema::empty())?;
                    eval(&bound, &[])
                })
                .collect();
            let evald = evald?;
            let row = build_full_row(&table_def, self.columns.as_deref(), evald)?;
            // Constraint checks.
            for (i, c) in table_def.columns.iter().enumerate() {
                let v = &row[i];
                if c.not_null && matches!(v, Value::Null) {
                    return Err(MiniDbError::constraint(format!(
                        "NOT NULL violated for column {}",
                        c.name
                    )));
                }
                if !matches!(v, Value::Null) && c.unique {
                    if let Some(idx) = ctx.engine.catalog().find_index_for(&self.table, &c.name) {
                        if idx.unique {
                            // Look up existing rows with this key. Reads also
                            // see buffered txn state.
                            let existing = ctx.engine.index_lookup_eq(&self.table, &c.name, v)?;
                            if !existing.is_empty() {
                                return Err(MiniDbError::constraint(format!(
                                    "UNIQUE violated for column {}",
                                    c.name
                                )));
                            }
                        }
                    } else {
                        // Fall back to a scan.
                        let scan = ctx.engine.scan_table(&self.table)?;
                        for vr in scan {
                            if Value::cmp_sql(&vr.values[i], v)
                                .map(|o| o == Ordering::Equal)
                                .unwrap_or(false)
                            {
                                return Err(MiniDbError::constraint(format!(
                                    "UNIQUE violated for column {}",
                                    c.name
                                )));
                            }
                        }
                    }
                }
            }
            ctx.engine.dml_insert(&self.table, row)?;
            self.affected += 1;
        }
        self.done = true;
        Ok(Some(ExecRow::from_values(vec![Value::Int(self.affected)])))
    }
    fn close(&mut self, _ctx: &mut ExecCtx) -> Result<()> {
        Ok(())
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

fn build_full_row(
    table: &crate::catalog::Table,
    column_spec: Option<&[String]>,
    given: Vec<Value>,
) -> Result<Vec<Value>> {
    if let Some(cols) = column_spec {
        if cols.len() != given.len() {
            return Err(MiniDbError::ty(format!(
                "INSERT columns/values mismatch: {} columns, {} values",
                cols.len(),
                given.len()
            )));
        }
        let mut row = vec![Value::Null; table.columns.len()];
        for (cname, value) in cols.iter().zip(given.into_iter()) {
            let pos = table
                .column_index(cname)
                .ok_or_else(|| MiniDbError::ty(format!("no such column: {}", cname)))?;
            let target = table.columns[pos].data_type;
            row[pos] = if matches!(value, Value::Null) {
                Value::Null
            } else {
                value.coerce_to(target)?
            };
        }
        // Apply defaults for columns the user didn't mention.
        let provided: std::collections::HashSet<String> =
            cols.iter().map(|s| s.to_ascii_lowercase()).collect();
        for (i, c) in table.columns.iter().enumerate() {
            if !provided.contains(&c.name.to_ascii_lowercase()) {
                if let Some(d) = &c.default {
                    row[i] = d.clone().coerce_to(c.data_type)?;
                } else {
                    row[i] = Value::Null;
                }
            }
        }
        Ok(row)
    } else {
        if given.len() != table.columns.len() {
            return Err(MiniDbError::ty(format!(
                "INSERT expects {} values, got {}",
                table.columns.len(),
                given.len()
            )));
        }
        let mut row = Vec::with_capacity(given.len());
        for (v, c) in given.into_iter().zip(table.columns.iter()) {
            row.push(if matches!(v, Value::Null) {
                Value::Null
            } else {
                v.coerce_to(c.data_type)?
            });
        }
        Ok(row)
    }
}

// ---- Update ----
pub struct Update {
    pub table: String,
    pub child: Box<dyn PhysicalOperator>,
    pub assignments: Vec<(String, Expr)>,
    schema: Schema,
    affected: i64,
    done: bool,
}

impl Update {
    pub fn new(
        table: String,
        assignments: Vec<(String, Expr)>,
        child: Box<dyn PhysicalOperator>,
    ) -> Self {
        Update {
            table,
            child,
            assignments,
            schema: count_schema(),
            affected: 0,
            done: false,
        }
    }
}

impl PhysicalOperator for Update {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)
    }
    fn next(&mut self, ctx: &mut ExecCtx) -> Result<Option<ExecRow>> {
        if self.done {
            return Ok(None);
        }
        // Bind RHS expressions against child schema once.
        let input = self.child.schema().clone();
        let bindings: Vec<(usize, BoundExpr)> = {
            let table_def = ctx.engine.catalog().get_table(&self.table)?.clone();
            let mut out = Vec::new();
            for (col, expr) in &self.assignments {
                let pos = table_def
                    .column_index(col)
                    .ok_or_else(|| MiniDbError::ty(format!("no such column: {}", col)))?;
                let b = bind(expr, &input)?;
                out.push((pos, b));
            }
            out
        };
        let table_def = ctx.engine.catalog().get_table(&self.table)?.clone();
        // Materialise to avoid re-reading mid-mutation.
        let mut rows = Vec::new();
        while let Some(r) = self.child.next(ctx)? {
            rows.push(r);
        }
        for r in rows {
            let mut new_values = r.values.clone();
            for (idx, b) in &bindings {
                let v = eval(b, &r.values)?;
                let dt = table_def.columns[*idx].data_type;
                let v = if matches!(v, Value::Null) {
                    Value::Null
                } else {
                    v.coerce_to(dt)?
                };
                if matches!(v, Value::Null) && table_def.columns[*idx].not_null {
                    return Err(MiniDbError::constraint(format!(
                        "NOT NULL violated for column {}",
                        table_def.columns[*idx].name
                    )));
                }
                new_values[*idx] = v;
            }
            match r.rref {
                Some(RowRef::Persistent(rid)) => {
                    ctx.engine
                        .dml_update_persistent(&self.table, rid, new_values)?;
                }
                Some(RowRef::Pending(i)) => {
                    ctx.engine.dml_update_pending(&self.table, i, new_values)?;
                }
                None => {
                    return Err(MiniDbError::runtime(
                        "UPDATE source row missing row reference",
                    ));
                }
            }
            self.affected += 1;
        }
        self.done = true;
        Ok(Some(ExecRow::from_values(vec![Value::Int(self.affected)])))
    }
    fn close(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.close(ctx)
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

// ---- Delete ----
pub struct Delete {
    pub table: String,
    pub child: Box<dyn PhysicalOperator>,
    schema: Schema,
    affected: i64,
    done: bool,
}

impl Delete {
    pub fn new(table: String, child: Box<dyn PhysicalOperator>) -> Self {
        Delete {
            table,
            child,
            schema: count_schema(),
            affected: 0,
            done: false,
        }
    }
}

impl PhysicalOperator for Delete {
    fn open(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.open(ctx)
    }
    fn next(&mut self, ctx: &mut ExecCtx) -> Result<Option<ExecRow>> {
        if self.done {
            return Ok(None);
        }
        let mut rows = Vec::new();
        while let Some(r) = self.child.next(ctx)? {
            rows.push(r);
        }
        for r in rows {
            match r.rref {
                Some(RowRef::Persistent(rid)) => {
                    ctx.engine.dml_delete_persistent(&self.table, rid)?;
                }
                Some(RowRef::Pending(i)) => {
                    ctx.engine.dml_delete_pending(&self.table, i)?;
                }
                None => {
                    return Err(MiniDbError::runtime(
                        "DELETE source row missing row reference",
                    ));
                }
            }
            self.affected += 1;
        }
        self.done = true;
        Ok(Some(ExecRow::from_values(vec![Value::Int(self.affected)])))
    }
    fn close(&mut self, ctx: &mut ExecCtx) -> Result<()> {
        self.child.close(ctx)
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
}

// ---- helpers ----
pub fn _unused(_: Tri) {} // avoid unused-import warnings for Tri in some builds
