pub mod logical;
pub mod physical;

use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;
use logical::LogicalPlan;
use physical::{PhysicalPlan, ScanOp};

pub struct Planner<'a> {
    catalog: &'a Catalog,
}

impl<'a> Planner<'a> {
    pub fn new(catalog: &'a Catalog) -> Self {
        Planner { catalog }
    }

    pub fn plan_statement(&self, stmt: Statement) -> Result<PhysicalPlan> {
        let logical = self.to_logical(stmt)?;
        self.to_physical(logical)
    }

    fn to_logical(&self, stmt: Statement) -> Result<LogicalPlan> {
        match stmt {
            Statement::Select(s) => self.plan_select(s),
            Statement::Insert(s) => Ok(LogicalPlan::Insert {
                table: s.table,
                columns: s.columns,
                values: s.values,
            }),
            Statement::Update(s) => Ok(LogicalPlan::Update {
                table: s.table.clone(),
                assignments: s.assignments,
                predicate: s.where_clause,
            }),
            Statement::Delete(s) => Ok(LogicalPlan::Delete {
                table: s.table,
                predicate: s.where_clause,
            }),
            Statement::CreateTable(s) => Ok(LogicalPlan::CreateTable { stmt: s }),
            Statement::DropTable(s) => Ok(LogicalPlan::DropTable {
                name: s.name,
                if_exists: s.if_exists,
            }),
            Statement::CreateIndex(s) => Ok(LogicalPlan::CreateIndex { stmt: s }),
            Statement::DropIndex(s) => Ok(LogicalPlan::DropIndex {
                name: s.name,
                if_exists: s.if_exists,
            }),
            Statement::Begin => Ok(LogicalPlan::Begin),
            Statement::Commit => Ok(LogicalPlan::Commit),
            Statement::Rollback => Ok(LogicalPlan::Rollback),
            Statement::MetaCommand(mc) => Ok(LogicalPlan::MetaCommand(mc)),
        }
    }

    fn plan_select(&self, s: SelectStmt) -> Result<LogicalPlan> {
        // Build scan/join base
        let base = match s.from {
            None => {
                // SELECT without FROM -- just project literals
                // Use a fake single-row input
                LogicalPlan::Scan {
                    table: "__dual__".into(),
                    alias: None,
                }
            }
            Some(FromClause::Table(tref)) => {
                if self.catalog.get_table(&tref.name).is_none() {
                    return Err(MiniDbError::NoSuchTable(tref.name.clone()));
                }
                LogicalPlan::Scan {
                    table: tref.name.clone(),
                    alias: tref.alias,
                }
            }
            Some(FromClause::Join {
                left,
                right,
                condition,
                ..
            }) => {
                if self.catalog.get_table(&left.name).is_none() {
                    return Err(MiniDbError::NoSuchTable(left.name.clone()));
                }
                if self.catalog.get_table(&right.name).is_none() {
                    return Err(MiniDbError::NoSuchTable(right.name.clone()));
                }
                let left_scan = LogicalPlan::Scan {
                    table: left.name.clone(),
                    alias: left.alias,
                };
                let right_scan = LogicalPlan::Scan {
                    table: right.name.clone(),
                    alias: right.alias,
                };
                LogicalPlan::Join {
                    left: Box::new(left_scan),
                    right: Box::new(right_scan),
                    condition,
                }
            }
        };

        // Apply WHERE
        let filtered = if let Some(pred) = s.where_clause {
            LogicalPlan::Filter {
                input: Box::new(base),
                predicate: pred,
            }
        } else {
            base
        };

        // Apply ORDER BY
        let sorted = if !s.order_by.is_empty() {
            LogicalPlan::Sort {
                input: Box::new(filtered),
                order_by: s.order_by,
            }
        } else {
            filtered
        };

        // Apply LIMIT/OFFSET
        let limited = if s.limit.is_some() || s.offset.is_some() {
            let limit = s
                .limit
                .map(|e| eval_const_expr(&e))
                .transpose()?
                .unwrap_or(usize::MAX);
            let offset = s
                .offset
                .map(|e| eval_const_expr(&e))
                .transpose()?
                .unwrap_or(0);
            LogicalPlan::Limit {
                input: Box::new(sorted),
                limit,
                offset,
            }
        } else {
            sorted
        };

        // Apply projection
        Ok(LogicalPlan::Project {
            input: Box::new(limited),
            projections: s.projections,
        })
    }

    fn to_physical(&self, plan: LogicalPlan) -> Result<PhysicalPlan> {
        match plan {
            LogicalPlan::Scan { table, alias } => Ok(PhysicalPlan::SeqScan { table, alias }),
            LogicalPlan::Filter { input, predicate } => {
                // Try to push down to IndexScan if applicable
                if let Some(index_plan) = self.try_index_scan(&input, &predicate)? {
                    return Ok(index_plan);
                }
                let phys_input = self.to_physical(*input)?;
                Ok(PhysicalPlan::Filter {
                    input: Box::new(phys_input),
                    predicate,
                })
            }
            LogicalPlan::Project { input, projections } => {
                let phys_input = self.to_physical(*input)?;
                Ok(PhysicalPlan::Project {
                    input: Box::new(phys_input),
                    projections,
                })
            }
            LogicalPlan::Join {
                left,
                right,
                condition,
            } => {
                let phys_left = self.to_physical(*left)?;
                let phys_right = self.to_physical(*right)?;
                Ok(PhysicalPlan::NestedLoopJoin {
                    left: Box::new(phys_left),
                    right: Box::new(phys_right),
                    condition,
                })
            }
            LogicalPlan::Sort { input, order_by } => {
                let phys_input = self.to_physical(*input)?;
                Ok(PhysicalPlan::Sort {
                    input: Box::new(phys_input),
                    order_by,
                })
            }
            LogicalPlan::Limit {
                input,
                limit,
                offset,
            } => {
                let phys_input = self.to_physical(*input)?;
                Ok(PhysicalPlan::Limit {
                    input: Box::new(phys_input),
                    limit,
                    offset,
                })
            }
            LogicalPlan::Insert {
                table,
                columns,
                values,
            } => Ok(PhysicalPlan::Insert {
                table,
                columns,
                values,
            }),
            LogicalPlan::Update {
                table,
                assignments,
                predicate,
            } => Ok(PhysicalPlan::Update {
                table,
                assignments,
                predicate,
            }),
            LogicalPlan::Delete { table, predicate } => {
                Ok(PhysicalPlan::Delete { table, predicate })
            }
            LogicalPlan::CreateTable { stmt } => Ok(PhysicalPlan::CreateTable { stmt }),
            LogicalPlan::DropTable { name, if_exists } => {
                Ok(PhysicalPlan::DropTable { name, if_exists })
            }
            LogicalPlan::CreateIndex { stmt } => Ok(PhysicalPlan::CreateIndex { stmt }),
            LogicalPlan::DropIndex { name, if_exists } => {
                Ok(PhysicalPlan::DropIndex { name, if_exists })
            }
            LogicalPlan::Begin => Ok(PhysicalPlan::Begin),
            LogicalPlan::Commit => Ok(PhysicalPlan::Commit),
            LogicalPlan::Rollback => Ok(PhysicalPlan::Rollback),
            LogicalPlan::MetaCommand(mc) => Ok(PhysicalPlan::MetaCommand(mc)),
        }
    }

    /// Try to produce an IndexScan for a filter over a scan, if possible.
    fn try_index_scan(
        &self,
        input: &LogicalPlan,
        predicate: &Expr,
    ) -> Result<Option<PhysicalPlan>> {
        let (table, alias) = match input {
            LogicalPlan::Scan { table, alias } => (table.clone(), alias.clone()),
            _ => return Ok(None),
        };

        // Look for predicates of the form: col OP literal or literal OP col
        if let Some((col, op, val)) = extract_simple_predicate(predicate) {
            if let Some(idx) = self.catalog.find_index(&table, &col) {
                return Ok(Some(PhysicalPlan::IndexScan {
                    table: table.clone(),
                    index: idx.name.clone(),
                    column: col.clone(),
                    op,
                    value: val,
                    alias,
                }));
            }
        }
        Ok(None)
    }
}

/// Try to extract a simple `col OP literal` predicate.
fn extract_simple_predicate(expr: &Expr) -> Option<(String, ScanOp, crate::types::Value)> {
    if let Expr::BinaryOp { left, op, right } = expr {
        let scan_op = match op {
            BinaryOp::Eq => ScanOp::Eq,
            BinaryOp::Lt => ScanOp::Lt,
            BinaryOp::Le => ScanOp::Le,
            BinaryOp::Gt => ScanOp::Gt,
            BinaryOp::Ge => ScanOp::Ge,
            _ => return None,
        };

        match (left.as_ref(), right.as_ref()) {
            (Expr::Column(col), Expr::Literal(v)) => {
                return Some((col.clone(), scan_op, v.clone()));
            }
            (Expr::Literal(v), Expr::Column(col)) => {
                // Flip the op
                let flipped = match scan_op {
                    ScanOp::Lt => ScanOp::Gt,
                    ScanOp::Le => ScanOp::Ge,
                    ScanOp::Gt => ScanOp::Lt,
                    ScanOp::Ge => ScanOp::Le,
                    ScanOp::Eq => ScanOp::Eq,
                };
                return Some((col.clone(), flipped, v.clone()));
            }
            _ => {}
        }
    }
    None
}

fn eval_const_expr(expr: &Expr) -> Result<usize> {
    match expr {
        Expr::Literal(crate::types::Value::Int(i)) => {
            if *i < 0 {
                Err(MiniDbError::InvalidArgument(
                    "LIMIT/OFFSET cannot be negative".into(),
                ))
            } else {
                Ok(*i as usize)
            }
        }
        _ => Err(MiniDbError::NotSupported(
            "LIMIT/OFFSET must be a literal integer".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Catalog, TableSchema};
    use crate::parser::parse;
    use crate::types::{ColumnDef, DataType};

    fn make_catalog() -> Catalog {
        let mut cat = Catalog::new();
        cat.create_table(TableSchema {
            name: "users".into(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: DataType::Int,
                    not_null: true,
                    primary_key: true,
                    unique: true,
                    default: None,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: DataType::Text,
                    not_null: false,
                    primary_key: false,
                    unique: false,
                    default: None,
                },
            ],
            primary_key: Some(vec!["id".into()]),
            check_constraints: vec![],
            heap_first_page: 0,
            heap_last_page: 0,
            next_row_seq: 0,
        })
        .unwrap();
        cat
    }

    #[test]
    fn test_plan_select() {
        let cat = make_catalog();
        let planner = Planner::new(&cat);
        let stmts = parse("SELECT id, name FROM users WHERE id = 1;").unwrap();
        let plan = planner
            .plan_statement(stmts.into_iter().next().unwrap())
            .unwrap();
        // Should produce Project(Filter(SeqScan)) or Project(IndexScan)
        match plan {
            PhysicalPlan::Project { .. } => {}
            _ => panic!("expected Project"),
        }
    }
}
