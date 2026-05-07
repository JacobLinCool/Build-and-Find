pub mod logical;
pub mod physical;

use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::parser::ast::*;
use crate::plan::logical::*;
use crate::plan::physical::*;
use crate::types::{ColumnType, Value};

pub struct Planner<'a> {
    catalog: &'a Catalog,
}

impl<'a> Planner<'a> {
    pub fn new(catalog: &'a Catalog) -> Self {
        Self { catalog }
    }

    pub fn plan_select(&self, stmt: &SelectStmt) -> Result<PhysicalPlan> {
        let logical = self.build_logical_select(stmt)?;
        self.optimize_to_physical(logical)
    }

    fn build_logical_select(&self, stmt: &SelectStmt) -> Result<LogicalPlan> {
        let mut plan = if let Some(from) = &stmt.from {
            self.build_from(from)?
        } else {
            LogicalPlan::Values { rows: vec![vec![]] }
        };

        if let Some(pred) = &stmt.where_clause {
            plan = LogicalPlan::Filter {
                input: Box::new(plan),
                predicate: pred.clone(),
            };
        }

        if !stmt.order_by.as_ref().map_or(true, |o| o.is_empty()) {
            plan = LogicalPlan::Sort {
                input: Box::new(plan),
                keys: stmt.order_by.clone().unwrap(),
            };
        }

        if stmt.limit.is_some() || stmt.offset.is_some() {
            plan = LogicalPlan::Limit {
                input: Box::new(plan),
                limit: stmt.limit.clone(),
                offset: stmt.offset.clone(),
            };
        }

        plan = LogicalPlan::Project {
            input: Box::new(plan),
            items: stmt.projection.clone(),
        };

        Ok(plan)
    }

    fn build_from(&self, from: &FromClause) -> Result<LogicalPlan> {
        match from {
            FromClause::Table { name, alias } => {
                let table = self
                    .catalog
                    .get_table(name)
                    .ok_or_else(|| MiniDbError::TableNotFound(name.clone()))?;
                Ok(LogicalPlan::SeqScan {
                    table_name: name.clone(),
                    table_id: table.table_id,
                    root_page: table.root_page,
                    alias: alias.clone(),
                    schema: table.schema.clone(),
                })
            }
            FromClause::Join {
                left,
                right,
                on,
                join_type,
            } => {
                let left_plan = self.build_from(left)?;
                let right_plan = self.build_from(right)?;
                Ok(LogicalPlan::Join {
                    left: Box::new(left_plan),
                    right: Box::new(right_plan),
                    on: on.clone(),
                    join_type: *join_type,
                })
            }
        }
    }

    fn optimize_to_physical(&self, logical: LogicalPlan) -> Result<PhysicalPlan> {
        match logical {
            LogicalPlan::SeqScan {
                table_name,
                table_id,
                root_page,
                alias,
                schema,
            } => Ok(PhysicalPlan::SeqScan {
                table_name,
                table_id,
                root_page,
                alias,
                schema,
            }),
            LogicalPlan::IndexScan {
                table_name,
                table_id,
                root_page,
                alias,
                schema,
                index_id,
                index_root,
                key_expr,
            } => Ok(PhysicalPlan::IndexScan {
                table_name,
                table_id,
                root_page,
                alias,
                schema,
                index_id,
                index_root,
                key_expr,
            }),
            LogicalPlan::Filter { input, predicate } => {
                // Check if we can use an index scan
                if let LogicalPlan::SeqScan {
                    ref table_name,
                    ref schema,
                    table_id,
                    root_page,
                    ref alias,
                } = *input
                {
                    if let Some(idx_pred) = self.try_index_predicate(table_name, &predicate) {
                        let index = self
                            .catalog
                            .get_index_for_column(table_name, &idx_pred.column)
                            .unwrap();
                        return Ok(PhysicalPlan::IndexScan {
                            table_name: table_name.clone(),
                            table_id,
                            root_page,
                            alias: alias.clone(),
                            schema: schema.clone(),
                            index_id: index.index_id,
                            index_root: index.root_page,
                            key_expr: idx_pred.key_expr,
                        });
                    }
                }
                let input_phys = self.optimize_to_physical(*input)?;
                Ok(PhysicalPlan::Filter {
                    input: Box::new(input_phys),
                    predicate,
                })
            }
            LogicalPlan::Project { input, items } => {
                let input_phys = self.optimize_to_physical(*input)?;
                Ok(PhysicalPlan::Project {
                    input: Box::new(input_phys),
                    items,
                })
            }
            LogicalPlan::Join {
                left,
                right,
                on,
                join_type,
            } => {
                let left_phys = self.optimize_to_physical(*left)?;
                let right_phys = self.optimize_to_physical(*right)?;
                Ok(PhysicalPlan::NestedLoopJoin {
                    left: Box::new(left_phys),
                    right: Box::new(right_phys),
                    on,
                    join_type,
                })
            }
            LogicalPlan::Sort { input, keys } => {
                let input_phys = self.optimize_to_physical(*input)?;
                Ok(PhysicalPlan::Sort {
                    input: Box::new(input_phys),
                    keys,
                })
            }
            LogicalPlan::Limit {
                input,
                limit,
                offset,
            } => {
                let input_phys = self.optimize_to_physical(*input)?;
                Ok(PhysicalPlan::Limit {
                    input: Box::new(input_phys),
                    limit,
                    offset,
                })
            }
            LogicalPlan::Values { rows } => Ok(PhysicalPlan::Values { rows }),
        }
    }

    fn try_index_predicate(&self, table_name: &str, pred: &Expr) -> Option<IndexPredicate> {
        match pred {
            Expr::BinaryOp {
                left,
                op: BinaryOp::Eq,
                right,
            } => {
                match (left.as_ref(), right.as_ref()) {
                    (Expr::ColumnRef { table: _, name }, Expr::Literal(val)) => {
                        if let Some(idx) = self.catalog.get_index_for_column(table_name, name) {
                            return Some(IndexPredicate {
                                column: name.clone(),
                                key_expr: Expr::Literal(val.clone()),
                            });
                        }
                    }
                    (Expr::Literal(val), Expr::ColumnRef { table: _, name }) => {
                        if let Some(idx) = self.catalog.get_index_for_column(table_name, name) {
                            return Some(IndexPredicate {
                                column: name.clone(),
                                key_expr: Expr::Literal(val.clone()),
                            });
                        }
                    }
                    _ => {}
                }
                None
            }
            Expr::BinaryOp {
                left,
                op: BinaryOp::Gt | BinaryOp::Ge | BinaryOp::Lt | BinaryOp::Le,
                right,
            } => {
                if let Expr::ColumnRef { table: _, name } = left.as_ref() {
                    if let Expr::Literal(_) = right.as_ref() {
                        if self
                            .catalog
                            .get_index_for_column(table_name, name)
                            .is_some()
                        {
                            return Some(IndexPredicate {
                                column: name.clone(),
                                key_expr: (**right).clone(),
                            });
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
}

struct IndexPredicate {
    column: String,
    key_expr: Expr,
}
