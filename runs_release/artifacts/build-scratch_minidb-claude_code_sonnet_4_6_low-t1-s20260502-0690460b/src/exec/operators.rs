/// Volcano-style physical operators
use crate::error::{MiniDbError, Result};
use crate::exec::eval::Evaluator;
use crate::parser::ast::{BinOp, Expr};
use crate::plan::{IndexPredicate, OutputColumn, ProjectItem, Schema, SortKey};
use crate::storage::btree::BTree;
use crate::storage::heap::{deserialize_row, serialize_row, Heap};
use crate::types::{Row, Value};
use std::collections::HashMap;

/// The core iterator trait for physical operators
pub trait PhysicalOperator {
    fn schema(&self) -> &Schema;
    fn open(&mut self) -> Result<()>;
    fn next(&mut self) -> Result<Option<Row>>;
    fn close(&mut self) -> Result<()>;
}

// ── SeqScan ─────────────────────────────────────────────────────────────────

pub struct SeqScan {
    schema: Schema,
    rows: Vec<(u64, Row)>,
    pos: usize,
}

impl SeqScan {
    pub fn new(table_name: &str, heap: &Heap, schema_def: &[crate::types::ColumnDef]) -> Self {
        let schema = Schema::new(
            schema_def
                .iter()
                .map(|c| OutputColumn {
                    name: c.name.clone(),
                    table: Some(table_name.to_string()),
                    dtype: Some(c.dtype.clone()),
                })
                .collect(),
        );
        let rows: Vec<(u64, Row)> = heap.scan().collect();
        SeqScan {
            schema,
            rows,
            pos: 0,
        }
    }

    pub fn row_id_at(&self, idx: usize) -> Option<u64> {
        self.rows.get(idx.wrapping_sub(1)).map(|(id, _)| *id)
    }
}

impl PhysicalOperator for SeqScan {
    fn schema(&self) -> &Schema {
        &self.schema
    }
    fn open(&mut self) -> Result<()> {
        self.pos = 0;
        Ok(())
    }
    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos < self.rows.len() {
            let row = self.rows[self.pos].1.clone();
            self.pos += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

// ── IndexScan ────────────────────────────────────────────────────────────────

pub struct IndexScan {
    schema: Schema,
    rows: Vec<Row>,
    pos: usize,
}

impl IndexScan {
    pub fn new(
        table_name: &str,
        heap: &Heap,
        btree: &BTree,
        predicate: &IndexPredicate,
        schema_def: &[crate::types::ColumnDef],
    ) -> Self {
        let schema = Schema::new(
            schema_def
                .iter()
                .map(|c| OutputColumn {
                    name: c.name.clone(),
                    table: Some(table_name.to_string()),
                    dtype: Some(c.dtype.clone()),
                })
                .collect(),
        );

        let row_ids = match predicate {
            IndexPredicate::Eq(v) => btree.lookup_eq(v),
            IndexPredicate::Lt(v) => btree
                .lookup_range(None, Some(v))
                .into_iter()
                .filter(|&rid| heap.get_row(rid).ok().flatten().is_some())
                .filter(|&rid| {
                    if let Ok(Some(row)) = heap.get_row(rid) {
                        // We'd need col idx here; for now return all
                        true
                    } else {
                        false
                    }
                })
                .collect(),
            IndexPredicate::Lte(v) => btree.lookup_range(None, Some(v)),
            IndexPredicate::Gt(v) => btree
                .lookup_range(Some(v), None)
                .into_iter()
                .filter(|&rid| {
                    if let Ok(Some(row)) = heap.get_row(rid) {
                        true
                    } else {
                        false
                    }
                })
                .collect(),
            IndexPredicate::Gte(v) => btree.lookup_range(Some(v), None),
            IndexPredicate::Range { low, high } => btree.lookup_range(low.as_ref(), high.as_ref()),
        };

        let rows: Vec<Row> = row_ids
            .into_iter()
            .filter_map(|rid| heap.get_row(rid).ok().flatten())
            .collect();

        IndexScan {
            schema,
            rows,
            pos: 0,
        }
    }
}

impl PhysicalOperator for IndexScan {
    fn schema(&self) -> &Schema {
        &self.schema
    }
    fn open(&mut self) -> Result<()> {
        self.pos = 0;
        Ok(())
    }
    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos < self.rows.len() {
            let row = self.rows[self.pos].clone();
            self.pos += 1;
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

// ── Filter ───────────────────────────────────────────────────────────────────

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
    fn open(&mut self) -> Result<()> {
        self.input.open()
    }
    fn next(&mut self) -> Result<Option<Row>> {
        loop {
            match self.input.next()? {
                None => return Ok(None),
                Some(row) => {
                    if Evaluator::eval_bool(&self.predicate, &row, &self.schema)? {
                        return Ok(Some(row));
                    }
                }
            }
        }
    }
    fn close(&mut self) -> Result<()> {
        self.input.close()
    }
}

// ── Project ──────────────────────────────────────────────────────────────────

pub struct Project {
    input: Box<dyn PhysicalOperator>,
    items: Vec<ProjectItem>,
    output_schema: Schema,
    input_schema: Schema,
}

impl Project {
    pub fn new(input: Box<dyn PhysicalOperator>, items: Vec<ProjectItem>) -> Self {
        let input_schema = input.schema().clone();
        let output_cols = Self::build_output_cols(&items, &input_schema);
        let output_schema = Schema::new(output_cols);
        Project {
            input,
            items,
            output_schema,
            input_schema,
        }
    }

    fn build_output_cols(items: &[ProjectItem], input: &Schema) -> Vec<OutputColumn> {
        let mut cols = Vec::new();
        for item in items {
            match item {
                ProjectItem::Star => {
                    cols.extend(input.columns.iter().cloned());
                }
                ProjectItem::TableStar(tbl) => {
                    for c in &input.columns {
                        if c.table
                            .as_deref()
                            .map(|t| t.eq_ignore_ascii_case(tbl))
                            .unwrap_or(false)
                        {
                            cols.push(c.clone());
                        }
                    }
                }
                ProjectItem::Expr { expr, alias } => {
                    let name = alias.clone().unwrap_or_else(|| expr_name(expr));
                    cols.push(OutputColumn {
                        name,
                        table: None,
                        dtype: None,
                    });
                }
            }
        }
        cols
    }

    fn project_row(&self, row: &Row) -> Result<Row> {
        let mut out = Vec::new();
        for item in &self.items {
            match item {
                ProjectItem::Star => {
                    out.extend(row.iter().cloned());
                }
                ProjectItem::TableStar(tbl) => {
                    for (i, c) in self.input_schema.columns.iter().enumerate() {
                        if c.table
                            .as_deref()
                            .map(|t| t.eq_ignore_ascii_case(tbl))
                            .unwrap_or(false)
                        {
                            out.push(row.get(i).cloned().unwrap_or(Value::Null));
                        }
                    }
                }
                ProjectItem::Expr { expr, .. } => {
                    out.push(Evaluator::eval(expr, row, &self.input_schema)?);
                }
            }
        }
        Ok(out)
    }
}

impl PhysicalOperator for Project {
    fn schema(&self) -> &Schema {
        &self.output_schema
    }
    fn open(&mut self) -> Result<()> {
        self.input.open()
    }
    fn next(&mut self) -> Result<Option<Row>> {
        match self.input.next()? {
            None => Ok(None),
            Some(row) => Ok(Some(self.project_row(&row)?)),
        }
    }
    fn close(&mut self) -> Result<()> {
        self.input.close()
    }
}

fn expr_name(expr: &Expr) -> String {
    match expr {
        Expr::Column(c) => c.clone(),
        Expr::QualifiedColumn(_, c) => c.clone(),
        Expr::Literal(v) => format!("{}", v),
        Expr::Function { name, .. } => name.clone(),
        _ => "?".into(),
    }
}

// ── NestedLoopJoin ───────────────────────────────────────────────────────────

pub struct NestedLoopJoin {
    left: Box<dyn PhysicalOperator>,
    right_rows: Vec<Row>,
    right_schema_cols: Vec<OutputColumn>,
    condition: Expr,
    schema: Schema,
    current_left: Option<Row>,
    right_pos: usize,
    right_source: Vec<Row>,
}

impl NestedLoopJoin {
    pub fn new(
        left: Box<dyn PhysicalOperator>,
        right: Box<dyn PhysicalOperator>,
        condition: Expr,
    ) -> Self {
        let mut left_cols = left.schema().columns.clone();
        let right_cols = right.schema().columns.clone();
        let mut all_cols = left_cols.clone();
        all_cols.extend(right_cols.iter().cloned());
        let schema = Schema::new(all_cols);

        // Materialize right side
        let right_schema_cols = right_cols;
        NestedLoopJoin {
            left,
            right_rows: Vec::new(),
            right_schema_cols,
            condition,
            schema,
            current_left: None,
            right_pos: 0,
            right_source: Vec::new(),
        }
    }
}

impl PhysicalOperator for NestedLoopJoin {
    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn open(&mut self) -> Result<()> {
        self.left.open()?;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Row>> {
        loop {
            if self.current_left.is_none() {
                self.current_left = self.left.next()?;
                if self.current_left.is_none() {
                    return Ok(None);
                }
                self.right_pos = 0;
            }

            let left_row = self.current_left.as_ref().unwrap().clone();

            while self.right_pos < self.right_source.len() {
                let right_row = self.right_source[self.right_pos].clone();
                self.right_pos += 1;
                let mut combined = left_row.clone();
                combined.extend(right_row.iter().cloned());
                if Evaluator::eval_bool(&self.condition, &combined, &self.schema)? {
                    return Ok(Some(combined));
                }
            }

            // Exhausted right side for this left row
            self.current_left = None;
        }
    }

    fn close(&mut self) -> Result<()> {
        self.left.close()
    }
}

pub struct NestedLoopJoinBuilder {
    pub left: Box<dyn PhysicalOperator>,
    pub right: Vec<Row>,
    pub condition: Expr,
    pub schema: Schema,
    left_row: Option<Row>,
    right_pos: usize,
}

impl NestedLoopJoinBuilder {
    pub fn new(
        left: Box<dyn PhysicalOperator>,
        right: Vec<Row>,
        right_schema: Schema,
        condition: Expr,
    ) -> Self {
        let mut all_cols = left.schema().columns.clone();
        all_cols.extend(right_schema.columns.iter().cloned());
        let schema = Schema::new(all_cols);
        NestedLoopJoinBuilder {
            left,
            right,
            condition,
            schema,
            left_row: None,
            right_pos: 0,
        }
    }
}

impl PhysicalOperator for NestedLoopJoinBuilder {
    fn schema(&self) -> &Schema {
        &self.schema
    }
    fn open(&mut self) -> Result<()> {
        self.left.open()
    }
    fn next(&mut self) -> Result<Option<Row>> {
        loop {
            if self.left_row.is_none() {
                self.left_row = self.left.next()?;
                if self.left_row.is_none() {
                    return Ok(None);
                }
                self.right_pos = 0;
            }
            let lr = self.left_row.as_ref().unwrap().clone();
            while self.right_pos < self.right.len() {
                let rr = self.right[self.right_pos].clone();
                self.right_pos += 1;
                let mut combined = lr.clone();
                combined.extend(rr.iter().cloned());
                if Evaluator::eval_bool(&self.condition, &combined, &self.schema)? {
                    return Ok(Some(combined));
                }
            }
            self.left_row = None;
        }
    }
    fn close(&mut self) -> Result<()> {
        self.left.close()
    }
}

// ── Sort ─────────────────────────────────────────────────────────────────────

pub struct Sort {
    input: Box<dyn PhysicalOperator>,
    keys: Vec<SortKey>,
    schema: Schema,
    sorted: Vec<Row>,
    pos: usize,
    opened: bool,
}

impl Sort {
    pub fn new(input: Box<dyn PhysicalOperator>, keys: Vec<SortKey>) -> Self {
        let schema = input.schema().clone();
        Sort {
            input,
            keys,
            schema,
            sorted: Vec::new(),
            pos: 0,
            opened: false,
        }
    }

    fn materialize_and_sort(&mut self) -> Result<()> {
        self.input.open()?;
        let mut rows = Vec::new();
        while let Some(row) = self.input.next()? {
            rows.push(row);
        }
        self.input.close()?;
        let schema = self.schema.clone();
        let keys = self.keys.clone();
        rows.sort_by(|a, b| {
            for key in &keys {
                let av = Evaluator::eval(&key.expr, a, &schema).unwrap_or(Value::Null);
                let bv = Evaluator::eval(&key.expr, b, &schema).unwrap_or(Value::Null);
                let ord = av.cmp(&bv);
                if ord != std::cmp::Ordering::Equal {
                    return if key.asc { ord } else { ord.reverse() };
                }
            }
            std::cmp::Ordering::Equal
        });
        self.sorted = rows;
        Ok(())
    }
}

impl PhysicalOperator for Sort {
    fn schema(&self) -> &Schema {
        &self.schema
    }
    fn open(&mut self) -> Result<()> {
        if !self.opened {
            self.materialize_and_sort()?;
            self.opened = true;
        }
        self.pos = 0;
        Ok(())
    }
    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos < self.sorted.len() {
            let r = self.sorted[self.pos].clone();
            self.pos += 1;
            Ok(Some(r))
        } else {
            Ok(None)
        }
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

// ── Limit ─────────────────────────────────────────────────────────────────────

pub struct Limit {
    input: Box<dyn PhysicalOperator>,
    limit: usize,
    offset: usize,
    schema: Schema,
    count: usize,
    skipped: usize,
}

impl Limit {
    pub fn new(input: Box<dyn PhysicalOperator>, limit: usize, offset: usize) -> Self {
        let schema = input.schema().clone();
        Limit {
            input,
            limit,
            offset,
            schema,
            count: 0,
            skipped: 0,
        }
    }
}

impl PhysicalOperator for Limit {
    fn schema(&self) -> &Schema {
        &self.schema
    }
    fn open(&mut self) -> Result<()> {
        self.count = 0;
        self.skipped = 0;
        self.input.open()
    }
    fn next(&mut self) -> Result<Option<Row>> {
        // Skip offset rows
        while self.skipped < self.offset {
            match self.input.next()? {
                None => return Ok(None),
                Some(_) => self.skipped += 1,
            }
        }
        if self.count >= self.limit {
            return Ok(None);
        }
        match self.input.next()? {
            None => Ok(None),
            Some(row) => {
                self.count += 1;
                Ok(Some(row))
            }
        }
    }
    fn close(&mut self) -> Result<()> {
        self.input.close()
    }
}

// ── Insert ───────────────────────────────────────────────────────────────────

pub struct Insert {
    schema: Schema,
    // Rows to insert (pre-built)
    rows: Vec<Row>,
    affected: usize,
    done: bool,
}

impl Insert {
    pub fn new(rows: Vec<Row>) -> Self {
        Insert {
            schema: Schema::new(vec![OutputColumn {
                name: "rows_affected".into(),
                table: None,
                dtype: None,
            }]),
            rows,
            affected: 0,
            done: false,
        }
    }

    pub fn rows_affected(&self) -> usize {
        self.rows.len()
    }
}

impl PhysicalOperator for Insert {
    fn schema(&self) -> &Schema {
        &self.schema
    }
    fn open(&mut self) -> Result<()> {
        self.done = false;
        Ok(())
    }
    fn next(&mut self) -> Result<Option<Row>> {
        if self.done {
            Ok(None)
        } else {
            self.done = true;
            Ok(Some(vec![Value::Int(self.rows.len() as i64)]))
        }
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

// ── Update ───────────────────────────────────────────────────────────────────

pub struct Update {
    schema: Schema,
    affected: usize,
    done: bool,
}

impl Update {
    pub fn new(affected: usize) -> Self {
        Update {
            schema: Schema::new(vec![OutputColumn {
                name: "rows_affected".into(),
                table: None,
                dtype: None,
            }]),
            affected,
            done: false,
        }
    }
}

impl PhysicalOperator for Update {
    fn schema(&self) -> &Schema {
        &self.schema
    }
    fn open(&mut self) -> Result<()> {
        self.done = false;
        Ok(())
    }
    fn next(&mut self) -> Result<Option<Row>> {
        if self.done {
            Ok(None)
        } else {
            self.done = true;
            Ok(Some(vec![Value::Int(self.affected as i64)]))
        }
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

// ── Delete ───────────────────────────────────────────────────────────────────

pub struct Delete {
    schema: Schema,
    affected: usize,
    done: bool,
}

impl Delete {
    pub fn new(affected: usize) -> Self {
        Delete {
            schema: Schema::new(vec![OutputColumn {
                name: "rows_affected".into(),
                table: None,
                dtype: None,
            }]),
            affected,
            done: false,
        }
    }
}

impl PhysicalOperator for Delete {
    fn schema(&self) -> &Schema {
        &self.schema
    }
    fn open(&mut self) -> Result<()> {
        self.done = false;
        Ok(())
    }
    fn next(&mut self) -> Result<Option<Row>> {
        if self.done {
            Ok(None)
        } else {
            self.done = true;
            Ok(Some(vec![Value::Int(self.affected as i64)]))
        }
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

// ── ValuesScan ───────────────────────────────────────────────────────────────

pub struct ValuesScan {
    rows: Vec<Row>,
    pos: usize,
    schema: Schema,
}

impl ValuesScan {
    pub fn new(rows: Vec<Row>, schema: Schema) -> Self {
        ValuesScan {
            rows,
            pos: 0,
            schema,
        }
    }
}

impl PhysicalOperator for ValuesScan {
    fn schema(&self) -> &Schema {
        &self.schema
    }
    fn open(&mut self) -> Result<()> {
        self.pos = 0;
        Ok(())
    }
    fn next(&mut self) -> Result<Option<Row>> {
        if self.pos < self.rows.len() {
            let r = self.rows[self.pos].clone();
            self.pos += 1;
            Ok(Some(r))
        } else {
            Ok(None)
        }
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Schema;
    use crate::types::Value;

    fn make_scan(rows: Vec<Row>) -> ValuesScan {
        let schema = Schema::new(vec![OutputColumn {
            name: "x".into(),
            table: None,
            dtype: None,
        }]);
        ValuesScan::new(rows, schema)
    }

    #[test]
    fn test_limit_operator() {
        let rows = vec![
            vec![Value::Int(1)],
            vec![Value::Int(2)],
            vec![Value::Int(3)],
        ];
        let scan = Box::new(make_scan(rows));
        let mut limit = Limit::new(scan, 2, 0);
        limit.open().unwrap();
        assert!(limit.next().unwrap().is_some());
        assert!(limit.next().unwrap().is_some());
        assert!(limit.next().unwrap().is_none());
    }

    #[test]
    fn test_filter_operator() {
        let rows = vec![
            vec![Value::Int(1)],
            vec![Value::Int(2)],
            vec![Value::Int(3)],
        ];
        let scan = Box::new(make_scan(rows));
        let pred = Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column("x".into())),
            right: Box::new(Expr::Literal(Value::Int(1))),
        };
        let mut filter = Filter::new(scan, pred);
        filter.open().unwrap();
        let r1 = filter.next().unwrap().unwrap();
        assert_eq!(r1[0], Value::Int(2));
        let r2 = filter.next().unwrap().unwrap();
        assert_eq!(r2[0], Value::Int(3));
        assert!(filter.next().unwrap().is_none());
    }

    #[test]
    fn test_sort_operator() {
        let rows = vec![
            vec![Value::Int(3)],
            vec![Value::Int(1)],
            vec![Value::Int(2)],
        ];
        let scan = Box::new(make_scan(rows));
        let key = SortKey {
            expr: Expr::Column("x".into()),
            asc: true,
        };
        let mut sort = Sort::new(scan, vec![key]);
        sort.open().unwrap();
        assert_eq!(sort.next().unwrap().unwrap()[0], Value::Int(1));
        assert_eq!(sort.next().unwrap().unwrap()[0], Value::Int(2));
        assert_eq!(sort.next().unwrap().unwrap()[0], Value::Int(3));
    }

    #[test]
    fn test_limit_with_offset() {
        let rows: Vec<Row> = (1..=5).map(|i| vec![Value::Int(i)]).collect();
        let scan = Box::new(make_scan(rows));
        let mut limit = Limit::new(scan, 2, 2);
        limit.open().unwrap();
        assert_eq!(limit.next().unwrap().unwrap()[0], Value::Int(3));
        assert_eq!(limit.next().unwrap().unwrap()[0], Value::Int(4));
        assert!(limit.next().unwrap().is_none());
    }
}
