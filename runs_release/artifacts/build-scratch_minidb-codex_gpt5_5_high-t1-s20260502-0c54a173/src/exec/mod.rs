use crate::catalog::{IndexSchema, TableSchema};
use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;
use crate::storage::btree::{BTreeIndex, RangeOp};
use crate::storage::heap::RowId;
use crate::storage::{DatabaseState, heap::TableData};
use crate::types::{Column, Truth, TruthOrdering, Value};
use std::cmp::Ordering;

#[derive(Debug, Clone)]
pub struct QueryOutput {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub affected: Option<usize>,
    pub mutation: bool,
}

impl QueryOutput {
    pub fn ok() -> Self {
        Self {
            columns: vec![],
            rows: vec![],
            affected: Some(0),
            mutation: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Field {
    pub table: Option<String>,
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone)]
pub struct DataRow {
    pub fields: Vec<Field>,
}

impl DataRow {
    fn empty() -> Self {
        Self { fields: vec![] }
    }

    fn values_with_names(names: &[String], values: Vec<Value>) -> Self {
        Self {
            fields: names
                .iter()
                .cloned()
                .zip(values)
                .map(|(name, value)| Field {
                    table: None,
                    name,
                    value,
                })
                .collect(),
        }
    }
}

pub trait PhysicalOperator {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()>;
    fn next(&mut self, state: &mut DatabaseState) -> Result<Option<DataRow>>;
    fn close(&mut self) -> Result<()>;
    fn schema(&self) -> Vec<String>;
}

pub struct SeqScan {
    table: String,
    rows: Vec<DataRow>,
    pos: usize,
    schema: Vec<String>,
}

impl SeqScan {
    pub fn new(table: String) -> Self {
        Self {
            table,
            rows: vec![],
            pos: 0,
            schema: vec![],
        }
    }
}

impl PhysicalOperator for SeqScan {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        let table_name = normalize(&self.table);
        let schema = table_schema(state, &table_name)?.clone();
        let data = state
            .tables
            .get(&table_name)
            .ok_or_else(|| MiniDbError::sql(format!("missing table data for {table_name}")))?;
        self.schema = schema.qualified_columns();
        self.rows = data
            .rows
            .values()
            .map(|stored| row_from_values(&schema, &stored.values))
            .collect();
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self, _state: &mut DatabaseState) -> Result<Option<DataRow>> {
        let row = self.rows.get(self.pos).cloned();
        self.pos = self.pos.saturating_add(usize::from(row.is_some()));
        Ok(row)
    }

    fn close(&mut self) -> Result<()> {
        self.rows.clear();
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        self.schema.clone()
    }
}

pub struct IndexScan {
    table: String,
    index: String,
    op: RangeOp,
    value: Value,
    rows: Vec<DataRow>,
    pos: usize,
    schema: Vec<String>,
}

impl IndexScan {
    pub fn new(table: String, index: String, op: RangeOp, value: Value) -> Self {
        Self {
            table,
            index,
            op,
            value,
            rows: vec![],
            pos: 0,
            schema: vec![],
        }
    }
}

impl PhysicalOperator for IndexScan {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        let table_name = normalize(&self.table);
        let schema = table_schema(state, &table_name)?.clone();
        let index = state
            .indexes
            .get(&normalize(&self.index))
            .ok_or_else(|| MiniDbError::sql(format!("index {} not found", self.index)))?;
        let data = state
            .tables
            .get(&table_name)
            .ok_or_else(|| MiniDbError::sql(format!("missing table data for {table_name}")))?;
        self.schema = schema.qualified_columns();
        self.rows = index
            .scan(self.op, &self.value)
            .into_iter()
            .filter_map(|id| data.rows.get(&id))
            .map(|stored| row_from_values(&schema, &stored.values))
            .collect();
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self, _state: &mut DatabaseState) -> Result<Option<DataRow>> {
        let row = self.rows.get(self.pos).cloned();
        self.pos = self.pos.saturating_add(usize::from(row.is_some()));
        Ok(row)
    }

    fn close(&mut self) -> Result<()> {
        self.rows.clear();
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        self.schema.clone()
    }
}

pub struct Filter {
    child: Box<dyn PhysicalOperator>,
    predicate: Expr,
}

impl Filter {
    pub fn new(child: Box<dyn PhysicalOperator>, predicate: Expr) -> Self {
        Self { child, predicate }
    }
}

impl PhysicalOperator for Filter {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        self.child.open(state)
    }

    fn next(&mut self, state: &mut DatabaseState) -> Result<Option<DataRow>> {
        while let Some(row) = self.child.next(state)? {
            if eval_expr(&self.predicate, &row)?.truthy().accepts() {
                return Ok(Some(row));
            }
        }
        Ok(None)
    }

    fn close(&mut self) -> Result<()> {
        self.child.close()
    }

    fn schema(&self) -> Vec<String> {
        self.child.schema()
    }
}

pub struct Project {
    child: Box<dyn PhysicalOperator>,
    projection: Vec<SelectItem>,
    schema: Vec<String>,
}

impl Project {
    pub fn new(child: Box<dyn PhysicalOperator>, projection: Vec<SelectItem>) -> Self {
        Self {
            child,
            projection,
            schema: vec![],
        }
    }
}

impl PhysicalOperator for Project {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        self.child.open(state)?;
        self.schema = project_schema(&self.projection, &self.child.schema());
        Ok(())
    }

    fn next(&mut self, state: &mut DatabaseState) -> Result<Option<DataRow>> {
        let Some(row) = self.child.next(state)? else {
            return Ok(None);
        };
        let mut values = Vec::new();
        let mut names = Vec::new();
        for item in &self.projection {
            match item {
                SelectItem::Wildcard => {
                    for field in &row.fields {
                        values.push(field.value.clone());
                        names.push(display_field_name(field));
                    }
                }
                SelectItem::Expr { expr, alias } => {
                    values.push(eval_expr(expr, &row)?);
                    names.push(match alias {
                        Some(name) => name.clone(),
                        None => expr_name(expr),
                    });
                }
            }
        }
        Ok(Some(DataRow::values_with_names(&names, values)))
    }

    fn close(&mut self) -> Result<()> {
        self.child.close()
    }

    fn schema(&self) -> Vec<String> {
        self.schema.clone()
    }
}

pub struct NestedLoopJoin {
    left: Box<dyn PhysicalOperator>,
    right: Box<dyn PhysicalOperator>,
    on: Expr,
    rows: Vec<DataRow>,
    pos: usize,
    schema: Vec<String>,
}

impl NestedLoopJoin {
    pub fn new(
        left: Box<dyn PhysicalOperator>,
        right: Box<dyn PhysicalOperator>,
        on: Expr,
    ) -> Self {
        Self {
            left,
            right,
            on,
            rows: vec![],
            pos: 0,
            schema: vec![],
        }
    }
}

impl PhysicalOperator for NestedLoopJoin {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        self.left.open(state)?;
        let mut left_rows = Vec::new();
        while let Some(row) = self.left.next(state)? {
            left_rows.push(row);
        }
        self.right.open(state)?;
        let mut right_rows = Vec::new();
        while let Some(row) = self.right.next(state)? {
            right_rows.push(row);
        }
        self.schema = self.left.schema();
        self.schema.extend(self.right.schema());
        self.rows.clear();
        for left in &left_rows {
            for right in &right_rows {
                let mut fields = left.fields.clone();
                fields.extend(right.fields.clone());
                let combined = DataRow { fields };
                if eval_expr(&self.on, &combined)?.truthy().accepts() {
                    self.rows.push(combined);
                }
            }
        }
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self, _state: &mut DatabaseState) -> Result<Option<DataRow>> {
        let row = self.rows.get(self.pos).cloned();
        self.pos = self.pos.saturating_add(usize::from(row.is_some()));
        Ok(row)
    }

    fn close(&mut self) -> Result<()> {
        self.rows.clear();
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        self.schema.clone()
    }
}

pub struct Sort {
    child: Box<dyn PhysicalOperator>,
    order_by: Vec<OrderBy>,
    rows: Vec<DataRow>,
    pos: usize,
    schema: Vec<String>,
}

impl Sort {
    pub fn new(child: Box<dyn PhysicalOperator>, order_by: Vec<OrderBy>) -> Self {
        Self {
            child,
            order_by,
            rows: vec![],
            pos: 0,
            schema: vec![],
        }
    }
}

impl PhysicalOperator for Sort {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        self.child.open(state)?;
        self.schema = self.child.schema();
        self.rows.clear();
        while let Some(row) = self.child.next(state)? {
            self.rows.push(row);
        }
        let order = self.order_by.clone();
        self.rows.sort_by(|a, b| compare_rows(a, b, &order));
        self.pos = 0;
        Ok(())
    }

    fn next(&mut self, _state: &mut DatabaseState) -> Result<Option<DataRow>> {
        let row = self.rows.get(self.pos).cloned();
        self.pos = self.pos.saturating_add(usize::from(row.is_some()));
        Ok(row)
    }

    fn close(&mut self) -> Result<()> {
        self.rows.clear();
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        self.schema.clone()
    }
}

pub struct Limit {
    child: Box<dyn PhysicalOperator>,
    limit: u64,
    offset: u64,
    seen: u64,
    emitted: u64,
}

impl Limit {
    pub fn new(child: Box<dyn PhysicalOperator>, limit: u64, offset: u64) -> Self {
        Self {
            child,
            limit,
            offset,
            seen: 0,
            emitted: 0,
        }
    }
}

impl PhysicalOperator for Limit {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        self.seen = 0;
        self.emitted = 0;
        self.child.open(state)
    }

    fn next(&mut self, state: &mut DatabaseState) -> Result<Option<DataRow>> {
        while self.seen < self.offset {
            if self.child.next(state)?.is_none() {
                return Ok(None);
            }
            self.seen += 1;
        }
        if self.emitted >= self.limit {
            return Ok(None);
        }
        let row = self.child.next(state)?;
        if row.is_some() {
            self.emitted += 1;
        }
        Ok(row)
    }

    fn close(&mut self) -> Result<()> {
        self.child.close()
    }

    fn schema(&self) -> Vec<String> {
        self.child.schema()
    }
}

pub struct Insert {
    statement: Statement,
    emitted: bool,
    affected: usize,
}

impl Insert {
    pub fn new(statement: Statement) -> Self {
        Self {
            statement,
            emitted: false,
            affected: 0,
        }
    }
}

impl PhysicalOperator for Insert {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        self.affected = apply_insert_statement(state, &self.statement)?;
        self.emitted = false;
        Ok(())
    }

    fn next(&mut self, _state: &mut DatabaseState) -> Result<Option<DataRow>> {
        if self.emitted {
            Ok(None)
        } else {
            self.emitted = true;
            Ok(Some(DataRow::values_with_names(
                &[String::from("affected")],
                vec![Value::Int(self.affected as i64)],
            )))
        }
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        vec!["affected".into()]
    }
}

pub struct Update {
    statement: Statement,
    emitted: bool,
    affected: usize,
}

impl Update {
    pub fn new(statement: Statement) -> Self {
        Self {
            statement,
            emitted: false,
            affected: 0,
        }
    }
}

impl PhysicalOperator for Update {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        self.affected = apply_update_statement(state, &self.statement)?;
        self.emitted = false;
        Ok(())
    }

    fn next(&mut self, _state: &mut DatabaseState) -> Result<Option<DataRow>> {
        if self.emitted {
            Ok(None)
        } else {
            self.emitted = true;
            Ok(Some(DataRow::values_with_names(
                &[String::from("affected")],
                vec![Value::Int(self.affected as i64)],
            )))
        }
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        vec!["affected".into()]
    }
}

pub struct Delete {
    statement: Statement,
    emitted: bool,
    affected: usize,
}

impl Delete {
    pub fn new(statement: Statement) -> Self {
        Self {
            statement,
            emitted: false,
            affected: 0,
        }
    }
}

impl PhysicalOperator for Delete {
    fn open(&mut self, state: &mut DatabaseState) -> Result<()> {
        self.affected = apply_delete_statement(state, &self.statement)?;
        self.emitted = false;
        Ok(())
    }

    fn next(&mut self, _state: &mut DatabaseState) -> Result<Option<DataRow>> {
        if self.emitted {
            Ok(None)
        } else {
            self.emitted = true;
            Ok(Some(DataRow::values_with_names(
                &[String::from("affected")],
                vec![Value::Int(self.affected as i64)],
            )))
        }
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn schema(&self) -> Vec<String> {
        vec!["affected".into()]
    }
}

pub fn execute_statement(state: &mut DatabaseState, statement: Statement) -> Result<QueryOutput> {
    match statement {
        Statement::Select(select) => run_select(state, select),
        other => {
            let mutation = matches!(
                other,
                Statement::Insert { .. } | Statement::Update { .. } | Statement::Delete { .. }
            );
            let affected = apply_statement(state, other)?;
            Ok(QueryOutput {
                columns: vec![],
                rows: vec![],
                affected: Some(affected),
                mutation,
            })
        }
    }
}

pub fn apply_recovered_statement(state: &mut DatabaseState, statement: Statement) -> Result<()> {
    match statement {
        Statement::Begin | Statement::Commit | Statement::Rollback | Statement::Select(_) => Ok(()),
        other => {
            let _ = apply_statement(state, other)?;
            Ok(())
        }
    }
}

pub fn apply_statement(state: &mut DatabaseState, statement: Statement) -> Result<usize> {
    match statement {
        Statement::CreateTable {
            name,
            columns,
            checks,
        } => {
            let name = normalize(&name);
            if state.catalog.tables.contains_key(&name) {
                return Err(MiniDbError::sql(format!("table {name} already exists")));
            }
            let cols: Vec<Column> = columns.into_iter().map(Into::into).collect();
            let mut seen = std::collections::BTreeSet::new();
            for col in &cols {
                if !seen.insert(normalize(&col.name)) {
                    return Err(MiniDbError::sql(format!("duplicate column {}", col.name)));
                }
            }
            state.catalog.tables.insert(
                name.clone(),
                TableSchema {
                    name: name.clone(),
                    columns: cols,
                    checks,
                },
            );
            state.tables.insert(name, TableData::default());
            Ok(0)
        }
        Statement::DropTable { name } => {
            let name = normalize(&name);
            ensure_table_exists(state, &name)?;
            state.catalog.tables.remove(&name);
            state.tables.remove(&name);
            let indexes: Vec<String> = state
                .catalog
                .indexes
                .iter()
                .filter(|(_, idx)| idx.table == name)
                .map(|(n, _)| n.clone())
                .collect();
            for idx in indexes {
                state.catalog.indexes.remove(&idx);
                state.indexes.remove(&idx);
            }
            Ok(0)
        }
        Statement::CreateIndex {
            name,
            table,
            column,
        } => {
            let name = normalize(&name);
            let table = normalize(&table);
            let column = normalize(&column);
            if state.catalog.indexes.contains_key(&name) {
                return Err(MiniDbError::sql(format!("index {name} already exists")));
            }
            let schema = table_schema(state, &table)?.clone();
            if schema.column_index(&column).is_none() {
                return Err(MiniDbError::sql(format!("column {column} not found")));
            }
            state.catalog.indexes.insert(
                name.clone(),
                IndexSchema {
                    name: name.clone(),
                    table: table.clone(),
                    column: column.clone(),
                },
            );
            state
                .indexes
                .insert(name.clone(), BTreeIndex::new(name, table, column));
            rebuild_indexes(state)?;
            Ok(0)
        }
        Statement::DropIndex { name } => {
            let name = normalize(&name);
            if state.catalog.indexes.remove(&name).is_none() {
                return Err(MiniDbError::sql(format!("index {name} not found")));
            }
            state.indexes.remove(&name);
            Ok(0)
        }
        statement @ Statement::Insert { .. } => apply_insert_statement(state, &statement),
        statement @ Statement::Update { .. } => apply_update_statement(state, &statement),
        statement @ Statement::Delete { .. } => apply_delete_statement(state, &statement),
        Statement::Begin | Statement::Commit | Statement::Rollback | Statement::Select(_) => Ok(0),
    }
}

fn apply_insert_statement(state: &mut DatabaseState, statement: &Statement) -> Result<usize> {
    let Statement::Insert {
        table,
        columns,
        values,
    } = statement
    else {
        return Err(MiniDbError::sql("not an insert statement"));
    };
    let table = normalize(table);
    let schema = table_schema(state, &table)?.clone();
    let mut row = schema
        .columns
        .iter()
        .map(|c| c.default.clone())
        .collect::<Vec<_>>();
    if let Some(cols) = columns {
        if cols.len() != values.len() {
            return Err(MiniDbError::sql("column list and value list length differ"));
        }
        for (col, expr) in cols.iter().zip(values.iter()) {
            let idx = schema
                .column_index(col)
                .ok_or_else(|| MiniDbError::sql(format!("column {col} not found")))?;
            row[idx] = eval_expr(expr, &DataRow::empty())?.cast_to(schema.columns[idx].ty)?;
        }
    } else {
        if values.len() != schema.columns.len() {
            return Err(MiniDbError::sql(
                "positional insert must provide every column",
            ));
        }
        for (idx, expr) in values.iter().enumerate() {
            row[idx] = eval_expr(expr, &DataRow::empty())?.cast_to(schema.columns[idx].ty)?;
        }
    }
    validate_row(state, &schema, &row, None)?;
    state
        .tables
        .get_mut(&table)
        .ok_or_else(|| MiniDbError::sql(format!("table {table} not found")))?
        .insert(row);
    rebuild_indexes(state)?;
    Ok(1)
}

fn apply_update_statement(state: &mut DatabaseState, statement: &Statement) -> Result<usize> {
    let Statement::Update {
        table,
        assignments,
        selection,
    } = statement
    else {
        return Err(MiniDbError::sql("not an update statement"));
    };
    let table = normalize(table);
    let schema = table_schema(state, &table)?.clone();
    let matches = matching_row_ids(state, &schema, selection.as_ref())?;
    let mut replacements = Vec::new();
    for row_id in matches {
        let current = state
            .tables
            .get(&table)
            .and_then(|t| t.rows.get(&row_id))
            .ok_or_else(|| MiniDbError::sql("row disappeared during update"))?;
        let mut new_values = current.values.clone();
        let ctx = row_from_values(&schema, &current.values);
        for (col, expr) in assignments {
            let idx = schema
                .column_index(col)
                .ok_or_else(|| MiniDbError::sql(format!("column {col} not found")))?;
            new_values[idx] = eval_expr(expr, &ctx)?.cast_to(schema.columns[idx].ty)?;
        }
        validate_row(state, &schema, &new_values, Some(row_id))?;
        replacements.push((row_id, new_values));
    }
    let table_data = state
        .tables
        .get_mut(&table)
        .ok_or_else(|| MiniDbError::sql(format!("table {table} not found")))?;
    let count = replacements.len();
    for (row_id, values) in replacements {
        if let Some(row) = table_data.rows.get_mut(&row_id) {
            row.values = values;
        }
    }
    rebuild_indexes(state)?;
    Ok(count)
}

fn apply_delete_statement(state: &mut DatabaseState, statement: &Statement) -> Result<usize> {
    let Statement::Delete { table, selection } = statement else {
        return Err(MiniDbError::sql("not a delete statement"));
    };
    let table = normalize(table);
    let schema = table_schema(state, &table)?.clone();
    let matches = matching_row_ids(state, &schema, selection.as_ref())?;
    let table_data = state
        .tables
        .get_mut(&table)
        .ok_or_else(|| MiniDbError::sql(format!("table {table} not found")))?;
    let count = matches.len();
    for row_id in matches {
        table_data.delete(row_id);
    }
    rebuild_indexes(state)?;
    Ok(count)
}

pub fn run_select(state: &mut DatabaseState, select: Select) -> Result<QueryOutput> {
    let mut root: Box<dyn PhysicalOperator> = match &select.from {
        TableRef::Table(table) => build_single_table_scan(state, table, select.selection.as_ref())?,
        TableRef::Join { left, right, on } => Box::new(NestedLoopJoin::new(
            Box::new(SeqScan::new(normalize(left))),
            Box::new(SeqScan::new(normalize(right))),
            on.clone(),
        )),
    };
    if let Some(predicate) = select.selection.clone() {
        root = Box::new(Filter::new(root, predicate));
    }
    if !select.order_by.is_empty() {
        root = Box::new(Sort::new(root, select.order_by));
    }
    if let Some(limit) = select.limit {
        root = Box::new(Limit::new(root, limit.limit, limit.offset));
    }
    root = Box::new(Project::new(root, select.projection));
    root.open(state)?;
    let columns = root.schema();
    let mut rows = Vec::new();
    while let Some(row) = root.next(state)? {
        rows.push(row.fields.into_iter().map(|f| f.value).collect());
    }
    root.close()?;
    Ok(QueryOutput {
        columns,
        rows,
        affected: None,
        mutation: false,
    })
}

fn build_single_table_scan(
    state: &DatabaseState,
    table: &str,
    selection: Option<&Expr>,
) -> Result<Box<dyn PhysicalOperator>> {
    let table = normalize(table);
    ensure_table_exists(state, &table)?;
    if let Some((index, op, value)) =
        selection.and_then(|expr| usable_index_predicate(state, &table, expr))
    {
        Ok(Box::new(IndexScan::new(table, index, op, value)))
    } else {
        Ok(Box::new(SeqScan::new(table)))
    }
}

fn usable_index_predicate(
    state: &DatabaseState,
    table: &str,
    expr: &Expr,
) -> Option<(String, RangeOp, Value)> {
    let (col, op, value) = match expr {
        Expr::Binary { left, op, right } => match (&**left, &**right) {
            (Expr::Column { table: t, name }, Expr::Literal(v))
                if t.as_ref().is_none_or(|x| normalize(x) == table) =>
            {
                let range = range_op(*op)?;
                (normalize(name), range, v.clone())
            }
            (Expr::Literal(v), Expr::Column { table: t, name })
                if t.as_ref().is_none_or(|x| normalize(x) == table) =>
            {
                let range = reverse_range_op(*op)?;
                (normalize(name), range, v.clone())
            }
            _ => return None,
        },
        _ => return None,
    };
    state
        .catalog
        .indexes
        .iter()
        .find(|(_, idx)| idx.table == table && idx.column == col)
        .map(|(name, _)| (name.clone(), op, value))
}

fn range_op(op: BinaryOp) -> Option<RangeOp> {
    match op {
        BinaryOp::Eq => Some(RangeOp::Eq),
        BinaryOp::Lt => Some(RangeOp::Lt),
        BinaryOp::Le => Some(RangeOp::Le),
        BinaryOp::Gt => Some(RangeOp::Gt),
        BinaryOp::Ge => Some(RangeOp::Ge),
        _ => None,
    }
}

fn reverse_range_op(op: BinaryOp) -> Option<RangeOp> {
    match op {
        BinaryOp::Eq => Some(RangeOp::Eq),
        BinaryOp::Lt => Some(RangeOp::Gt),
        BinaryOp::Le => Some(RangeOp::Ge),
        BinaryOp::Gt => Some(RangeOp::Lt),
        BinaryOp::Ge => Some(RangeOp::Le),
        _ => None,
    }
}

pub fn eval_expr(expr: &Expr, row: &DataRow) -> Result<Value> {
    match expr {
        Expr::Literal(v) => Ok(v.clone()),
        Expr::Column { table, name } => lookup_column(row, table.as_deref(), name),
        Expr::Unary { op, expr } => {
            let value = eval_expr(expr, row)?;
            match op {
                UnaryOp::Not => Ok(Value::Bool(value.truthy().negate().accepts())),
                UnaryOp::Neg => match value {
                    Value::Null => Ok(Value::Null),
                    Value::Int(v) => Ok(Value::Int(-v)),
                    Value::Real(v) => Ok(Value::Real(-v)),
                    _ => Err(MiniDbError::Type(
                        "unary minus requires numeric value".into(),
                    )),
                },
            }
        }
        Expr::Binary { left, op, right } => {
            eval_binary(*op, eval_expr(left, row)?, eval_expr(right, row)?)
        }
        Expr::IsNull { expr, not } => {
            let is_null = eval_expr(expr, row)?.is_null();
            Ok(Value::Bool(if *not { !is_null } else { is_null }))
        }
        Expr::Function { name, args } => eval_function(name, args, row),
    }
}

fn eval_binary(op: BinaryOp, left: Value, right: Value) -> Result<Value> {
    match op {
        BinaryOp::And => Ok(Value::Bool(left.truthy().and(right.truthy()).accepts())),
        BinaryOp::Or => Ok(Value::Bool(left.truthy().or(right.truthy()).accepts())),
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            Ok(compare_values(op, &left, &right))
        }
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
            numeric_binary(op, left, right)
        }
        BinaryOp::Concat => {
            if left.is_null() || right.is_null() {
                Ok(Value::Null)
            } else {
                Ok(Value::Text(format!(
                    "{}{}",
                    left.display(),
                    right.display()
                )))
            }
        }
    }
}

fn compare_values(op: BinaryOp, left: &Value, right: &Value) -> Value {
    match left.compare(right) {
        TruthOrdering::Unknown => Value::Null,
        TruthOrdering::Known(ordering) => {
            let matched = match op {
                BinaryOp::Eq => ordering == Ordering::Equal,
                BinaryOp::Ne => ordering != Ordering::Equal,
                BinaryOp::Lt => ordering == Ordering::Less,
                BinaryOp::Le => ordering != Ordering::Greater,
                BinaryOp::Gt => ordering == Ordering::Greater,
                BinaryOp::Ge => ordering != Ordering::Less,
                _ => false,
            };
            Value::Bool(matched)
        }
    }
}

fn numeric_binary(op: BinaryOp, left: Value, right: Value) -> Result<Value> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    match (left, right) {
        (Value::Int(a), Value::Int(b)) if op != BinaryOp::Div => {
            let v = match op {
                BinaryOp::Add => a.saturating_add(b),
                BinaryOp::Sub => a.saturating_sub(b),
                BinaryOp::Mul => a.saturating_mul(b),
                _ => 0,
            };
            Ok(Value::Int(v))
        }
        (a, b) => {
            let av = numeric_as_f64(a)?;
            let bv = numeric_as_f64(b)?;
            if op == BinaryOp::Div && bv == 0.0 {
                return Err(MiniDbError::Type("division by zero".into()));
            }
            let v = match op {
                BinaryOp::Add => av + bv,
                BinaryOp::Sub => av - bv,
                BinaryOp::Mul => av * bv,
                BinaryOp::Div => av / bv,
                _ => av,
            };
            Ok(Value::Real(v))
        }
    }
}

fn numeric_as_f64(value: Value) -> Result<f64> {
    match value {
        Value::Int(v) => Ok(v as f64),
        Value::Real(v) => Ok(v),
        _ => Err(MiniDbError::Type("numeric expression required".into())),
    }
}

fn eval_function(name: &str, args: &[Expr], row: &DataRow) -> Result<Value> {
    let upper = name.to_ascii_uppercase();
    let values = args
        .iter()
        .map(|arg| eval_expr(arg, row))
        .collect::<Result<Vec<_>>>()?;
    match upper.as_str() {
        "LENGTH" => {
            ensure_arg_count(&upper, &values, 1)?;
            Ok(match &values[0] {
                Value::Null => Value::Null,
                Value::Text(s) => Value::Int(s.chars().count() as i64),
                v => Value::Int(v.display().chars().count() as i64),
            })
        }
        "LOWER" => {
            ensure_arg_count(&upper, &values, 1)?;
            Ok(match &values[0] {
                Value::Null => Value::Null,
                v => Value::Text(v.display().to_lowercase()),
            })
        }
        "UPPER" => {
            ensure_arg_count(&upper, &values, 1)?;
            Ok(match &values[0] {
                Value::Null => Value::Null,
                v => Value::Text(v.display().to_uppercase()),
            })
        }
        "SUBSTR" => {
            if values.len() != 2 && values.len() != 3 {
                return Err(MiniDbError::Type(
                    "SUBSTR expects two or three arguments".into(),
                ));
            }
            if values.iter().any(Value::is_null) {
                return Ok(Value::Null);
            }
            let s = values[0].display();
            let start = int_arg(&values[1])?.max(1) as usize - 1;
            let len = if values.len() == 3 {
                Some(int_arg(&values[2])?.max(0) as usize)
            } else {
                None
            };
            let chars = s.chars().skip(start);
            let out: String = match len {
                Some(n) => chars.take(n).collect(),
                None => chars.collect(),
            };
            Ok(Value::Text(out))
        }
        "ABS" => {
            ensure_arg_count(&upper, &values, 1)?;
            match &values[0] {
                Value::Null => Ok(Value::Null),
                Value::Int(v) => Ok(Value::Int(v.saturating_abs())),
                Value::Real(v) => Ok(Value::Real(v.abs())),
                _ => Err(MiniDbError::Type("ABS expects numeric value".into())),
            }
        }
        "COALESCE" => match values.into_iter().find(|v| !v.is_null()) {
            Some(value) => Ok(value),
            None => Ok(Value::Null),
        },
        _ => Err(MiniDbError::Type(format!("unknown function {name}"))),
    }
}

fn int_arg(value: &Value) -> Result<i64> {
    match value {
        Value::Int(v) => Ok(*v),
        _ => Err(MiniDbError::Type("integer argument required".into())),
    }
}

fn ensure_arg_count(name: &str, values: &[Value], expected: usize) -> Result<()> {
    if values.len() == expected {
        Ok(())
    } else {
        Err(MiniDbError::Type(format!(
            "{name} expects {expected} argument(s)"
        )))
    }
}

fn lookup_column(row: &DataRow, table: Option<&str>, name: &str) -> Result<Value> {
    let name = normalize(name);
    let table = table.map(normalize);
    let mut matches = row.fields.iter().filter(|field| {
        normalize(&field.name) == name
            && table
                .as_ref()
                .is_none_or(|t| field.table.as_ref().is_some_and(|ft| normalize(ft) == *t))
    });
    let first = matches.next();
    if matches.next().is_some() && table.is_none() {
        return Err(MiniDbError::sql(format!("ambiguous column {name}")));
    }
    first
        .map(|f| f.value.clone())
        .ok_or_else(|| MiniDbError::sql(format!("column {name} not found")))
}

fn validate_row(
    state: &DatabaseState,
    schema: &TableSchema,
    values: &[Value],
    replacing: Option<RowId>,
) -> Result<()> {
    for (idx, col) in schema.columns.iter().enumerate() {
        if (col.not_null || col.primary_key) && values[idx].is_null() {
            return Err(MiniDbError::sql(format!(
                "column {} cannot be NULL",
                col.name
            )));
        }
        if col.unique || col.primary_key {
            let data = state
                .tables
                .get(&schema.name)
                .ok_or_else(|| MiniDbError::sql(format!("table {} not found", schema.name)))?;
            for row in data.rows.values() {
                if Some(row.id) != replacing
                    && !values[idx].is_null()
                    && row.values[idx] == values[idx]
                {
                    return Err(MiniDbError::sql(format!(
                        "unique constraint failed: {}.{}",
                        schema.name, col.name
                    )));
                }
            }
        }
    }
    let ctx = row_from_values(schema, values);
    for check in &schema.checks {
        match eval_expr(check, &ctx)?.truthy() {
            Truth::False => return Err(MiniDbError::sql("check constraint failed")),
            Truth::True | Truth::Unknown => {}
        }
    }
    Ok(())
}

fn matching_row_ids(
    state: &DatabaseState,
    schema: &TableSchema,
    selection: Option<&Expr>,
) -> Result<Vec<RowId>> {
    let data = state
        .tables
        .get(&schema.name)
        .ok_or_else(|| MiniDbError::sql(format!("table {} not found", schema.name)))?;
    let mut out = Vec::new();
    for row in data.rows.values() {
        let ctx = row_from_values(schema, &row.values);
        let matched = match selection {
            Some(expr) => eval_expr(expr, &ctx)?.truthy().accepts(),
            None => true,
        };
        if matched {
            out.push(row.id);
        }
    }
    Ok(out)
}

fn rebuild_indexes(state: &mut DatabaseState) -> Result<()> {
    let metas = state.catalog.indexes.clone();
    for (name, meta) in metas {
        let schema = table_schema(state, &meta.table)?.clone();
        let col_idx = schema
            .column_index(&meta.column)
            .ok_or_else(|| MiniDbError::sql(format!("column {} not found", meta.column)))?;
        let data = state
            .tables
            .get(&meta.table)
            .ok_or_else(|| MiniDbError::sql(format!("table {} not found", meta.table)))?;
        let mut index = BTreeIndex::new(name.clone(), meta.table.clone(), meta.column.clone());
        for row in data.rows.values() {
            index.insert(&row.values[col_idx], row.id);
        }
        state.indexes.insert(name, index);
    }
    Ok(())
}

fn table_schema<'a>(state: &'a DatabaseState, table: &str) -> Result<&'a TableSchema> {
    state
        .catalog
        .tables
        .get(&normalize(table))
        .ok_or_else(|| MiniDbError::sql(format!("table {} not found", table)))
}

fn ensure_table_exists(state: &DatabaseState, table: &str) -> Result<()> {
    table_schema(state, table).map(|_| ())
}

fn row_from_values(schema: &TableSchema, values: &[Value]) -> DataRow {
    DataRow {
        fields: schema
            .columns
            .iter()
            .zip(values.iter())
            .map(|(col, value)| Field {
                table: Some(schema.name.clone()),
                name: col.name.clone(),
                value: value.clone(),
            })
            .collect(),
    }
}

fn project_schema(projection: &[SelectItem], child_schema: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for item in projection {
        match item {
            SelectItem::Wildcard => out.extend(child_schema.iter().cloned()),
            SelectItem::Expr { expr, alias } => out.push(match alias {
                Some(name) => name.clone(),
                None => expr_name(expr),
            }),
        }
    }
    out
}

fn expr_name(expr: &Expr) -> String {
    match expr {
        Expr::Column {
            table: Some(t),
            name,
        } => format!("{t}.{name}"),
        Expr::Column { table: None, name } => name.clone(),
        Expr::Function { name, .. } => name.to_ascii_lowercase(),
        _ => "expr".into(),
    }
}

fn display_field_name(field: &Field) -> String {
    match field.table.as_ref() {
        Some(table) => format!("{table}.{}", field.name),
        None => field.name.clone(),
    }
}

fn compare_rows(a: &DataRow, b: &DataRow, order_by: &[OrderBy]) -> Ordering {
    for item in order_by {
        let av = match eval_expr(&item.expr, a) {
            Ok(value) => value,
            Err(_) => Value::Null,
        };
        let bv = match eval_expr(&item.expr, b) {
            Ok(value) => value,
            Err(_) => Value::Null,
        };
        let mut ord = match av.compare(&bv) {
            TruthOrdering::Known(ord) => ord,
            TruthOrdering::Unknown => Ordering::Equal,
        };
        if !item.asc {
            ord = ord.reverse();
        }
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

fn normalize(s: &str) -> String {
    s.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_sql;

    #[test]
    fn executes_basic_crud() {
        let mut state = DatabaseState::default();
        for stmt in parse_sql(
            "create table t (id int primary key, name text); insert into t values (1, 'Ada');",
        )
        .unwrap()
        {
            execute_statement(&mut state, stmt).unwrap();
        }
        let stmt = parse_sql("select upper(name) from t where id = 1;")
            .unwrap()
            .remove(0);
        let out = execute_statement(&mut state, stmt).unwrap();
        assert_eq!(out.rows[0][0], Value::Text("ADA".into()));
    }
}
