use crate::parser::ast;
use crate::types::{IndexDef, Value};

/// Logical plan nodes
#[derive(Debug, Clone)]
pub enum LogicalPlan {
    Scan {
        table: String,
        alias: Option<String>,
    },
    IndexScan {
        table: String,
        index_name: String,
        column: String,
        value: Value,
        op: ScanOp,
    },
    Filter {
        input: Box<LogicalPlan>,
        predicate: ast::Expr,
    },
    Project {
        input: Box<LogicalPlan>,
        columns: Vec<ast::SelectColumn>,
    },
    Join {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        on: ast::Expr,
    },
    Sort {
        input: Box<LogicalPlan>,
        order_by: Vec<ast::OrderByExpr>,
    },
    Limit {
        input: Box<LogicalPlan>,
        limit: i64,
        offset: i64,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        values: Vec<Vec<ast::Expr>>,
    },
    Update {
        table: String,
        assignments: Vec<(String, ast::Expr)>,
        where_clause: Option<ast::Expr>,
    },
    Delete {
        table: String,
        where_clause: Option<ast::Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScanOp {
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Physical plan nodes
#[derive(Debug, Clone)]
pub enum PhysicalPlan {
    SeqScan {
        table: String,
        alias: Option<String>,
    },
    IndexScan {
        table: String,
        index_name: String,
        column: String,
        value: Value,
        op: ScanOp,
    },
    Filter {
        input: Box<PhysicalPlan>,
        predicate: ast::Expr,
    },
    Project {
        input: Box<PhysicalPlan>,
        columns: Vec<ast::SelectColumn>,
    },
    NestedLoopJoin {
        left: Box<PhysicalPlan>,
        right: Box<PhysicalPlan>,
        on: ast::Expr,
    },
    Sort {
        input: Box<PhysicalPlan>,
        order_by: Vec<ast::OrderByExpr>,
    },
    Limit {
        input: Box<PhysicalPlan>,
        limit: i64,
        offset: i64,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        values: Vec<Vec<ast::Expr>>,
    },
    Update {
        table: String,
        assignments: Vec<(String, ast::Expr)>,
        where_clause: Option<ast::Expr>,
    },
    Delete {
        table: String,
        where_clause: Option<ast::Expr>,
    },
}

/// Convert a logical plan to a physical plan
pub fn plan_to_physical(logical: LogicalPlan) -> PhysicalPlan {
    match logical {
        LogicalPlan::Scan { table, alias } => PhysicalPlan::SeqScan { table, alias },
        LogicalPlan::IndexScan {
            table,
            index_name,
            column,
            value,
            op,
        } => PhysicalPlan::IndexScan {
            table,
            index_name,
            column,
            value,
            op,
        },
        LogicalPlan::Filter { input, predicate } => PhysicalPlan::Filter {
            input: Box::new(plan_to_physical(*input)),
            predicate,
        },
        LogicalPlan::Project { input, columns } => PhysicalPlan::Project {
            input: Box::new(plan_to_physical(*input)),
            columns,
        },
        LogicalPlan::Join { left, right, on } => PhysicalPlan::NestedLoopJoin {
            left: Box::new(plan_to_physical(*left)),
            right: Box::new(plan_to_physical(*right)),
            on,
        },
        LogicalPlan::Sort { input, order_by } => PhysicalPlan::Sort {
            input: Box::new(plan_to_physical(*input)),
            order_by,
        },
        LogicalPlan::Limit {
            input,
            limit,
            offset,
        } => PhysicalPlan::Limit {
            input: Box::new(plan_to_physical(*input)),
            limit,
            offset,
        },
        LogicalPlan::Insert {
            table,
            columns,
            values,
        } => PhysicalPlan::Insert {
            table,
            columns,
            values,
        },
        LogicalPlan::Update {
            table,
            assignments,
            where_clause,
        } => PhysicalPlan::Update {
            table,
            assignments,
            where_clause,
        },
        LogicalPlan::Delete {
            table,
            where_clause,
        } => PhysicalPlan::Delete {
            table,
            where_clause,
        },
    }
}

/// Logical planner: converts AST to logical plan
pub struct LogicalPlanner {
    indexes: Vec<IndexDef>,
}

impl LogicalPlanner {
    pub fn new(indexes: Vec<IndexDef>) -> Self {
        LogicalPlanner { indexes }
    }

    pub fn plan(&self, stmt: ast::Statement) -> Result<LogicalPlan, String> {
        match stmt {
            ast::Statement::Select(sel) => self.plan_select(sel),
            ast::Statement::Insert(ins) => Ok(LogicalPlan::Insert {
                table: ins.table_name,
                columns: ins.columns,
                values: ins.values,
            }),
            ast::Statement::Update(upd) => Ok(LogicalPlan::Update {
                table: upd.table_name,
                assignments: upd.assignments,
                where_clause: upd.where_clause,
            }),
            ast::Statement::Delete(del) => Ok(LogicalPlan::Delete {
                table: del.table_name,
                where_clause: del.where_clause,
            }),
            _ => Err("Non-query statement in planner".into()),
        }
    }

    fn plan_select(&self, sel: ast::SelectStmt) -> Result<LogicalPlan, String> {
        let mut plan = self.plan_from(sel.from)?;

        // Add filter if WHERE clause exists
        if let Some(where_e) = sel.where_clause.clone() {
            // Check if we can use an index scan
            if let Some(index_plan) = self.try_index_scan(&plan, &where_e) {
                plan = index_plan;
            } else {
                plan = LogicalPlan::Filter {
                    input: Box::new(plan),
                    predicate: where_e,
                };
            }
        }

        // Add sort
        if !sel.order_by.is_empty() {
            plan = LogicalPlan::Sort {
                input: Box::new(plan),
                order_by: sel.order_by,
            };
        }

        // Add limit/offset
        if sel.limit.is_some() || sel.offset.is_some() {
            plan = LogicalPlan::Limit {
                input: Box::new(plan),
                limit: sel.limit.unwrap_or(i64::MAX),
                offset: sel.offset.unwrap_or(0),
            };
        }

        // Add projection
        plan = LogicalPlan::Project {
            input: Box::new(plan),
            columns: sel.columns,
        };

        Ok(plan)
    }

    fn plan_from(&self, from: ast::FromClause) -> Result<LogicalPlan, String> {
        match from {
            ast::FromClause::Table(name) => Ok(LogicalPlan::Scan {
                table: name,
                alias: None,
            }),
            ast::FromClause::Join { left, right, on } => {
                let left_plan = self.plan_from(*left)?;
                let right_plan = LogicalPlan::Scan {
                    table: right,
                    alias: None,
                };
                Ok(LogicalPlan::Join {
                    left: Box::new(left_plan),
                    right: Box::new(right_plan),
                    on,
                })
            }
        }
    }

    fn try_index_scan(&self, plan: &LogicalPlan, predicate: &ast::Expr) -> Option<LogicalPlan> {
        // Try to extract a simple equality or range predicate on an indexed column
        if let LogicalPlan::Scan { table, .. } = plan {
            match predicate {
                ast::Expr::BinaryOp { left, op, right } => {
                    if let (ast::Expr::Column(col), ast::Expr::Literal(val)) =
                        (left.as_ref(), right.as_ref())
                    {
                        let scan_op = match op {
                            ast::BinOp::Eq => ScanOp::Eq,
                            ast::BinOp::Lt => ScanOp::Lt,
                            ast::BinOp::Le => ScanOp::Le,
                            ast::BinOp::Gt => ScanOp::Gt,
                            ast::BinOp::Ge => ScanOp::Ge,
                            _ => return None,
                        };
                        // Find matching index
                        if let Some(idx) = self
                            .indexes
                            .iter()
                            .find(|i| i.table == *table && i.column == *col)
                        {
                            return Some(LogicalPlan::IndexScan {
                                table: table.clone(),
                                index_name: idx.name.clone(),
                                column: col.clone(),
                                value: val.clone(),
                                op: scan_op,
                            });
                        }
                    }
                    // Also check reversed: literal op column
                    if let (ast::Expr::Literal(val), ast::Expr::Column(col)) =
                        (left.as_ref(), right.as_ref())
                    {
                        let scan_op = match op {
                            ast::BinOp::Eq => ScanOp::Eq,
                            ast::BinOp::Lt => ScanOp::Gt,
                            ast::BinOp::Le => ScanOp::Ge,
                            ast::BinOp::Gt => ScanOp::Lt,
                            ast::BinOp::Ge => ScanOp::Le,
                            _ => return None,
                        };
                        if let Some(idx) = self
                            .indexes
                            .iter()
                            .find(|i| i.table == *table && i.column == *col)
                        {
                            return Some(LogicalPlan::IndexScan {
                                table: table.clone(),
                                index_name: idx.name.clone(),
                                column: col.clone(),
                                value: val.clone(),
                                op: scan_op,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    #[test]
    fn test_simple_select_plan() {
        let tokens = tokenize("SELECT * FROM users;").unwrap();
        let stmts = parse(tokens).unwrap();
        let planner = LogicalPlanner::new(vec![]);
        let plan = planner.plan(stmts.into_iter().next().unwrap()).unwrap();
        assert!(matches!(plan, LogicalPlan::Project { .. }));
    }

    #[test]
    fn test_select_with_where() {
        let tokens = tokenize("SELECT name FROM users WHERE id = 1;").unwrap();
        let stmts = parse(tokens).unwrap();
        let planner = LogicalPlanner::new(vec![]);
        let plan = planner.plan(stmts.into_iter().next().unwrap()).unwrap();
        match plan {
            LogicalPlan::Project { input, .. } => {
                assert!(matches!(*input, LogicalPlan::Filter { .. }));
            }
            _ => panic!("Expected Project"),
        }
    }

    #[test]
    fn test_index_scan_selection() {
        let tokens = tokenize("SELECT * FROM users WHERE id = 1;").unwrap();
        let stmts = parse(tokens).unwrap();
        let indexes = vec![IndexDef {
            name: "idx_id".into(),
            table: "users".into(),
            column: "id".into(),
        }];
        let planner = LogicalPlanner::new(indexes);
        let plan = planner.plan(stmts.into_iter().next().unwrap()).unwrap();
        match plan {
            LogicalPlan::Project { input, .. } => {
                assert!(matches!(*input, LogicalPlan::IndexScan { .. }));
            }
            _ => panic!("Expected Project with IndexScan"),
        }
    }
}
