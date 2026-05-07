pub mod logical;
pub mod physical;

use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;
use crate::plan::logical::LogicalPlan;
use crate::plan::physical::{IndexPredicate, PhysicalPlan};
use crate::types::Value;

pub struct Planner<'a> {
    pub catalog: &'a Catalog,
}

impl<'a> Planner<'a> {
    pub fn new(catalog: &'a Catalog) -> Self {
        Planner { catalog }
    }

    pub fn plan_stmt(&self, stmt: &Stmt) -> Result<PhysicalPlan> {
        match stmt {
            Stmt::Select(s) => {
                let logical = self.plan_select_logical(s)?;
                self.logical_to_physical(logical)
            }
            Stmt::Insert(s) => Ok(PhysicalPlan::Insert {
                table: s.table.clone(),
                columns: s.columns.clone(),
                rows: s.values.clone(),
            }),
            Stmt::Update(s) => {
                let scan = if let Some(pred) = &s.where_clause {
                    let table_scan = self.choose_scan(&s.table, None, pred);
                    PhysicalPlan::Filter {
                        input: Box::new(table_scan),
                        predicate: pred.clone(),
                    }
                } else {
                    PhysicalPlan::SeqScan {
                        table: s.table.clone(),
                        alias: None,
                    }
                };
                Ok(PhysicalPlan::Update {
                    table: s.table.clone(),
                    input: Box::new(scan),
                    assignments: s.assignments.clone(),
                })
            }
            Stmt::Delete(s) => {
                let scan = if let Some(pred) = &s.where_clause {
                    let table_scan = self.choose_scan(&s.table, None, pred);
                    PhysicalPlan::Filter {
                        input: Box::new(table_scan),
                        predicate: pred.clone(),
                    }
                } else {
                    PhysicalPlan::SeqScan {
                        table: s.table.clone(),
                        alias: None,
                    }
                };
                Ok(PhysicalPlan::Delete {
                    table: s.table.clone(),
                    input: Box::new(scan),
                })
            }
            _ => Err(MiniDbError::Runtime(format!(
                "cannot plan statement: {:?}",
                stmt
            ))),
        }
    }

    fn plan_select_logical(&self, s: &SelectStmt) -> Result<LogicalPlan> {
        let mut plan = match &s.from {
            None => {
                // SELECT without FROM - constant expressions
                LogicalPlan::Scan {
                    table: "__dual__".to_string(),
                    alias: None,
                }
            }
            Some(from) => self.plan_from(from)?,
        };

        if let Some(pred) = &s.where_clause {
            plan = LogicalPlan::Filter {
                input: Box::new(plan),
                predicate: pred.clone(),
            };
        }

        // Build projection list
        let projections: Vec<(Expr, Option<String>)> = s
            .projections
            .iter()
            .map(|item| match item {
                SelectItem::Star => (Expr::Col("*".to_string(), None), None),
                SelectItem::TableStar(tbl) => (Expr::Col("*".to_string(), Some(tbl.clone())), None),
                SelectItem::Expr(e, alias) => (e.clone(), alias.clone()),
            })
            .collect();

        plan = LogicalPlan::Project {
            input: Box::new(plan),
            projections,
        };

        if !s.order_by.is_empty() {
            plan = LogicalPlan::Sort {
                input: Box::new(plan),
                keys: s.order_by.clone(),
            };
        }

        if s.limit.is_some() || s.offset.is_some() {
            let limit = match &s.limit {
                Some(Expr::Lit(Value::Int(n))) => *n as usize,
                Some(_) => usize::MAX,
                None => usize::MAX,
            };
            let offset = match &s.offset {
                Some(Expr::Lit(Value::Int(n))) => *n as usize,
                Some(_) => 0,
                None => 0,
            };
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
            FromClause::Table(name, alias) => Ok(LogicalPlan::Scan {
                table: name.clone(),
                alias: alias.clone(),
            }),
            FromClause::Join {
                left,
                right,
                condition,
            } => {
                let left_plan = self.plan_from(left)?;
                let right_plan = self.plan_from(right)?;
                Ok(LogicalPlan::Join {
                    left: Box::new(left_plan),
                    right: Box::new(right_plan),
                    condition: condition.clone(),
                })
            }
        }
    }

    fn logical_to_physical(&self, plan: LogicalPlan) -> Result<PhysicalPlan> {
        match plan {
            LogicalPlan::Scan { table, alias } => Ok(PhysicalPlan::SeqScan { table, alias }),
            LogicalPlan::Filter { input, predicate } => {
                // Try to push predicate into index scan
                if let LogicalPlan::Scan {
                    ref table,
                    ref alias,
                } = *input
                {
                    let table_name = table.clone();
                    let alias_clone = alias.clone();
                    let index_scan = self.try_index_scan(&table_name, &alias_clone, &predicate);
                    if let Some(iscan) = index_scan {
                        // We still apply the filter on top for correctness (may over-fetch)
                        return Ok(PhysicalPlan::Filter {
                            input: Box::new(iscan),
                            predicate,
                        });
                    }
                }
                let physical_input = self.logical_to_physical(*input)?;
                Ok(PhysicalPlan::Filter {
                    input: Box::new(physical_input),
                    predicate,
                })
            }
            LogicalPlan::Project { input, projections } => {
                let physical_input = self.logical_to_physical(*input)?;
                Ok(PhysicalPlan::Project {
                    input: Box::new(physical_input),
                    projections,
                })
            }
            LogicalPlan::Join {
                left,
                right,
                condition,
            } => {
                let left_phys = self.logical_to_physical(*left)?;
                let right_phys = self.logical_to_physical(*right)?;
                Ok(PhysicalPlan::NestedLoopJoin {
                    left: Box::new(left_phys),
                    right: Box::new(right_phys),
                    condition,
                })
            }
            LogicalPlan::Sort { input, keys } => {
                let physical_input = self.logical_to_physical(*input)?;
                Ok(PhysicalPlan::Sort {
                    input: Box::new(physical_input),
                    keys,
                })
            }
            LogicalPlan::Limit {
                input,
                limit,
                offset,
            } => {
                let physical_input = self.logical_to_physical(*input)?;
                Ok(PhysicalPlan::Limit {
                    input: Box::new(physical_input),
                    limit,
                    offset,
                })
            }
            LogicalPlan::Insert {
                table,
                columns,
                rows,
            } => Ok(PhysicalPlan::Insert {
                table,
                columns,
                rows,
            }),
            LogicalPlan::Update {
                table,
                assignments,
                predicate,
            } => {
                let scan = if let Some(pred) = predicate {
                    PhysicalPlan::Filter {
                        input: Box::new(PhysicalPlan::SeqScan {
                            table: table.clone(),
                            alias: None,
                        }),
                        predicate: pred,
                    }
                } else {
                    PhysicalPlan::SeqScan {
                        table: table.clone(),
                        alias: None,
                    }
                };
                Ok(PhysicalPlan::Update {
                    table,
                    input: Box::new(scan),
                    assignments,
                })
            }
            LogicalPlan::Delete { table, predicate } => {
                let scan = if let Some(pred) = predicate {
                    PhysicalPlan::Filter {
                        input: Box::new(PhysicalPlan::SeqScan {
                            table: table.clone(),
                            alias: None,
                        }),
                        predicate: pred,
                    }
                } else {
                    PhysicalPlan::SeqScan {
                        table: table.clone(),
                        alias: None,
                    }
                };
                Ok(PhysicalPlan::Delete {
                    table,
                    input: Box::new(scan),
                })
            }
        }
    }

    fn choose_scan(&self, table: &str, alias: Option<String>, predicate: &Expr) -> PhysicalPlan {
        if let Some(scan) = self.try_index_scan(table, &alias, predicate) {
            return scan;
        }
        PhysicalPlan::SeqScan {
            table: table.to_string(),
            alias,
        }
    }

    fn try_index_scan(
        &self,
        table: &str,
        alias: &Option<String>,
        predicate: &Expr,
    ) -> Option<PhysicalPlan> {
        let indexes = self.catalog.indexes_for_table(table);
        if indexes.is_empty() {
            return None;
        }

        for idx in &indexes {
            if let Some(ip) = extract_index_predicate(predicate, &idx.column, alias.as_deref()) {
                return Some(PhysicalPlan::IndexScan {
                    table: table.to_string(),
                    alias: alias.clone(),
                    index_name: idx.name.clone(),
                    predicate: ip,
                });
            }
        }
        None
    }
}

/// Try to extract an index predicate for the given column from the expression.
fn extract_index_predicate(
    expr: &Expr,
    col_name: &str,
    table_alias: Option<&str>,
) -> Option<IndexPredicate> {
    match expr {
        Expr::BinOp(left, op, right) => {
            let is_col_left = is_column(left, col_name, table_alias);
            let is_col_right = is_column(right, col_name, table_alias);

            if is_col_left && is_literal(right) {
                match op {
                    BinOp::Eq => return Some(IndexPredicate::Eq(*right.clone())),
                    BinOp::Lt => {
                        return Some(IndexPredicate::Range {
                            low: None,
                            low_inclusive: false,
                            high: Some(*right.clone()),
                            high_inclusive: false,
                        })
                    }
                    BinOp::Le => {
                        return Some(IndexPredicate::Range {
                            low: None,
                            low_inclusive: false,
                            high: Some(*right.clone()),
                            high_inclusive: true,
                        })
                    }
                    BinOp::Gt => {
                        return Some(IndexPredicate::Range {
                            low: Some(*right.clone()),
                            low_inclusive: false,
                            high: None,
                            high_inclusive: false,
                        })
                    }
                    BinOp::Ge => {
                        return Some(IndexPredicate::Range {
                            low: Some(*right.clone()),
                            low_inclusive: true,
                            high: None,
                            high_inclusive: false,
                        })
                    }
                    _ => {}
                }
            } else if is_col_right && is_literal(left) {
                // Reverse comparison
                match op {
                    BinOp::Eq => return Some(IndexPredicate::Eq(*left.clone())),
                    BinOp::Lt => {
                        return Some(IndexPredicate::Range {
                            low: Some(*left.clone()),
                            low_inclusive: false,
                            high: None,
                            high_inclusive: false,
                        })
                    }
                    BinOp::Le => {
                        return Some(IndexPredicate::Range {
                            low: Some(*left.clone()),
                            low_inclusive: true,
                            high: None,
                            high_inclusive: false,
                        })
                    }
                    BinOp::Gt => {
                        return Some(IndexPredicate::Range {
                            low: None,
                            low_inclusive: false,
                            high: Some(*left.clone()),
                            high_inclusive: false,
                        })
                    }
                    BinOp::Ge => {
                        return Some(IndexPredicate::Range {
                            low: None,
                            low_inclusive: false,
                            high: Some(*left.clone()),
                            high_inclusive: true,
                        })
                    }
                    _ => {}
                }
            }
            None
        }
        _ => None,
    }
}

fn is_column(expr: &Expr, col_name: &str, table_alias: Option<&str>) -> bool {
    match expr {
        Expr::Col(name, tbl) => {
            name.eq_ignore_ascii_case(col_name)
                && (tbl.is_none()
                    || table_alias.is_none()
                    || tbl
                        .as_deref()
                        .map(|t| t.eq_ignore_ascii_case(table_alias.unwrap()))
                        .unwrap_or(true))
        }
        _ => false,
    }
}

fn is_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Lit(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::parser::parse;

    fn make_catalog_with_table() -> Catalog {
        let mut cat = Catalog::new();
        let stmts = parse("CREATE TABLE users (id INT PRIMARY KEY, age INT, name TEXT);").unwrap();
        if let Stmt::CreateTable(ct) = &stmts[0] {
            cat.create_table(ct).unwrap();
        }
        cat
    }

    #[test]
    fn test_plan_simple_select() {
        let cat = make_catalog_with_table();
        let planner = Planner::new(&cat);
        let stmts = parse("SELECT * FROM users WHERE id = 1;").unwrap();
        let plan = planner.plan_stmt(&stmts[0]).unwrap();
        // Should produce Filter(SeqScan) or IndexScan depending on indexes
        assert!(matches!(plan, PhysicalPlan::Project { .. }));
    }

    #[test]
    fn test_plan_insert() {
        let cat = make_catalog_with_table();
        let planner = Planner::new(&cat);
        let stmts = parse("INSERT INTO users (id, name) VALUES (1, 'alice');").unwrap();
        let plan = planner.plan_stmt(&stmts[0]).unwrap();
        assert!(matches!(plan, PhysicalPlan::Insert { .. }));
    }
}
