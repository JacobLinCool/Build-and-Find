/// Logical and physical query planning
use crate::catalog::{Catalog, IndexMeta, TableSchema};
use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;
use crate::types::{ColumnDef, DataType, Value};

/// Logical plan node
#[derive(Debug, Clone)]
pub enum LogicalPlan {
    Scan {
        table: String,
    },
    IndexScan {
        table: String,
        index: String,
        column: String,
        predicate: IndexPredicate,
    },
    Filter {
        input: Box<LogicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<LogicalPlan>,
        columns: Vec<ProjectItem>,
    },
    Join {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        condition: Expr,
    },
    Sort {
        input: Box<LogicalPlan>,
        keys: Vec<SortKey>,
    },
    Limit {
        input: Box<LogicalPlan>,
        limit: usize,
        offset: usize,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        rows: Vec<Vec<Expr>>,
    },
    Update {
        table: String,
        assignments: Vec<(String, Expr)>,
        predicate: Option<Expr>,
    },
    Delete {
        table: String,
        predicate: Option<Expr>,
    },
    CreateTable(CreateTableStmt),
    DropTable(DropTableStmt),
    CreateIndex(CreateIndexStmt),
    DropIndex(DropIndexStmt),
    Values {
        rows: Vec<Vec<Expr>>,
    },
}

#[derive(Debug, Clone)]
pub enum IndexPredicate {
    Eq(Value),
    Lt(Value),
    Lte(Value),
    Gt(Value),
    Gte(Value),
    Range {
        low: Option<Value>,
        high: Option<Value>,
    },
}

#[derive(Debug, Clone)]
pub enum ProjectItem {
    Star,
    TableStar(String),
    Expr { expr: Expr, alias: Option<String> },
}

#[derive(Debug, Clone)]
pub struct SortKey {
    pub expr: Expr,
    pub asc: bool,
}

/// Output schema for a plan node
#[derive(Debug, Clone)]
pub struct Schema {
    pub columns: Vec<OutputColumn>,
}

#[derive(Debug, Clone)]
pub struct OutputColumn {
    pub name: String,
    pub table: Option<String>,
    pub dtype: Option<DataType>,
}

impl Schema {
    pub fn new(columns: Vec<OutputColumn>) -> Self {
        Schema { columns }
    }

    pub fn col_index(&self, name: &str) -> Option<usize> {
        // Prefer exact match, then case-insensitive
        self.columns
            .iter()
            .position(|c| c.name == name)
            .or_else(|| {
                self.columns
                    .iter()
                    .position(|c| c.name.eq_ignore_ascii_case(name))
            })
    }

    pub fn qualified_col_index(&self, table: &str, col: &str) -> Option<usize> {
        self.columns.iter().position(|c| {
            c.name.eq_ignore_ascii_case(col)
                && c.table
                    .as_deref()
                    .map(|t| t.eq_ignore_ascii_case(table))
                    .unwrap_or(true)
        })
    }
}

/// The planner converts AST statements into logical plans
pub struct Planner<'a> {
    catalog: &'a Catalog,
}

impl<'a> Planner<'a> {
    pub fn new(catalog: &'a Catalog) -> Self {
        Planner { catalog }
    }

    pub fn plan_statement(&self, stmt: &Statement) -> Result<LogicalPlan> {
        match stmt {
            Statement::Select(sel) => self.plan_select(sel),
            Statement::Insert(ins) => Ok(LogicalPlan::Insert {
                table: ins.table.clone(),
                columns: ins.columns.clone(),
                rows: ins.values.clone(),
            }),
            Statement::Update(upd) => Ok(LogicalPlan::Update {
                table: upd.table.clone(),
                assignments: upd.assignments.clone(),
                predicate: upd.where_clause.clone(),
            }),
            Statement::Delete(del) => Ok(LogicalPlan::Delete {
                table: del.table.clone(),
                predicate: del.where_clause.clone(),
            }),
            Statement::CreateTable(ct) => Ok(LogicalPlan::CreateTable(ct.clone())),
            Statement::DropTable(dt) => Ok(LogicalPlan::DropTable(dt.clone())),
            Statement::CreateIndex(ci) => Ok(LogicalPlan::CreateIndex(ci.clone())),
            Statement::DropIndex(di) => Ok(LogicalPlan::DropIndex(di.clone())),
            _ => Err(MiniDbError::Runtime(
                "statement does not produce a plan".into(),
            )),
        }
    }

    fn plan_select(&self, sel: &SelectStmt) -> Result<LogicalPlan> {
        let mut plan = if let Some(from) = &sel.from {
            self.plan_from(from)?
        } else {
            // SELECT without FROM: just values
            LogicalPlan::Values {
                rows: vec![sel
                    .columns
                    .iter()
                    .filter_map(|c| {
                        if let SelectItem::Expr { expr, .. } = c {
                            Some(expr.clone())
                        } else {
                            None
                        }
                    })
                    .collect()],
            }
        };

        // Apply WHERE
        if let Some(pred) = &sel.where_clause {
            plan = self.try_index_scan(plan, pred);
            // If we haven't switched to index scan, add a filter
            plan = match &plan {
                LogicalPlan::IndexScan { .. } => plan,
                _ => LogicalPlan::Filter {
                    input: Box::new(plan),
                    predicate: pred.clone(),
                },
            };
        }

        // Project
        let project_items: Vec<ProjectItem> = sel
            .columns
            .iter()
            .map(|c| match c {
                SelectItem::Star => ProjectItem::Star,
                SelectItem::TableStar(t) => ProjectItem::TableStar(t.clone()),
                SelectItem::Expr { expr, alias } => ProjectItem::Expr {
                    expr: expr.clone(),
                    alias: alias.clone(),
                },
            })
            .collect();
        plan = LogicalPlan::Project {
            input: Box::new(plan),
            columns: project_items,
        };

        // ORDER BY
        if !sel.order_by.is_empty() {
            let keys = sel
                .order_by
                .iter()
                .map(|o| SortKey {
                    expr: o.expr.clone(),
                    asc: o.asc,
                })
                .collect();
            plan = LogicalPlan::Sort {
                input: Box::new(plan),
                keys,
            };
        }

        // LIMIT / OFFSET
        if sel.limit.is_some() || sel.offset.is_some() {
            let limit = sel
                .limit
                .as_ref()
                .map(|e| eval_const_usize(e))
                .unwrap_or(usize::MAX);
            let offset = sel
                .offset
                .as_ref()
                .map(|e| eval_const_usize(e))
                .unwrap_or(0);
            plan = LogicalPlan::Limit {
                input: Box::new(plan),
                limit,
                offset,
            };
        }

        Ok(plan)
    }

    fn plan_from(&self, from: &FromClause) -> Result<LogicalPlan> {
        match from {
            FromClause::Table { name, .. } => {
                // Validate table exists
                self.catalog.get_table(name)?;
                Ok(LogicalPlan::Scan {
                    table: name.clone(),
                })
            }
            FromClause::Join { left, right, on } => {
                let l = self.plan_from(left)?;
                let r = self.plan_from(right)?;
                Ok(LogicalPlan::Join {
                    left: Box::new(l),
                    right: Box::new(r),
                    condition: on.clone(),
                })
            }
        }
    }

    fn try_index_scan(&self, plan: LogicalPlan, pred: &Expr) -> LogicalPlan {
        // Only applies to a simple Scan node
        let table = if let LogicalPlan::Scan { table } = &plan {
            table.clone()
        } else {
            return plan;
        };

        // Get indexes for this table
        let indexes = self.catalog.indexes_for_table(&table);
        if indexes.is_empty() {
            return plan;
        }

        // Try to find an equality/range predicate on an indexed column
        for idx in &indexes {
            if let Some(ip) = extract_index_predicate(pred, &idx.column) {
                return LogicalPlan::IndexScan {
                    table: table.clone(),
                    index: idx.name.clone(),
                    column: idx.column.clone(),
                    predicate: ip,
                };
            }
        }
        plan
    }
}

fn extract_index_predicate(expr: &Expr, col: &str) -> Option<IndexPredicate> {
    match expr {
        Expr::BinOp { op, left, right } => {
            let (col_side, val_side, flipped) = if is_col(left, col) {
                (left.as_ref(), right.as_ref(), false)
            } else if is_col(right, col) {
                (right.as_ref(), left.as_ref(), true)
            } else {
                return None;
            };
            let val = eval_const_value(val_side)?;
            let ip = match (op, flipped) {
                (BinOp::Eq, _) => IndexPredicate::Eq(val),
                (BinOp::Lt, false) => IndexPredicate::Lt(val),
                (BinOp::Lte, false) => IndexPredicate::Lte(val),
                (BinOp::Gt, false) => IndexPredicate::Gt(val),
                (BinOp::Gte, false) => IndexPredicate::Gte(val),
                (BinOp::Lt, true) => IndexPredicate::Gt(val),
                (BinOp::Lte, true) => IndexPredicate::Gte(val),
                (BinOp::Gt, true) => IndexPredicate::Lt(val),
                (BinOp::Gte, true) => IndexPredicate::Lte(val),
                _ => return None,
            };
            Some(ip)
        }
        _ => None,
    }
}

fn is_col(expr: &Expr, col: &str) -> bool {
    match expr {
        Expr::Column(c) => c.eq_ignore_ascii_case(col),
        Expr::QualifiedColumn(_, c) => c.eq_ignore_ascii_case(col),
        _ => false,
    }
}

fn eval_const_value(expr: &Expr) -> Option<Value> {
    match expr {
        Expr::Literal(v) => Some(v.clone()),
        Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr,
        } => {
            if let Some(Value::Int(n)) = eval_const_value(expr) {
                Some(Value::Int(-n))
            } else if let Some(Value::Real(f)) = eval_const_value(expr) {
                Some(Value::Real(-f))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn eval_const_usize(expr: &Expr) -> usize {
    match expr {
        Expr::Literal(Value::Int(n)) => *n as usize,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::types::ColumnDef;

    fn make_catalog() -> Catalog {
        let mut cat = Catalog::new();
        cat.create_table(crate::catalog::TableSchema::new(
            "users",
            vec![
                ColumnDef::new("id", DataType::Int),
                ColumnDef::new("name", DataType::Text),
            ],
        ))
        .unwrap();
        cat
    }

    #[test]
    fn test_plan_select() {
        let cat = make_catalog();
        let planner = Planner::new(&cat);
        let stmts =
            crate::parser::Parser::parse("SELECT id, name FROM users WHERE id = 1;").unwrap();
        let plan = planner.plan_statement(&stmts[0]).unwrap();
        matches!(plan, LogicalPlan::Project { .. });
    }

    #[test]
    fn test_plan_insert() {
        let cat = make_catalog();
        let planner = Planner::new(&cat);
        let stmts = crate::parser::Parser::parse("INSERT INTO users VALUES (1, 'alice');").unwrap();
        let plan = planner.plan_statement(&stmts[0]).unwrap();
        matches!(plan, LogicalPlan::Insert { .. });
    }
}
