use crate::catalog::Catalog;
use crate::error::Result;
use crate::parser::ast::{FromClause, OrderByExpr, Program, SelectColumn, Statement};
use crate::plan::physical::PhysicalPlan;

pub struct PhysicalPlanner<'a> {
    catalog: &'a Catalog,
}

impl<'a> PhysicalPlanner<'a> {
    pub fn new(catalog: &'a Catalog) -> Self {
        PhysicalPlanner { catalog }
    }

    pub fn plan(&self, program: &Program) -> Result<PhysicalPlan> {
        match program {
            Program::Statement(stmt) => self.plan_statement(stmt),
            _ => Err(crate::error::MiniDbError::SqlError(
                "unsupported program type".to_string(),
            )),
        }
    }

    fn plan_statement(&self, stmt: &Statement) -> Result<PhysicalPlan> {
        match stmt {
            Statement::Select {
                columns,
                from,
                where_clause,
                order_by,
                limit,
                offset,
            } => self.plan_select(columns, from, where_clause, order_by, *limit, *offset),
            Statement::Insert {
                table,
                columns,
                values,
            } => Ok(PhysicalPlan::Insert {
                table: table.clone(),
                columns: columns.clone(),
                values: values.clone(),
            }),
            Statement::Update {
                table,
                set_clauses,
                where_clause,
            } => Ok(PhysicalPlan::Update {
                table: table.clone(),
                set_clauses: set_clauses.clone(),
                where_clause: where_clause.clone(),
            }),
            Statement::Delete {
                table,
                where_clause,
            } => Ok(PhysicalPlan::Delete {
                table: table.clone(),
                where_clause: where_clause.clone(),
            }),
            Statement::Begin | Statement::Commit | Statement::Rollback => {
                // These are handled at a higher level
                Err(crate::error::MiniDbError::SqlError(
                    "transaction statements handled elsewhere".to_string(),
                ))
            }
            Statement::CreateTable { .. }
            | Statement::DropTable { .. }
            | Statement::CreateIndex { .. }
            | Statement::DropIndex { .. } => Err(crate::error::MiniDbError::SqlError(
                "DDL statements handled elsewhere".to_string(),
            )),
        }
    }

    fn plan_select(
        &self,
        columns: &[SelectColumn],
        from: &FromClause,
        where_clause: &Option<crate::parser::ast::Expr>,
        order_by: &[OrderByExpr],
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<PhysicalPlan> {
        let mut plan = self.plan_from(from)?;

        // Add filter
        if let Some(predicate) = where_clause {
            plan = PhysicalPlan::Filter {
                input: Box::new(plan),
                predicate: predicate.clone(),
            };
        }

        // Add sort
        if !order_by.is_empty() {
            plan = PhysicalPlan::Sort {
                input: Box::new(plan),
                order_by: order_by.to_vec(),
            };
        }

        // Add limit/offset
        if limit.is_some() || offset.is_some() {
            plan = PhysicalPlan::Limit {
                input: Box::new(plan),
                limit: limit.unwrap_or(i64::MAX),
                offset: offset.unwrap_or(0),
            };
        }

        // Add projection
        plan = PhysicalPlan::Project {
            input: Box::new(plan),
            columns: columns.to_vec(),
        };

        Ok(plan)
    }

    fn plan_from(&self, from: &FromClause) -> Result<PhysicalPlan> {
        match from {
            FromClause::Table(table) => {
                // Check for index scan opportunity
                Ok(PhysicalPlan::SeqScan {
                    table: table.clone(),
                    alias: None,
                })
            }
            FromClause::Join { left, right, alias } => {
                let left_plan = self.plan_from(left)?;
                let right_plan = PhysicalPlan::SeqScan {
                    table: right.clone(),
                    alias: alias.clone(),
                };

                Ok(PhysicalPlan::NestedLoopJoin {
                    left: Box::new(left_plan),
                    right: Box::new(right_plan),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    #[test]
    fn test_plan_select() {
        let mut catalog = Catalog::new();
        let cols = vec![crate::parser::ast::ColumnDef {
            name: "id".to_string(),
            col_type: crate::types::ColumnType::Int,
            primary_key: true,
            not_null: true,
            unique: false,
            default: None,
            check: None,
        }];
        catalog.create_table("t".to_string(), cols).unwrap();

        let planner = PhysicalPlanner::new(&catalog);
        let tokens = tokenize("SELECT * FROM t").unwrap();
        let program = parse(tokens).unwrap();
        let plan = planner.plan(&program).unwrap();

        assert!(matches!(plan, PhysicalPlan::Project { .. }));
    }
}
