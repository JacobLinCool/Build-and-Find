//! Concrete physical operators for the Volcano executor.

use std::cmp::Ordering;

use crate::catalog::{ColumnSchema, IndexSchema, TableSchema};
use crate::error::{MiniDbError, Result};
use crate::parser::ast::{self, BinOp, Expr};
use crate::plan::physical::{IndexBounds, PhysicalPlan};
use crate::storage::heap::{Heap, Tuple};
use crate::storage::page::RowId;
use crate::types::{sql_cmp, DataType, TriBool, Value};

use super::expr as expr_eval;
use super::{ColumnRef, ExecCtx, OperatorSchema, PhysicalOperator, Row};

// ---------------- helpers ----------------

fn build_table_schema(table: &TableSchema, alias: Option<&str>) -> OperatorSchema {
    let qualifier = alias
        .map(|a| a.to_string())
        .unwrap_or_else(|| table.name.clone());
    OperatorSchema::new(
        table
            .columns
            .iter()
            .map(|c| ColumnRef {
                table: Some(qualifier.clone()),
                name: c.name.clone(),
                dtype: Some(c.dtype),
            })
            .collect(),
    )
}

fn read_table_rows(ctx: &mut ExecCtx<'_>, schema: &TableSchema) -> Result<Vec<(RowId, Tuple)>> {
    Heap::new(ctx.pager).scan(schema.heap_head)
}

fn read_row_by_id(ctx: &mut ExecCtx<'_>, row: RowId) -> Result<Option<Tuple>> {
    Heap::new(ctx.pager).read(row)
}

fn predicate_passes(v: Value) -> Result<bool> {
    Ok(TriBool::from_value(&v)?.is_true())
}

// ---------------- SeqScan ----------------

pub struct SeqScan {
    table: String,
    alias: Option<String>,
    schema: OperatorSchema,
    rows: Vec<Row>,
    cursor: usize,
}

impl SeqScan {
    pub fn new(ctx: &ExecCtx<'_>, table: String, alias: Option<String>) -> Result<Self> {
        let t = ctx.catalog.get_table(&table)?;
        let schema = build_table_schema(t, alias.as_deref());
        Ok(Self {
            table,
            alias,
            schema,
            rows: Vec::new(),
            cursor: 0,
        })
    }
}

impl PhysicalOperator for SeqScan {
    fn open(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()> {
        let t = ctx.catalog.get_table(&self.table)?.clone();
        let raw = read_table_rows(ctx, &t)?;
        self.rows = raw.into_iter().map(|(_, t)| t.values).collect();
        self.cursor = 0;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecCtx<'_>) -> Result<Option<Row>> {
        if self.cursor < self.rows.len() {
            let r = self.rows[self.cursor].clone();
            self.cursor += 1;
            Ok(Some(r))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self, _ctx: &mut ExecCtx<'_>) -> Result<()> {
        self.rows.clear();
        Ok(())
    }

    fn schema(&self) -> &OperatorSchema {
        &self.schema
    }
}

// ---------------- IndexScan ----------------

pub struct IndexScan {
    table: String,
    index_name: String,
    column: String,
    bounds: IndexBounds,
    residual: Option<Expr>,
    schema: OperatorSchema,
    rows: Vec<Row>,
    cursor: usize,
}

impl IndexScan {
    pub fn new(
        ctx: &ExecCtx<'_>,
        table: String,
        alias: Option<String>,
        index: String,
        column: String,
        bounds: IndexBounds,
        residual: Option<Expr>,
    ) -> Result<Self> {
        let t = ctx.catalog.get_table(&table)?;
        let schema = build_table_schema(t, alias.as_deref());
        Ok(Self {
            table,
            index_name: index,
            column,
            bounds,
            residual,
            schema,
            rows: Vec::new(),
            cursor: 0,
        })
    }
}

impl PhysicalOperator for IndexScan {
    fn open(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()> {
        let table = ctx.catalog.get_table(&self.table)?.clone();
        let dtype = table
            .column(&self.column)
            .ok_or_else(|| MiniDbError::ty(format!("missing column {}", self.column)))?
            .dtype;
        // Evaluate bound expressions against an empty row (they must be const).
        let empty_schema = OperatorSchema::default();
        let empty_row: Row = Vec::new();
        let row_ids = {
            let idx = ctx
                .indexes
                .get(&self.index_name.to_ascii_lowercase())
                .ok_or_else(|| MiniDbError::ty(format!("index '{}' missing", self.index_name)))?;
            if let Some(eq) = &self.bounds.eq {
                let v = expr_eval::eval(eq, &empty_schema, &empty_row)?;
                let v = expr_eval::coerce(v, dtype)?;
                idx.tree.lookup_eq(&v)
            } else {
                let lower_v = match &self.bounds.lower {
                    Some((e, inc)) => {
                        let v = expr_eval::eval(e, &empty_schema, &empty_row)?;
                        Some((expr_eval::coerce(v, dtype)?, *inc))
                    }
                    None => None,
                };
                let upper_v = match &self.bounds.upper {
                    Some((e, inc)) => {
                        let v = expr_eval::eval(e, &empty_schema, &empty_row)?;
                        Some((expr_eval::coerce(v, dtype)?, *inc))
                    }
                    None => None,
                };
                let lower_ref = lower_v.as_ref().map(|(v, b)| (v, *b));
                let upper_ref = upper_v.as_ref().map(|(v, b)| (v, *b));
                idx.tree.lookup_range(lower_ref, upper_ref)
            }
        };

        let mut rows = Vec::new();
        for rid in row_ids {
            if let Some(t) = read_row_by_id(ctx, rid)? {
                let row = t.values;
                if let Some(pred) = &self.residual {
                    let ok = predicate_passes(expr_eval::eval(pred, &self.schema, &row)?)?;
                    if !ok {
                        continue;
                    }
                }
                rows.push(row);
            }
        }
        self.rows = rows;
        self.cursor = 0;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecCtx<'_>) -> Result<Option<Row>> {
        if self.cursor < self.rows.len() {
            let r = self.rows[self.cursor].clone();
            self.cursor += 1;
            Ok(Some(r))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self, _ctx: &mut ExecCtx<'_>) -> Result<()> {
        self.rows.clear();
        Ok(())
    }

    fn schema(&self) -> &OperatorSchema {
        &self.schema
    }
}

// ---------------- Filter ----------------

pub struct Filter {
    pub input: Box<dyn PhysicalOperator>,
    pub pred: Expr,
}

impl PhysicalOperator for Filter {
    fn open(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()> {
        self.input.open(ctx)
    }

    fn next(&mut self, ctx: &mut ExecCtx<'_>) -> Result<Option<Row>> {
        loop {
            match self.input.next(ctx)? {
                None => return Ok(None),
                Some(row) => {
                    let v = expr_eval::eval(&self.pred, self.input.schema(), &row)?;
                    if predicate_passes(v)? {
                        return Ok(Some(row));
                    }
                }
            }
        }
    }

    fn close(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()> {
        self.input.close(ctx)
    }

    fn schema(&self) -> &OperatorSchema {
        self.input.schema()
    }
}

// ---------------- Project ----------------

pub struct Project {
    pub input: Box<dyn PhysicalOperator>,
    /// Per-output column: index into the input row.
    /// For derived expressions, idx is None and `derived` is the expression.
    items: Vec<ProjectItem>,
    schema: OperatorSchema,
}

#[derive(Clone)]
struct ProjectItem {
    source: ProjectSource,
}

#[derive(Clone)]
enum ProjectSource {
    Direct(usize),
    Derived(Expr),
}

impl Project {
    pub fn new(input: Box<dyn PhysicalOperator>, projections: &[ast::Projection]) -> Result<Self> {
        let mut items: Vec<ProjectItem> = Vec::new();
        let mut output_columns: Vec<ColumnRef> = Vec::new();
        let in_schema = input.schema().clone();
        for p in projections {
            match p {
                ast::Projection::All => {
                    for (i, c) in in_schema.columns.iter().enumerate() {
                        items.push(ProjectItem {
                            source: ProjectSource::Direct(i),
                        });
                        output_columns.push(c.clone());
                    }
                }
                ast::Projection::AllOf(qualifier) => {
                    let q_lc = qualifier.to_ascii_lowercase();
                    for (i, c) in in_schema.columns.iter().enumerate() {
                        if c.table.as_deref().map(str::to_ascii_lowercase).as_deref()
                            == Some(q_lc.as_str())
                        {
                            items.push(ProjectItem {
                                source: ProjectSource::Direct(i),
                            });
                            output_columns.push(c.clone());
                        }
                    }
                }
                ast::Projection::Expr { expr, alias } => match expr {
                    Expr::Column { table, name } => {
                        let idx = expr_eval::resolve_column(&in_schema, table.as_deref(), name)?;
                        items.push(ProjectItem {
                            source: ProjectSource::Direct(idx),
                        });
                        let mut cr = in_schema.columns[idx].clone();
                        if let Some(a) = alias {
                            cr.name = a.clone();
                            cr.table = None;
                        }
                        output_columns.push(cr);
                    }
                    other => {
                        items.push(ProjectItem {
                            source: ProjectSource::Derived(other.clone()),
                        });
                        let name = alias.clone().unwrap_or_else(|| display_expr(other));
                        output_columns.push(ColumnRef {
                            table: None,
                            name,
                            dtype: None,
                        });
                    }
                },
            }
        }
        let schema = OperatorSchema::new(output_columns);
        Ok(Self {
            input,
            items,
            schema,
        })
    }
}

fn display_expr(e: &Expr) -> String {
    match e {
        Expr::Literal(v) => format!("{}", v),
        Expr::Column { table, name } => match table {
            Some(t) => format!("{}.{}", t, name),
            None => name.clone(),
        },
        Expr::Binary { op, left, right } => {
            format!(
                "({} {} {})",
                display_expr(left),
                display_op(*op),
                display_expr(right)
            )
        }
        Expr::And(a, b) => format!("({} AND {})", display_expr(a), display_expr(b)),
        Expr::Or(a, b) => format!("({} OR {})", display_expr(a), display_expr(b)),
        Expr::Not(x) => format!("(NOT {})", display_expr(x)),
        Expr::Neg(x) => format!("(-{})", display_expr(x)),
        Expr::IsNull(x) => format!("({} IS NULL)", display_expr(x)),
        Expr::Func { name, args } => {
            let parts: Vec<String> = args.iter().map(display_expr).collect();
            format!("{}({})", name, parts.join(", "))
        }
    }
}

fn display_op(o: BinOp) -> &'static str {
    use BinOp::*;
    match o {
        Eq => "=",
        NotEq => "<>",
        Lt => "<",
        Le => "<=",
        Gt => ">",
        Ge => ">=",
        Add => "+",
        Sub => "-",
        Mul => "*",
        Div => "/",
        Mod => "%",
        Concat => "||",
    }
}

impl PhysicalOperator for Project {
    fn open(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()> {
        self.input.open(ctx)
    }

    fn next(&mut self, ctx: &mut ExecCtx<'_>) -> Result<Option<Row>> {
        let row = match self.input.next(ctx)? {
            Some(r) => r,
            None => return Ok(None),
        };
        let mut out = Vec::with_capacity(self.items.len());
        for item in &self.items {
            let v = match &item.source {
                ProjectSource::Direct(i) => row.get(*i).cloned().unwrap_or(Value::Null),
                ProjectSource::Derived(e) => expr_eval::eval(e, self.input.schema(), &row)?,
            };
            out.push(v);
        }
        Ok(Some(out))
    }

    fn close(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()> {
        self.input.close(ctx)
    }

    fn schema(&self) -> &OperatorSchema {
        &self.schema
    }
}

// ---------------- NestedLoopJoin ----------------

pub struct NestedLoopJoin {
    left: Box<dyn PhysicalOperator>,
    right: Box<dyn PhysicalOperator>,
    on: Expr,
    schema: OperatorSchema,
    left_rows: Vec<Row>,
    right_rows: Vec<Row>,
    li: usize,
    ri: usize,
}

impl NestedLoopJoin {
    pub fn new(
        left: Box<dyn PhysicalOperator>,
        right: Box<dyn PhysicalOperator>,
        on: Expr,
    ) -> Self {
        let mut cols = Vec::new();
        cols.extend(left.schema().columns.clone());
        cols.extend(right.schema().columns.clone());
        let schema = OperatorSchema::new(cols);
        Self {
            left,
            right,
            on,
            schema,
            left_rows: Vec::new(),
            right_rows: Vec::new(),
            li: 0,
            ri: 0,
        }
    }
}

impl PhysicalOperator for NestedLoopJoin {
    fn open(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()> {
        self.left.open(ctx)?;
        self.right.open(ctx)?;
        // Materialize.
        self.left_rows.clear();
        while let Some(r) = self.left.next(ctx)? {
            self.left_rows.push(r);
        }
        self.right_rows.clear();
        while let Some(r) = self.right.next(ctx)? {
            self.right_rows.push(r);
        }
        self.li = 0;
        self.ri = 0;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecCtx<'_>) -> Result<Option<Row>> {
        while self.li < self.left_rows.len() {
            while self.ri < self.right_rows.len() {
                let mut combined = Vec::with_capacity(
                    self.left_rows[self.li].len() + self.right_rows[self.ri].len(),
                );
                combined.extend_from_slice(&self.left_rows[self.li]);
                combined.extend_from_slice(&self.right_rows[self.ri]);
                self.ri += 1;
                let v = expr_eval::eval(&self.on, &self.schema, &combined)?;
                if predicate_passes(v)? {
                    return Ok(Some(combined));
                }
            }
            self.li += 1;
            self.ri = 0;
        }
        Ok(None)
    }

    fn close(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()> {
        self.left.close(ctx)?;
        self.right.close(ctx)?;
        Ok(())
    }

    fn schema(&self) -> &OperatorSchema {
        &self.schema
    }
}

// ---------------- Sort ----------------

pub struct Sort {
    input: Box<dyn PhysicalOperator>,
    keys: Vec<ast::OrderKey>,
    sorted: Vec<Row>,
    cursor: usize,
}

impl Sort {
    pub fn new(input: Box<dyn PhysicalOperator>, keys: Vec<ast::OrderKey>) -> Self {
        Self {
            input,
            keys,
            sorted: Vec::new(),
            cursor: 0,
        }
    }
}

impl PhysicalOperator for Sort {
    fn open(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()> {
        self.input.open(ctx)?;
        let mut rows = Vec::new();
        while let Some(r) = self.input.next(ctx)? {
            rows.push(r);
        }
        let schema = self.input.schema().clone();
        let keys = self.keys.clone();
        // Compute key vectors first to surface evaluation errors deterministically.
        let mut keyed: Vec<(Vec<Value>, Row)> = Vec::with_capacity(rows.len());
        for r in rows {
            let mut ks = Vec::with_capacity(keys.len());
            for k in &keys {
                ks.push(expr_eval::eval(&k.expr, &schema, &r)?);
            }
            keyed.push((ks, r));
        }
        keyed.sort_by(|a, b| {
            for (i, k) in keys.iter().enumerate() {
                let cmp = a.0[i].cmp_total(&b.0[i]);
                let ord = match k.direction {
                    ast::SortDir::Asc => cmp,
                    ast::SortDir::Desc => cmp.reverse(),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            Ordering::Equal
        });
        self.sorted = keyed.into_iter().map(|(_, r)| r).collect();
        self.cursor = 0;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecCtx<'_>) -> Result<Option<Row>> {
        if self.cursor < self.sorted.len() {
            let r = self.sorted[self.cursor].clone();
            self.cursor += 1;
            Ok(Some(r))
        } else {
            Ok(None)
        }
    }

    fn close(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()> {
        self.input.close(ctx)?;
        self.sorted.clear();
        Ok(())
    }

    fn schema(&self) -> &OperatorSchema {
        self.input.schema()
    }
}

// ---------------- Limit ----------------

pub struct Limit {
    input: Box<dyn PhysicalOperator>,
    limit: i64,
    offset: i64,
    skipped: i64,
    emitted: i64,
}

impl Limit {
    pub fn new(input: Box<dyn PhysicalOperator>, limit: i64, offset: i64) -> Self {
        Self {
            input,
            limit,
            offset,
            skipped: 0,
            emitted: 0,
        }
    }
}

impl PhysicalOperator for Limit {
    fn open(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()> {
        self.input.open(ctx)?;
        self.skipped = 0;
        self.emitted = 0;
        Ok(())
    }

    fn next(&mut self, ctx: &mut ExecCtx<'_>) -> Result<Option<Row>> {
        while self.skipped < self.offset {
            match self.input.next(ctx)? {
                Some(_) => self.skipped += 1,
                None => return Ok(None),
            }
        }
        if self.emitted >= self.limit {
            return Ok(None);
        }
        let r = self.input.next(ctx)?;
        if r.is_some() {
            self.emitted += 1;
        }
        Ok(r)
    }

    fn close(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()> {
        self.input.close(ctx)
    }

    fn schema(&self) -> &OperatorSchema {
        self.input.schema()
    }
}

// ---------------- Insert ----------------

pub struct Insert {
    table: String,
    columns: Option<Vec<String>>,
    rows: Vec<Vec<Expr>>,
    schema: OperatorSchema,
    affected: i64,
    yielded: bool,
}

impl Insert {
    pub fn new(table: String, columns: Option<Vec<String>>, rows: Vec<Vec<Expr>>) -> Self {
        let schema = OperatorSchema::new(vec![ColumnRef {
            table: None,
            name: "rows".into(),
            dtype: Some(DataType::Int),
        }]);
        Self {
            table,
            columns,
            rows,
            schema,
            affected: 0,
            yielded: false,
        }
    }

    fn project_to_table_order(&self, table: &TableSchema, row: &[Expr]) -> Result<Vec<Expr>> {
        match &self.columns {
            None => {
                if row.len() != table.columns.len() {
                    return Err(MiniDbError::ty(format!(
                        "INSERT: expected {} values, got {}",
                        table.columns.len(),
                        row.len()
                    )));
                }
                Ok(row.to_vec())
            }
            Some(names) => {
                if names.len() != row.len() {
                    return Err(MiniDbError::ty(format!(
                        "INSERT: column list has {} columns but {} values",
                        names.len(),
                        row.len()
                    )));
                }
                let mut out: Vec<Expr> = (0..table.columns.len())
                    .map(|i| {
                        if let Some(d) = &table.columns[i].default {
                            Expr::Literal(d.clone())
                        } else {
                            Expr::Literal(Value::Null)
                        }
                    })
                    .collect();
                for (n, e) in names.iter().zip(row.iter()) {
                    let idx = table.column_index(n).ok_or_else(|| {
                        MiniDbError::ty(format!("unknown column '{}' on table '{}'", n, table.name))
                    })?;
                    out[idx] = e.clone();
                }
                Ok(out)
            }
        }
    }
}

impl PhysicalOperator for Insert {
    fn open(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()> {
        let table = ctx.catalog.get_table(&self.table)?.clone();
        let checks = table.parsed_checks()?;
        let empty_schema = OperatorSchema::default();
        let empty_row: Row = Vec::new();
        // Collect existing rows for uniqueness checks (full scan; secondary
        // indexes accelerate point lookups but keep this simple).
        let existing = read_table_rows(ctx, &table)?;

        let mut count = 0i64;
        for raw_row in &self.rows {
            let exprs = self.project_to_table_order(&table, raw_row)?;
            let mut values: Vec<Value> = Vec::with_capacity(table.columns.len());
            for (i, e) in exprs.iter().enumerate() {
                let v = expr_eval::eval(e, &empty_schema, &empty_row)?;
                let v = expr_eval::coerce(v, table.columns[i].dtype)?;
                values.push(v);
            }
            // Constraint validation
            check_row_constraints(&table, &values, &checks)?;
            check_uniqueness(&table, &values, &existing, None)?;
            // Heap insert
            let row_id =
                Heap::new(ctx.pager).insert(table.heap_head, &Tuple::new(values.clone()))?;
            // Index updates
            update_indexes_after_insert(ctx, &table, &values, row_id)?;
            count += 1;
        }
        self.affected = count;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecCtx<'_>) -> Result<Option<Row>> {
        if self.yielded {
            return Ok(None);
        }
        self.yielded = true;
        Ok(Some(vec![Value::Int(self.affected)]))
    }

    fn close(&mut self, _ctx: &mut ExecCtx<'_>) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &OperatorSchema {
        &self.schema
    }
}

// ---------------- Update ----------------

pub struct Update {
    table: String,
    assignments: Vec<(String, Expr)>,
    pred: Option<Expr>,
    schema: OperatorSchema,
    affected: i64,
    yielded: bool,
}

impl Update {
    pub fn new(table: String, assignments: Vec<(String, Expr)>, pred: Option<Expr>) -> Self {
        let schema = OperatorSchema::new(vec![ColumnRef {
            table: None,
            name: "rows".into(),
            dtype: Some(DataType::Int),
        }]);
        Self {
            table,
            assignments,
            pred,
            schema,
            affected: 0,
            yielded: false,
        }
    }
}

impl PhysicalOperator for Update {
    fn open(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()> {
        let table = ctx.catalog.get_table(&self.table)?.clone();
        let checks = table.parsed_checks()?;
        let row_schema = build_table_schema(&table, None);
        // Resolve assignment column indices.
        let mut assigns: Vec<(usize, Expr)> = Vec::new();
        for (name, e) in &self.assignments {
            let idx = table.column_index(name).ok_or_else(|| {
                MiniDbError::ty(format!(
                    "unknown column '{}' on table '{}'",
                    name, self.table
                ))
            })?;
            assigns.push((idx, e.clone()));
        }
        let rows = read_table_rows(ctx, &table)?;
        // First pass: choose rows to update + compute new values.
        let mut work: Vec<(RowId, Vec<Value>)> = Vec::new();
        for (rid, t) in &rows {
            let row_values = &t.values;
            let pass = match &self.pred {
                Some(p) => predicate_passes(expr_eval::eval(p, &row_schema, row_values)?)?,
                None => true,
            };
            if !pass {
                continue;
            }
            let mut new_values = row_values.clone();
            for (idx, e) in &assigns {
                let v = expr_eval::eval(e, &row_schema, row_values)?;
                let v = expr_eval::coerce(v, table.columns[*idx].dtype)?;
                new_values[*idx] = v;
            }
            check_row_constraints(&table, &new_values, &checks)?;
            work.push((*rid, new_values));
        }
        // Re-validate uniqueness with the post-update view of the table.
        for i in 0..work.len() {
            let (rid, new_values) = (work[i].0, work[i].1.clone());
            let mut others: Vec<(RowId, Tuple)> = Vec::new();
            for (orig_rid, t) in &rows {
                if *orig_rid == rid {
                    continue;
                }
                // If this rid is also in `work`, use its updated values.
                let updated = work
                    .iter()
                    .find(|(r2, _)| r2 == orig_rid)
                    .map(|(_, v)| v.clone());
                let values = updated.unwrap_or_else(|| t.values.clone());
                others.push((*orig_rid, Tuple::new(values)));
            }
            check_uniqueness(&table, &new_values, &others, Some(rid))?;
        }
        let count = work.len() as i64;
        // Apply updates.
        for (rid, new_values) in work {
            // Remove old index entries.
            let old = read_row_by_id(ctx, rid)?
                .ok_or_else(|| MiniDbError::storage(format!("row {} vanished", rid)))?;
            remove_indexes_for_row(ctx, &table, &old.values, rid)?;
            let new_rid = Heap::new(ctx.pager).update(rid, &Tuple::new(new_values.clone()))?;
            update_indexes_after_insert(ctx, &table, &new_values, new_rid)?;
        }
        self.affected = count;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecCtx<'_>) -> Result<Option<Row>> {
        if self.yielded {
            return Ok(None);
        }
        self.yielded = true;
        Ok(Some(vec![Value::Int(self.affected)]))
    }

    fn close(&mut self, _ctx: &mut ExecCtx<'_>) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &OperatorSchema {
        &self.schema
    }
}

// ---------------- Delete ----------------

pub struct Delete {
    table: String,
    pred: Option<Expr>,
    schema: OperatorSchema,
    affected: i64,
    yielded: bool,
}

impl Delete {
    pub fn new(table: String, pred: Option<Expr>) -> Self {
        let schema = OperatorSchema::new(vec![ColumnRef {
            table: None,
            name: "rows".into(),
            dtype: Some(DataType::Int),
        }]);
        Self {
            table,
            pred,
            schema,
            affected: 0,
            yielded: false,
        }
    }
}

impl PhysicalOperator for Delete {
    fn open(&mut self, ctx: &mut ExecCtx<'_>) -> Result<()> {
        let table = ctx.catalog.get_table(&self.table)?.clone();
        let row_schema = build_table_schema(&table, None);
        let rows = read_table_rows(ctx, &table)?;
        let mut to_delete: Vec<(RowId, Vec<Value>)> = Vec::new();
        for (rid, t) in &rows {
            let pass = match &self.pred {
                Some(p) => predicate_passes(expr_eval::eval(p, &row_schema, &t.values)?)?,
                None => true,
            };
            if pass {
                to_delete.push((*rid, t.values.clone()));
            }
        }
        let count = to_delete.len() as i64;
        for (rid, vals) in to_delete {
            remove_indexes_for_row(ctx, &table, &vals, rid)?;
            Heap::new(ctx.pager).delete(rid)?;
        }
        self.affected = count;
        Ok(())
    }

    fn next(&mut self, _ctx: &mut ExecCtx<'_>) -> Result<Option<Row>> {
        if self.yielded {
            return Ok(None);
        }
        self.yielded = true;
        Ok(Some(vec![Value::Int(self.affected)]))
    }

    fn close(&mut self, _ctx: &mut ExecCtx<'_>) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> &OperatorSchema {
        &self.schema
    }
}

// ---------------- Constraint helpers ----------------

fn check_row_constraints(table: &TableSchema, values: &[Value], checks: &[Expr]) -> Result<()> {
    // NOT NULL / type checks
    for (i, c) in table.columns.iter().enumerate() {
        if c.not_null && values[i].is_null() {
            return Err(MiniDbError::runtime(format!(
                "NOT NULL constraint failed: {}.{}",
                table.name, c.name
            )));
        }
    }
    // Table-level CHECK predicates
    let row_schema = build_table_schema(table, None);
    for c in checks {
        let v = expr_eval::eval(c, &row_schema, &values.to_vec())?;
        match TriBool::from_value(&v)? {
            TriBool::True | TriBool::Unknown => {} // SQL: CHECK passes when result is not FALSE.
            TriBool::False => {
                return Err(MiniDbError::runtime(format!(
                    "CHECK constraint failed on table '{}'",
                    table.name
                )));
            }
        }
    }
    Ok(())
}

fn check_uniqueness(
    table: &TableSchema,
    values: &[Value],
    existing: &[(RowId, Tuple)],
    skip_rid: Option<RowId>,
) -> Result<()> {
    for (i, c) in table.columns.iter().enumerate() {
        if !(c.unique || c.primary_key) {
            continue;
        }
        if values[i].is_null() && !c.primary_key {
            // SQL: UNIQUE allows multiple NULLs.
            continue;
        }
        for (rid, t) in existing {
            if Some(*rid) == skip_rid {
                continue;
            }
            let existing_v = &t.values[i];
            if existing_v.is_null() {
                continue;
            }
            if let Some(Ordering::Equal) = sql_cmp(&values[i], existing_v)? {
                return Err(MiniDbError::runtime(format!(
                    "UNIQUE constraint failed: {}.{}",
                    table.name, c.name
                )));
            }
        }
    }
    Ok(())
}

fn update_indexes_after_insert(
    ctx: &mut ExecCtx<'_>,
    table: &TableSchema,
    values: &[Value],
    row_id: RowId,
) -> Result<()> {
    let related: Vec<IndexSchema> = ctx
        .catalog
        .indexes
        .values()
        .filter(|i| i.table.eq_ignore_ascii_case(&table.name))
        .cloned()
        .collect();
    for idx_schema in related {
        let col_idx = table
            .column_index(&idx_schema.column)
            .ok_or_else(|| MiniDbError::catalog("index column missing"))?;
        let v = values[col_idx].clone();
        let key = idx_schema.name.to_ascii_lowercase();
        let idx = ctx
            .indexes
            .get_mut(&key)
            .ok_or_else(|| MiniDbError::storage(format!("index '{}' not loaded", key)))?;
        idx.tree.insert(v, row_id)?;
        ctx.indexes_dirty.insert(key);
    }
    Ok(())
}

fn remove_indexes_for_row(
    ctx: &mut ExecCtx<'_>,
    table: &TableSchema,
    values: &[Value],
    row_id: RowId,
) -> Result<()> {
    let related: Vec<IndexSchema> = ctx
        .catalog
        .indexes
        .values()
        .filter(|i| i.table.eq_ignore_ascii_case(&table.name))
        .cloned()
        .collect();
    for idx_schema in related {
        let col_idx = table
            .column_index(&idx_schema.column)
            .ok_or_else(|| MiniDbError::catalog("index column missing"))?;
        let v = &values[col_idx];
        let key = idx_schema.name.to_ascii_lowercase();
        let idx = ctx
            .indexes
            .get_mut(&key)
            .ok_or_else(|| MiniDbError::storage(format!("index '{}' not loaded", key)))?;
        idx.tree.remove(v, row_id);
        ctx.indexes_dirty.insert(key);
    }
    Ok(())
}

// ---------------- Build operator tree ----------------

pub fn build_operator(plan: PhysicalPlan, ctx: &ExecCtx<'_>) -> Result<Box<dyn PhysicalOperator>> {
    Ok(match plan {
        PhysicalPlan::SeqScan { table, alias } => Box::new(SeqScan::new(ctx, table, alias)?),
        PhysicalPlan::IndexScan {
            table,
            alias,
            index,
            column,
            bounds,
            residual,
        } => Box::new(IndexScan::new(
            ctx, table, alias, index, column, bounds, residual,
        )?),
        PhysicalPlan::Filter { input, pred } => Box::new(Filter {
            input: build_operator(*input, ctx)?,
            pred,
        }),
        PhysicalPlan::Project { input, exprs } => {
            Box::new(Project::new(build_operator(*input, ctx)?, &exprs)?)
        }
        PhysicalPlan::NestedLoopJoin { left, right, on } => Box::new(NestedLoopJoin::new(
            build_operator(*left, ctx)?,
            build_operator(*right, ctx)?,
            on,
        )),
        PhysicalPlan::Sort { input, keys } => {
            Box::new(Sort::new(build_operator(*input, ctx)?, keys))
        }
        PhysicalPlan::Limit {
            input,
            limit,
            offset,
        } => Box::new(Limit::new(build_operator(*input, ctx)?, limit, offset)),
        PhysicalPlan::Insert {
            table,
            columns,
            rows,
        } => Box::new(Insert::new(table, columns, rows)),
        PhysicalPlan::Update {
            table,
            assignments,
            pred,
        } => Box::new(Update::new(table, assignments, pred)),
        PhysicalPlan::Delete { table, pred } => Box::new(Delete::new(table, pred)),
        PhysicalPlan::Ddl(_) => {
            return Err(MiniDbError::internal(
                "DDL plans must be handled by the engine",
            ))
        }
    })
}

// re-export for ColumnSchema visibility in this file (avoid unused warning)
#[allow(dead_code)]
fn _refer(_c: ColumnSchema) {}
