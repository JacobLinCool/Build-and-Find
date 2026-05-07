use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;

#[derive(Debug, Clone)]
pub enum LogicalPlan {
    SeqScan {
        table: String,
        alias: Option<String>,
        filter: Option<Expr>,
    },
    IndexScan {
        table: String,
        alias: Option<String>,
        index_name: String,
        key_expr: Expr,
    },
    Filter {
        input: Box<LogicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<LogicalPlan>,
        columns: Vec<ProjectedCol>,
    },
    NestedLoopJoin {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        on: Option<Expr>,
    },
    Sort {
        input: Box<LogicalPlan>,
        keys: Vec<SortKey>,
    },
    Limit {
        input: Box<LogicalPlan>,
        count: usize,
        offset: usize,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
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
pub struct ProjectedCol {
    pub expr: Expr,
    pub alias: Option<String>,
    pub table_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SortKey {
    pub expr: Expr,
    pub asc: bool,
}

pub struct Planner<'a> {
    catalog: &'a Catalog,
}

impl<'a> Planner<'a> {
    pub fn new(catalog: &'a Catalog) -> Self {
        Self { catalog }
    }

    pub fn plan(&self, stmt: &Statement) -> Result<LogicalPlan> {
        match stmt {
            Statement::Select(sel) => self.plan_select(sel),
            Statement::Insert {
                table,
                columns,
                values,
            } => Ok(LogicalPlan::Insert {
                table: table.clone(),
                columns: columns.clone(),
                values: values.clone(),
            }),
            Statement::Update {
                table,
                assignments,
                where_clause,
            } => Ok(LogicalPlan::Update {
                table: table.clone(),
                assignments: assignments.clone(),
                filter: where_clause.clone(),
            }),
            Statement::Delete {
                table,
                where_clause,
            } => Ok(LogicalPlan::Delete {
                table: table.clone(),
                filter: where_clause.clone(),
            }),
            _ => Err(MiniDbError::Runtime {
                message: "cannot plan this statement type".into(),
            }),
        }
    }

    fn plan_select(&self, sel: &SelectStmt) -> Result<LogicalPlan> {
        // Build the base plan from FROM clause
        let mut plan = match &sel.from {
            Some(from) => self.plan_from(from)?,
            None => {
                // SELECT without FROM (e.g., SELECT 1+1)
                // Create a dummy single-row scan
                return Ok(LogicalPlan::Project {
                    input: Box::new(LogicalPlan::SeqScan {
                        table: String::new(),
                        alias: None,
                        filter: None,
                    }),
                    columns: self.plan_projection(&sel.projection, None)?,
                });
            }
        };

        // Apply WHERE filter
        if let Some(predicate) = &sel.where_clause {
            // Try to optimize with index scan
            if let Some(optimized) = self.try_index_scan(&plan, predicate)? {
                plan = optimized;
            } else {
                plan = LogicalPlan::Filter {
                    input: Box::new(plan),
                    predicate: predicate.clone(),
                };
            }
        }

        // Apply ORDER BY
        if !sel.order_by.is_empty() {
            let keys: Vec<SortKey> = sel
                .order_by
                .iter()
                .map(|item| SortKey {
                    expr: item.expr.clone(),
                    asc: item.asc,
                })
                .collect();
            plan = LogicalPlan::Sort {
                input: Box::new(plan),
                keys,
            };
        }

        // Apply LIMIT/OFFSET
        if sel.limit.is_some() || sel.offset.is_some() {
            let count = match &sel.limit {
                Some(Expr::Literal(crate::types::Value::Int(n))) => *n as usize,
                _ => usize::MAX,
            };
            let offset = match &sel.offset {
                Some(Expr::Literal(crate::types::Value::Int(n))) => *n as usize,
                _ => 0,
            };
            plan = LogicalPlan::Limit {
                input: Box::new(plan),
                count,
                offset,
            };
        }

        // Apply projection
        let table_name = self.get_primary_table_name(&sel.from);
        plan = LogicalPlan::Project {
            input: Box::new(plan),
            columns: self.plan_projection(&sel.projection, table_name.as_deref())?,
        };

        Ok(plan)
    }

    fn plan_from(&self, from: &FromClause) -> Result<LogicalPlan> {
        match from {
            FromClause::Table { name, alias } => {
                // Validate table exists
                self.catalog.get_table(name)?;
                Ok(LogicalPlan::SeqScan {
                    table: name.clone(),
                    alias: alias.clone(),
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
        // Check if predicate is a simple equality or range on an indexed column
        if let LogicalPlan::SeqScan { table, alias, .. } = plan {
            if let Some(idx_info) = self.match_index_predicate(table, predicate) {
                return Ok(Some(LogicalPlan::IndexScan {
                    table: table.clone(),
                    alias: alias.clone(),
                    index_name: idx_info.0,
                    key_expr: idx_info.1,
                }));
            }
        }
        Ok(None)
    }

    fn match_index_predicate(&self, table_name: &str, predicate: &Expr) -> Option<(String, Expr)> {
        match predicate {
            Expr::BinaryOp {
                left,
                op: BinaryOp::Eq,
                right,
            } => {
                if let Expr::Column { table: None, name } = left.as_ref() {
                    if let Some(idx) = self.catalog.get_index_for_column(table_name, name) {
                        return Some((idx.name.clone(), right.as_ref().clone()));
                    }
                }
                if let Expr::Column { table: None, name } = right.as_ref() {
                    if let Some(idx) = self.catalog.get_index_for_column(table_name, name) {
                        return Some((idx.name.clone(), left.as_ref().clone()));
                    }
                }
                None
            }
            Expr::BinaryOp { left, op, right }
                if matches!(
                    op,
                    BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
                ) =>
            {
                if let Expr::Column { table: None, name } = left.as_ref() {
                    if let Some(idx) = self.catalog.get_index_for_column(table_name, name) {
                        return Some((idx.name.clone(), right.as_ref().clone()));
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn plan_projection(
        &self,
        items: &[SelectItem],
        default_table: Option<&str>,
    ) -> Result<Vec<ProjectedCol>> {
        let mut cols = Vec::new();
        for item in items {
            match item {
                SelectItem::Star => {
                    // Expand all columns from all tables
                    if let Some(tbl) = default_table {
                        if let Ok(table_meta) = self.catalog.get_table(tbl) {
                            for col in &table_meta.columns {
                                cols.push(ProjectedCol {
                                    expr: Expr::Column {
                                        table: Some(tbl.to_string()),
                                        name: col.name.clone(),
                                    },
                                    alias: None,
                                    table_ref: Some(tbl.to_string()),
                                });
                            }
                        }
                    }
                }
                SelectItem::QualifiedStar(table) => {
                    if let Ok(table_meta) = self.catalog.get_table(table) {
                        for col in &table_meta.columns {
                            cols.push(ProjectedCol {
                                expr: Expr::Column {
                                    table: Some(table.clone()),
                                    name: col.name.clone(),
                                },
                                alias: None,
                                table_ref: Some(table.clone()),
                            });
                        }
                    }
                }
                SelectItem::Expr { expr, alias } => {
                    cols.push(ProjectedCol {
                        expr: expr.clone(),
                        alias: alias.clone(),
                        table_ref: None,
                    });
                }
            }
        }
        Ok(cols)
    }

    fn get_primary_table_name(&self, from: &Option<FromClause>) -> Option<String> {
        match from {
            Some(FromClause::Table { name, .. }) => Some(name.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::page::PageManager;
    use crate::types::ColumnType;
    use tempfile::tempdir;

    fn setup_catalog() -> (PageManager, Catalog) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut pm = PageManager::open(&path).unwrap();
        let mut catalog = Catalog::load(&mut pm).unwrap();
        let cols = vec![
            crate::types::ColumnDef::new("id", ColumnType::Int),
            crate::types::ColumnDef::new("name", ColumnType::Text),
        ];
        catalog
            .create_table(&mut pm, "users".into(), cols, vec![])
            .unwrap();
        (pm, catalog)
    }

    #[test]
    fn test_plan_select_star() {
        let (mut pm, mut catalog) = setup_catalog();
        catalog
            .create_index(&mut pm, "idx_id".into(), "users", "id")
            .unwrap();
        let planner = Planner::new(&catalog);
        let stmt = Statement::Select(SelectStmt {
            projection: vec![SelectItem::Star],
            from: Some(FromClause::Table {
                name: "users".into(),
                alias: None,
            }),
            where_clause: Some(Expr::BinaryOp {
                left: Box::new(Expr::Column {
                    table: None,
                    name: "id".into(),
                }),
                op: BinaryOp::Eq,
                right: Box::new(Expr::Literal(crate::types::Value::Int(1))),
            }),
            order_by: vec![],
            limit: None,
            offset: None,
        });
        let plan = planner.plan(&stmt).unwrap();
        // Should use index scan since we have an index on id
        match &plan {
            LogicalPlan::Project { input, .. } => match input.as_ref() {
                LogicalPlan::IndexScan { .. } => {}
                other => panic!("expected IndexScan, got {:?}", other),
            },
            _ => panic!("expected Project"),
        }
    }
}
