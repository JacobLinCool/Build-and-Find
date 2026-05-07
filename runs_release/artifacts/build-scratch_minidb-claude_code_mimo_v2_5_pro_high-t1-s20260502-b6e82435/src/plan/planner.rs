use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::parser::ast;
use crate::plan::logical::*;
use crate::types::Schema;

pub struct Planner<'a> {
    catalog: &'a Catalog,
}

impl<'a> Planner<'a> {
    pub fn new(catalog: &'a Catalog) -> Self {
        Self { catalog }
    }

    pub fn plan(&self, stmt: &ast::Statement) -> Result<LogicalPlan> {
        match stmt {
            ast::Statement::Select(sel) => self.plan_select(sel),
            ast::Statement::Insert(ins) => self.plan_insert(ins),
            ast::Statement::Update(upd) => self.plan_update(upd),
            ast::Statement::Delete(del) => self.plan_delete(del),
            _ => Ok(LogicalPlan::Empty),
        }
    }

    fn plan_select(&self, sel: &ast::SelectStmt) -> Result<LogicalPlan> {
        let mut plan = if let Some(from) = &sel.from {
            self.plan_from(from)?
        } else {
            LogicalPlan::Empty
        };

        // WHERE
        if let Some(pred) = &sel.where_clause {
            let compiled = self.compile_expr(pred, &plan)?;
            plan = LogicalPlan::Filter {
                input: Box::new(plan),
                predicate: compiled,
            };
        }

        // ORDER BY
        if let Some(order) = &sel.order_by {
            let mut keys = Vec::new();
            for item in order {
                let compiled = self.compile_expr(&item.expr, &plan)?;
                keys.push((compiled, item.ascending));
            }
            plan = LogicalPlan::Sort {
                input: Box::new(plan),
                keys,
            };
        }

        // LIMIT / OFFSET
        if sel.limit.is_some() || sel.offset.is_some() {
            let count = sel
                .limit
                .as_ref()
                .map(|e| self.eval_const_expr(e))
                .transpose()?
                .map(|v| match v {
                    crate::types::Value::Int(i) => i as usize,
                    _ => usize::MAX,
                })
                .unwrap_or(usize::MAX);

            let offset = sel
                .offset
                .as_ref()
                .map(|e| self.eval_const_expr(e))
                .transpose()?
                .map(|v| match v {
                    crate::types::Value::Int(i) => i as usize,
                    _ => 0,
                })
                .unwrap_or(0);

            plan = LogicalPlan::Limit {
                input: Box::new(plan),
                count,
                offset,
            };
        }

        // Projection
        let _has_aggregate = sel.projection.iter().any(|item| {
            if let ast::SelectItem::Expr { expr, .. } = item {
                self.is_aggregate(expr)
            } else {
                false
            }
        });

        if !sel.projection.is_empty() && !is_select_star(&sel.projection) {
            let mut exprs = Vec::new();
            for item in &sel.projection {
                match item {
                    ast::SelectItem::Wildcard => {
                        // Expand * to all columns
                        let schema = self.plan_schema(&plan)?;
                        for col in &schema.columns {
                            exprs.push((
                                CompiledExpr::ColumnRef {
                                    table: None,
                                    name: col.name.clone(),
                                    index: None,
                                },
                                Some(col.name.clone()),
                            ));
                        }
                    }
                    ast::SelectItem::QualifiedWildcard(table) => {
                        let schema = self.plan_schema(&plan)?;
                        for col in &schema.columns {
                            exprs.push((
                                CompiledExpr::ColumnRef {
                                    table: Some(table.clone()),
                                    name: col.name.clone(),
                                    index: None,
                                },
                                Some(col.name.clone()),
                            ));
                        }
                    }
                    ast::SelectItem::Expr { expr, alias } => {
                        let compiled = self.compile_expr(expr, &plan)?;
                        let name = alias.clone().or_else(|| {
                            if let ast::Expr::ColumnRef { name, .. } = expr {
                                Some(name.clone())
                            } else {
                                None
                            }
                        });
                        exprs.push((compiled, name));
                    }
                }
            }
            plan = LogicalPlan::Project {
                input: Box::new(plan),
                exprs,
            };
        }

        Ok(plan)
    }

    fn extract_plan_table(plan: &LogicalPlan) -> Option<String> {
        match plan {
            LogicalPlan::SeqScan { table, .. } => Some(table.clone()),
            LogicalPlan::IndexScan { table, .. } => Some(table.clone()),
            LogicalPlan::Filter { input, .. } => Self::extract_plan_table(input),
            LogicalPlan::Sort { input, .. } => Self::extract_plan_table(input),
            LogicalPlan::Limit { input, .. } => Self::extract_plan_table(input),
            _ => None,
        }
    }

    pub fn is_aggregate_pub(&self, expr: &ast::Expr) -> bool {
        self.is_aggregate(expr)
    }

    fn is_aggregate(&self, expr: &ast::Expr) -> bool {
        match expr {
            ast::Expr::FunctionCall { name, .. } => {
                matches!(
                    name.to_uppercase().as_str(),
                    "COUNT" | "SUM" | "AVG" | "MIN" | "MAX"
                )
            }
            _ => false,
        }
    }

    fn plan_from(&self, from: &ast::FromClause) -> Result<LogicalPlan> {
        match from {
            ast::FromClause::Table { name, alias } => {
                let _table = self.catalog.get_table(name)?;

                Ok(LogicalPlan::SeqScan {
                    table: name.clone(),
                    alias: alias.clone(),
                    filter: None,
                })
            }
            ast::FromClause::Join {
                left,
                right,
                join_type,
                condition,
            } => {
                let left_plan = self.plan_from(left)?;
                let right_plan = self.plan_from(right)?;

                let jt = match join_type {
                    ast::JoinType::Inner => JoinType::Inner,
                    ast::JoinType::Left => JoinType::Left,
                    ast::JoinType::Right => JoinType::Right,
                    ast::JoinType::Cross => JoinType::Cross,
                };

                let combined = LogicalPlan::NestedLoopJoin {
                    left: Box::new(left_plan.clone()),
                    right: Box::new(right_plan.clone()),
                    condition: CompiledExpr::Literal(crate::types::Value::Bool(true)),
                    join_type: jt.clone(),
                };

                let compiled_cond = self.compile_expr(condition, &combined)?;
                Ok(LogicalPlan::NestedLoopJoin {
                    left: Box::new(left_plan),
                    right: Box::new(right_plan),
                    condition: compiled_cond,
                    join_type: jt,
                })
            }
        }
    }

    fn plan_insert(&self, ins: &ast::InsertStmt) -> Result<LogicalPlan> {
        let table = self.catalog.get_table(&ins.table)?;
        let schema = table.schema();

        let columns = if let Some(cols) = &ins.columns {
            cols.clone()
        } else {
            schema
                .column_names()
                .iter()
                .map(|s| s.to_string())
                .collect()
        };

        let mut compiled_rows = Vec::new();
        for row in &ins.values {
            let mut compiled_row = Vec::new();
            for expr in row {
                compiled_row.push(self.compile_expr_for_schema(expr, &schema)?);
            }
            compiled_rows.push(compiled_row);
        }

        Ok(LogicalPlan::Insert {
            table: ins.table.clone(),
            rows: compiled_rows,
            columns,
        })
    }

    fn plan_update(&self, upd: &ast::UpdateStmt) -> Result<LogicalPlan> {
        let table = self.catalog.get_table(&upd.table)?;
        let schema = table.schema();

        let mut assignments = Vec::new();
        for (col, expr) in &upd.assignments {
            let compiled = self.compile_expr_for_schema(expr, &schema)?;
            assignments.push((col.clone(), compiled));
        }

        let filter = if let Some(pred) = &upd.where_clause {
            Some(self.compile_expr_for_schema(pred, &schema)?)
        } else {
            None
        };

        Ok(LogicalPlan::Update {
            table: upd.table.clone(),
            assignments,
            filter,
        })
    }

    fn plan_delete(&self, del: &ast::DeleteStmt) -> Result<LogicalPlan> {
        let table = self.catalog.get_table(&del.table)?;
        let schema = table.schema();

        let filter = if let Some(pred) = &del.where_clause {
            Some(self.compile_expr_for_schema(pred, &schema)?)
        } else {
            None
        };

        Ok(LogicalPlan::Delete {
            table: del.table.clone(),
            filter,
        })
    }

    fn plan_schema(&self, plan: &LogicalPlan) -> Result<Schema> {
        match plan {
            LogicalPlan::SeqScan { table, .. } => self.catalog.get_schema(table),
            LogicalPlan::IndexScan { table, .. } => self.catalog.get_schema(table),
            LogicalPlan::Filter { input, .. } => self.plan_schema(input),
            LogicalPlan::Project { exprs, .. } => {
                let cols = exprs
                    .iter()
                    .map(|(_, alias)| {
                        let name = alias.clone().unwrap_or_else(|| "expr".to_string());
                        crate::types::ColumnDef {
                            name,
                            col_type: crate::types::ColumnType::Text,
                            constraints: vec![],
                        }
                    })
                    .collect();
                Ok(Schema::new(cols))
            }
            LogicalPlan::NestedLoopJoin { left, right, .. } => {
                let left_schema = self.plan_schema(left)?;
                let right_schema = self.plan_schema(right)?;
                let left_table = Self::extract_plan_table(left);
                let right_table = Self::extract_plan_table(right);
                let mut cols: Vec<crate::types::ColumnDef> = left_schema
                    .columns
                    .iter()
                    .map(|c| {
                        let name = if let Some(ref tbl) = left_table {
                            format!("{}.{}", tbl, c.name)
                        } else {
                            c.name.clone()
                        };
                        crate::types::ColumnDef {
                            name,
                            col_type: c.col_type,
                            constraints: vec![],
                        }
                    })
                    .collect();
                for c in &right_schema.columns {
                    let name = if let Some(ref tbl) = right_table {
                        format!("{}.{}", tbl, c.name)
                    } else {
                        c.name.clone()
                    };
                    cols.push(crate::types::ColumnDef {
                        name,
                        col_type: c.col_type,
                        constraints: vec![],
                    });
                }
                Ok(Schema::new(cols))
            }
            LogicalPlan::Sort { input, .. } => self.plan_schema(input),
            LogicalPlan::Limit { input, .. } => self.plan_schema(input),
            LogicalPlan::Empty => Ok(Schema::new(vec![])),
            LogicalPlan::Insert { .. }
            | LogicalPlan::Update { .. }
            | LogicalPlan::Delete { .. } => Ok(Schema::new(vec![])),
        }
    }

    fn compile_expr(&self, expr: &ast::Expr, plan: &LogicalPlan) -> Result<CompiledExpr> {
        let schema = self.plan_schema(plan)?;
        self.compile_expr_with_schema(expr, &schema)
    }

    pub fn compile_expr_for_schema(
        &self,
        expr: &ast::Expr,
        schema: &Schema,
    ) -> Result<CompiledExpr> {
        self.compile_expr_with_schema(expr, schema)
    }

    fn compile_expr_with_schema(&self, expr: &ast::Expr, schema: &Schema) -> Result<CompiledExpr> {
        match expr {
            ast::Expr::Literal(v) => Ok(CompiledExpr::Literal(v.clone())),
            ast::Expr::ColumnRef { table, name } => {
                // Try qualified name first (table.column), then unqualified
                let index = if let Some(tbl) = table {
                    schema
                        .column_index(&format!("{}.{}", tbl, name))
                        .or_else(|| schema.column_index(name))
                } else {
                    schema.column_index(name)
                };
                Ok(CompiledExpr::ColumnRef {
                    table: table.clone(),
                    name: name.clone(),
                    index,
                })
            }
            ast::Expr::BinaryOp { left, op, right } => Ok(CompiledExpr::BinaryOp {
                left: Box::new(self.compile_expr_with_schema(left, schema)?),
                op: *op,
                right: Box::new(self.compile_expr_with_schema(right, schema)?),
            }),
            ast::Expr::UnaryOp { op, expr } => Ok(CompiledExpr::UnaryOp {
                op: *op,
                expr: Box::new(self.compile_expr_with_schema(expr, schema)?),
            }),
            ast::Expr::FunctionCall { name, args } => {
                let compiled_args = args
                    .iter()
                    .map(|a| self.compile_expr_with_schema(a, schema))
                    .collect::<Result<Vec<_>>>()?;
                Ok(CompiledExpr::FunctionCall {
                    name: name.clone(),
                    args: compiled_args,
                })
            }
            ast::Expr::IsNull(expr) => Ok(CompiledExpr::IsNull(Box::new(
                self.compile_expr_with_schema(expr, schema)?,
            ))),
            ast::Expr::IsNotNull(expr) => Ok(CompiledExpr::IsNotNull(Box::new(
                self.compile_expr_with_schema(expr, schema)?,
            ))),
            ast::Expr::Coalesce(args) => {
                let compiled = args
                    .iter()
                    .map(|a| self.compile_expr_with_schema(a, schema))
                    .collect::<Result<Vec<_>>>()?;
                Ok(CompiledExpr::Coalesce(compiled))
            }
            ast::Expr::Case {
                when_clauses,
                else_clause,
            } => {
                let compiled_when = when_clauses
                    .iter()
                    .map(|(cond, result)| {
                        Ok((
                            self.compile_expr_with_schema(cond, schema)?,
                            self.compile_expr_with_schema(result, schema)?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let compiled_else = else_clause
                    .as_ref()
                    .map(|e| self.compile_expr_with_schema(e, schema).map(Box::new))
                    .transpose()?;
                Ok(CompiledExpr::Case {
                    when_clauses: compiled_when,
                    else_clause: compiled_else,
                })
            }
            ast::Expr::InList { expr, list } => {
                // Check if it's a subquery
                if list.len() == 1 && matches!(&list[0], ast::Expr::Subquery(_)) {
                    // Subquery - return literal false for now
                    Ok(CompiledExpr::Literal(crate::types::Value::Bool(false)))
                } else {
                    // Compile as: expr = list[0] OR expr = list[1] OR ...
                    let compiled_expr = self.compile_expr_with_schema(expr, schema)?;
                    let mut result: Option<CompiledExpr> = None;
                    for item in list {
                        let compiled_item = self.compile_expr_with_schema(item, schema)?;
                        let eq = CompiledExpr::BinaryOp {
                            left: Box::new(compiled_expr.clone()),
                            op: crate::parser::ast::BinaryOp::Eq,
                            right: Box::new(compiled_item),
                        };
                        result = Some(match result {
                            Some(prev) => CompiledExpr::BinaryOp {
                                left: Box::new(prev),
                                op: crate::parser::ast::BinaryOp::Or,
                                right: Box::new(eq),
                            },
                            None => eq,
                        });
                    }
                    Ok(result.unwrap_or(CompiledExpr::Literal(crate::types::Value::Bool(false))))
                }
            }
            ast::Expr::Subquery(_) => Ok(CompiledExpr::Literal(crate::types::Value::Null)),
        }
    }

    fn eval_const_expr(&self, expr: &ast::Expr) -> Result<crate::types::Value> {
        match expr {
            ast::Expr::Literal(v) => Ok(v.clone()),
            ast::Expr::UnaryOp {
                op: ast::UnaryOp::Neg,
                expr,
            } => {
                let val = self.eval_const_expr(expr)?;
                match val {
                    crate::types::Value::Int(i) => Ok(crate::types::Value::Int(-i)),
                    crate::types::Value::Real(r) => Ok(crate::types::Value::Real(-r)),
                    _ => Err(MiniDbError::Type("cannot negate non-numeric value".into())),
                }
            }
            _ => Err(MiniDbError::Type("not a constant expression".into())),
        }
    }

    pub fn find_index_scan(
        &self,
        table: &str,
        predicate: &ast::Expr,
    ) -> Option<(String, CompiledExpr)> {
        if let ast::Expr::BinaryOp { left, op: _, right } = predicate {
            let (col_name, value_expr) = match (left.as_ref(), right.as_ref()) {
                (ast::Expr::ColumnRef { name, .. }, expr) => (name.clone(), expr),
                (expr, ast::Expr::ColumnRef { name, .. }) => (name.clone(), expr),
                _ => return None,
            };

            if !matches!(value_expr, ast::Expr::Literal(_)) {
                return None;
            }

            let indexes = self.catalog.indexes_for_table(table);
            for idx in indexes {
                if idx.column_name == col_name {
                    let schema = self.catalog.get_schema(table).ok()?;
                    if let Ok(compiled) = self.compile_expr_with_schema(value_expr, &schema) {
                        return Some((idx.name.clone(), compiled));
                    }
                }
            }
        }
        None
    }

    pub fn optimize(&self, plan: LogicalPlan) -> Result<LogicalPlan> {
        match plan {
            LogicalPlan::Filter { input, predicate } => match *input {
                LogicalPlan::SeqScan {
                    table,
                    alias,
                    filter: None,
                } => {
                    if let Some((index_name, key)) = self.try_find_index(&table, &predicate) {
                        Ok(LogicalPlan::IndexScan {
                            table,
                            alias,
                            index_name,
                            key,
                        })
                    } else {
                        Ok(LogicalPlan::SeqScan {
                            table,
                            alias,
                            filter: Some(predicate),
                        })
                    }
                }
                other => Ok(LogicalPlan::Filter {
                    input: Box::new(other),
                    predicate,
                }),
            },
            other => Ok(other),
        }
    }

    fn try_find_index(
        &self,
        table: &str,
        predicate: &CompiledExpr,
    ) -> Option<(String, CompiledExpr)> {
        if let CompiledExpr::BinaryOp { left, op, right } = predicate {
            if matches!(
                op,
                crate::parser::ast::BinaryOp::Eq
                    | crate::parser::ast::BinaryOp::Lt
                    | crate::parser::ast::BinaryOp::Le
                    | crate::parser::ast::BinaryOp::Gt
                    | crate::parser::ast::BinaryOp::Ge
            ) {
                let (col_name, value_expr) = match (left.as_ref(), right.as_ref()) {
                    (CompiledExpr::ColumnRef { name, .. }, CompiledExpr::Literal(_)) => {
                        (name.clone(), right.as_ref().clone())
                    }
                    (CompiledExpr::Literal(_), CompiledExpr::ColumnRef { name, .. }) => {
                        (name.clone(), left.as_ref().clone())
                    }
                    _ => return None,
                };

                let indexes = self.catalog.indexes_for_table(table);
                for idx in indexes {
                    if idx.column_name == col_name {
                        return Some((idx.name.clone(), value_expr));
                    }
                }
            }
        }
        None
    }
}

fn is_select_star(items: &[ast::SelectItem]) -> bool {
    items.len() == 1 && matches!(items[0], ast::SelectItem::Wildcard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ColumnConstraint, ColumnDef, ColumnType, TableDef};

    fn test_catalog() -> Catalog {
        let mut catalog = Catalog::new();
        let table = TableDef {
            name: "users".into(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    col_type: ColumnType::Int,
                    constraints: vec![ColumnConstraint::PrimaryKey],
                },
                ColumnDef {
                    name: "name".into(),
                    col_type: ColumnType::Text,
                    constraints: vec![],
                },
            ],
            check_constraints: vec![],
        };
        catalog.create_table(table, 1).unwrap();
        catalog
    }

    #[test]
    fn test_plan_select() {
        let catalog = test_catalog();
        let planner = Planner::new(&catalog);

        let stmt = ast::Statement::Select(ast::SelectStmt {
            distinct: false,
            projection: vec![ast::SelectItem::Wildcard],
            from: Some(ast::FromClause::Table {
                name: "users".into(),
                alias: None,
            }),
            where_clause: None,
            group_by: None,
            having: None,
            order_by: None,
            limit: None,
            offset: None,
        });

        let plan = planner.plan(&stmt).unwrap();
        assert!(matches!(plan, LogicalPlan::SeqScan { .. }));
    }
}
