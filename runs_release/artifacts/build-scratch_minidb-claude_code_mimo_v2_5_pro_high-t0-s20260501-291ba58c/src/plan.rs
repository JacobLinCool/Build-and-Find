use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;
use crate::types::{ColumnType, Value};

#[derive(Debug, Clone)]
pub enum LogicalPlan {
    SeqScan {
        table: String,
        filter: Option<Expr>,
    },
    IndexScan {
        table: String,
        index_name: String,
        key: Value,
        filter: Option<Expr>,
    },
    Filter {
        input: Box<LogicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<LogicalPlan>,
        projections: Vec<ResolvedProjection>,
    },
    NestedLoopJoin {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        on: Expr,
    },
    Sort {
        input: Box<LogicalPlan>,
        keys: Vec<(Expr, bool)>,
    },
    Limit {
        input: Box<LogicalPlan>,
        limit: usize,
        offset: usize,
    },
    Insert {
        table: String,
        columns: Vec<String>,
        values: Vec<Vec<Expr>>,
    },
    Update {
        table: String,
        assignments: Vec<(String, Expr)>,
        filter: Option<Expr>,
    },
    Delete {
        table: String,
        filter: Option<Expr>,
    },
}

#[derive(Debug, Clone)]
pub struct ResolvedProjection {
    pub expr: Expr,
    pub alias: String,
    pub col_type: Option<ColumnType>,
}

pub struct Planner<'a> {
    catalog: &'a Catalog,
}

impl<'a> Planner<'a> {
    pub fn new(catalog: &'a Catalog) -> Self {
        Self { catalog }
    }

    pub fn plan_select(&self, stmt: &SelectStmt) -> Result<LogicalPlan> {
        let mut plan = if let Some(from) = &stmt.from {
            self.plan_from(from)?
        } else {
            // SELECT without FROM (e.g., SELECT 1+1)
            return Ok(LogicalPlan::Project {
                input: Box::new(LogicalPlan::SeqScan {
                    table: String::new(),
                    filter: None,
                }),
                projections: self.resolve_projections(&stmt.projections, None)?,
            });
        };

        // Try to use index for simple equality/range predicates
        if let Some(predicate) = &stmt.where_clause {
            if let Some(indexed_plan) = self.try_index_scan(&plan, predicate)? {
                plan = indexed_plan;
            } else {
                plan = LogicalPlan::Filter {
                    input: Box::new(plan),
                    predicate: predicate.clone(),
                };
            }
        }

        // ORDER BY
        if let Some(order_by) = &stmt.order_by {
            let keys: Vec<(Expr, bool)> =
                order_by.iter().map(|o| (o.expr.clone(), o.asc)).collect();
            plan = LogicalPlan::Sort {
                input: Box::new(plan),
                keys,
            };
        }

        // LIMIT / OFFSET
        if let Some(limit_expr) = &stmt.limit {
            let limit = self.eval_const_expr(limit_expr)?;
            let offset = if let Some(offset_expr) = &stmt.offset {
                self.eval_const_expr(offset_expr)?
            } else {
                0
            };
            plan = LogicalPlan::Limit {
                input: Box::new(plan),
                limit,
                offset,
            };
        }

        // Projections
        let projections = self.resolve_projections(&stmt.projections, Some(&plan))?;
        plan = LogicalPlan::Project {
            input: Box::new(plan),
            projections,
        };

        Ok(plan)
    }

    fn plan_from(&self, from: &FromClause) -> Result<LogicalPlan> {
        match from {
            FromClause::Table { name, .. } => {
                let _ = self.catalog.get_table(name).ok_or_else(|| {
                    MiniDbError::Catalog(format!("table '{name}' does not exist"))
                })?;
                Ok(LogicalPlan::SeqScan {
                    table: name.clone(),
                    filter: None,
                })
            }
            FromClause::Join { left, right, on } => {
                let left_plan = self.plan_from(left)?;
                let right_plan = self.plan_from(right)?;
                Ok(LogicalPlan::NestedLoopJoin {
                    left: Box::new(left_plan),
                    right: Box::new(right_plan),
                    on: on.clone(),
                })
            }
        }
    }

    fn try_index_scan(&self, plan: &LogicalPlan, predicate: &Expr) -> Result<Option<LogicalPlan>> {
        let table_name = match plan {
            LogicalPlan::SeqScan { table, .. } => table.clone(),
            _ => return Ok(None),
        };

        // Check for simple equality: col = literal
        if let Expr::BinaryOp {
            left,
            op: BinaryOp::Eq,
            right,
        } = predicate
        {
            if let (Expr::ColumnRef { name, .. }, Expr::Literal(val)) =
                (left.as_ref(), right.as_ref())
            {
                let indexes = self.catalog.get_indexes_for_table(&table_name);
                for (idx_schema, _root_page) in indexes {
                    if idx_schema.column == *name {
                        return Ok(Some(LogicalPlan::IndexScan {
                            table: table_name,
                            index_name: idx_schema.name.clone(),
                            key: val.clone(),
                            filter: None,
                        }));
                    }
                }
            }
            // Also check reversed: literal = col
            if let (Expr::Literal(val), Expr::ColumnRef { name, .. }) =
                (left.as_ref(), right.as_ref())
            {
                let indexes = self.catalog.get_indexes_for_table(&table_name);
                for (idx_schema, _root_page) in indexes {
                    if idx_schema.column == *name {
                        return Ok(Some(LogicalPlan::IndexScan {
                            table: table_name,
                            index_name: idx_schema.name.clone(),
                            key: val.clone(),
                            filter: None,
                        }));
                    }
                }
            }
        }

        // Check for range: col > literal, col < literal, etc.
        if let Expr::BinaryOp { left, op: _, right } = predicate {
            if let (Expr::ColumnRef { name, .. }, Expr::Literal(val)) =
                (left.as_ref(), right.as_ref())
            {
                let indexes = self.catalog.get_indexes_for_table(&table_name);
                for (idx_schema, _root_page) in indexes {
                    if idx_schema.column == *name {
                        return Ok(Some(LogicalPlan::IndexScan {
                            table: table_name,
                            index_name: idx_schema.name.clone(),
                            key: val.clone(),
                            filter: Some(predicate.clone()),
                        }));
                    }
                }
            }
        }

        Ok(None)
    }

    fn resolve_projections(
        &self,
        projs: &[Projection],
        plan: Option<&LogicalPlan>,
    ) -> Result<Vec<ResolvedProjection>> {
        let mut resolved = Vec::new();
        for proj in projs {
            match proj {
                Projection::Star => {
                    // Expand * to all columns
                    if let Some(FromClause::Table { name, .. }) = self.get_plan_table(plan) {
                        if let Some(table) = self.catalog.get_table(&name) {
                            for col in &table.columns {
                                resolved.push(ResolvedProjection {
                                    expr: Expr::ColumnRef {
                                        table: Some(name.clone()),
                                        name: col.name.clone(),
                                    },
                                    alias: col.name.clone(),
                                    col_type: Some(col.col_type),
                                });
                            }
                        }
                    }
                }
                Projection::Column { table, name, alias } => {
                    resolved.push(ResolvedProjection {
                        expr: Expr::ColumnRef {
                            table: table.clone(),
                            name: name.clone(),
                        },
                        alias: alias.clone().unwrap_or_else(|| name.clone()),
                        col_type: None,
                    });
                }
                Projection::Expr { expr, alias } => {
                    resolved.push(ResolvedProjection {
                        expr: expr.clone(),
                        alias: alias.clone().unwrap_or_else(|| format!("{expr:?}")),
                        col_type: None,
                    });
                }
            }
        }
        Ok(resolved)
    }

    fn get_plan_table(&self, plan: Option<&LogicalPlan>) -> Option<FromClause> {
        match plan {
            Some(LogicalPlan::SeqScan { table, .. }) => Some(FromClause::Table {
                name: table.clone(),
                alias: None,
            }),
            Some(LogicalPlan::Filter { input, .. }) => self.get_plan_table(Some(input)),
            Some(LogicalPlan::Sort { input, .. }) => self.get_plan_table(Some(input)),
            Some(LogicalPlan::Limit { input, .. }) => self.get_plan_table(Some(input)),
            _ => None,
        }
    }

    fn eval_const_expr(&self, expr: &Expr) -> Result<usize> {
        match expr {
            Expr::Literal(Value::Int(i)) => Ok(*i as usize),
            _ => Err(MiniDbError::Syntax(
                "expected constant integer expression".to_string(),
            )),
        }
    }

    pub fn plan_insert(&self, stmt: &InsertStmt) -> Result<LogicalPlan> {
        let table = self.catalog.get_table(&stmt.table).ok_or_else(|| {
            MiniDbError::Catalog(format!("table '{}' does not exist", stmt.table))
        })?;
        let columns = if let Some(cols) = &stmt.columns {
            cols.clone()
        } else {
            table.columns.iter().map(|c| c.name.clone()).collect()
        };
        Ok(LogicalPlan::Insert {
            table: stmt.table.clone(),
            columns,
            values: stmt.values.clone(),
        })
    }

    pub fn plan_update(&self, stmt: &UpdateStmt) -> Result<LogicalPlan> {
        self.catalog.get_table(&stmt.table).ok_or_else(|| {
            MiniDbError::Catalog(format!("table '{}' does not exist", stmt.table))
        })?;
        Ok(LogicalPlan::Update {
            table: stmt.table.clone(),
            assignments: stmt.assignments.clone(),
            filter: stmt.where_clause.clone(),
        })
    }

    pub fn plan_delete(&self, stmt: &DeleteStmt) -> Result<LogicalPlan> {
        self.catalog.get_table(&stmt.table).ok_or_else(|| {
            MiniDbError::Catalog(format!("table '{}' does not exist", stmt.table))
        })?;
        Ok(LogicalPlan::Delete {
            table: stmt.table.clone(),
            filter: stmt.where_clause.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ColumnDef;

    fn test_catalog() -> Catalog {
        let mut cat = Catalog::default();
        cat.create_table(crate::types::TableSchema {
            name: "users".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".to_string(),
                    col_type: ColumnType::Int,
                    primary_key: true,
                    not_null: true,
                    unique: true,
                    default: None,
                },
                ColumnDef {
                    name: "name".to_string(),
                    col_type: ColumnType::Text,
                    primary_key: false,
                    not_null: false,
                    unique: false,
                    default: None,
                },
            ],
            check_expr: None,
        })
        .unwrap();
        cat
    }

    #[test]
    fn test_plan_simple_select() {
        let cat = test_catalog();
        let planner = Planner::new(&cat);
        let stmt = SelectStmt {
            projections: vec![Projection::Star],
            from: Some(FromClause::Table {
                name: "users".to_string(),
                alias: None,
            }),
            where_clause: None,
            order_by: None,
            limit: None,
            offset: None,
        };
        let plan = planner.plan_select(&stmt).unwrap();
        match plan {
            LogicalPlan::Project { .. } => {}
            _ => panic!("expected Project plan"),
        }
    }
}
