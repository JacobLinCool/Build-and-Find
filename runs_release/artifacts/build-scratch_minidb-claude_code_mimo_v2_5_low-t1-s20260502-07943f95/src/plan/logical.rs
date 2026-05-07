use crate::catalog::Catalog;
use crate::error::{MiniDbError, Result};
use crate::parser::ast;
use crate::storage::btree::IndexKey;
pub use crate::storage::btree::IndexPredicate;
use crate::types::Value;

#[derive(Debug, Clone)]
pub enum LogicalPlan {
    SeqScan {
        table: String,
        alias: Option<String>,
    },
    IndexScan {
        table: String,
        index_name: String,
        column: String,
        predicate: IndexPredicate,
    },
    Filter {
        input: Box<LogicalPlan>,
        predicate: ast::Expr,
    },
    Project {
        input: Box<LogicalPlan>,
        expressions: Vec<ast::Expr>,
        distinct: bool,
    },
    Sort {
        input: Box<LogicalPlan>,
        order_by: Vec<ast::OrderByItem>,
    },
    Limit {
        input: Box<LogicalPlan>,
        limit: usize,
        offset: usize,
    },
    NestedLoopJoin {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        on: ast::Expr,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        values: Vec<ast::Expr>,
    },
    Update {
        table: String,
        assignments: Vec<ast::Assignment>,
        where_clause: Option<ast::Expr>,
    },
    Delete {
        table: String,
        where_clause: Option<ast::Expr>,
    },
}

impl LogicalPlan {
    pub fn from_ast(stmt: &ast::Statement, catalog: &Catalog) -> Result<Self> {
        match stmt {
            ast::Statement::Select(sel) => Self::plan_select(sel, catalog),
            ast::Statement::Insert(ins) => Ok(LogicalPlan::Insert {
                table: ins.table.clone(),
                columns: ins.columns.clone(),
                values: ins.values.clone(),
            }),
            ast::Statement::Update(upd) => Ok(LogicalPlan::Update {
                table: upd.table.clone(),
                assignments: upd.assignments.clone(),
                where_clause: upd.where_clause.clone(),
            }),
            ast::Statement::Delete(del) => Ok(LogicalPlan::Delete {
                table: del.table.clone(),
                where_clause: del.where_clause.clone(),
            }),
            _ => Err(MiniDbError::Runtime(
                "Unsupported statement for planning".into(),
            )),
        }
    }

    fn plan_select(sel: &ast::Select, catalog: &Catalog) -> Result<Self> {
        // Start with seq scan or index scan
        let mut plan = match &sel.from {
            ast::FromClause::Table(table_ref) => {
                // Check if we can use an index scan
                if let Some(ref where_clause) = sel.where_clause {
                    if let Some((index_name, column, predicate)) =
                        Self::try_extract_index_predicate(where_clause, &table_ref.name, catalog)
                    {
                        LogicalPlan::IndexScan {
                            table: table_ref.name.clone(),
                            index_name,
                            column,
                            predicate,
                        }
                    } else {
                        LogicalPlan::SeqScan {
                            table: table_ref.name.clone(),
                            alias: table_ref.alias.clone(),
                        }
                    }
                } else {
                    LogicalPlan::SeqScan {
                        table: table_ref.name.clone(),
                        alias: table_ref.alias.clone(),
                    }
                }
            }
            ast::FromClause::Join(join) => {
                let left = LogicalPlan::SeqScan {
                    table: join.left_table.clone(),
                    alias: join.left_alias.clone(),
                };
                let right = LogicalPlan::SeqScan {
                    table: join.right_table.clone(),
                    alias: join.right_alias.clone(),
                };
                LogicalPlan::NestedLoopJoin {
                    left: Box::new(left),
                    right: Box::new(right),
                    on: join.on.clone(),
                }
            }
        };

        // Add filter if WHERE clause exists
        if let Some(ref where_clause) = sel.where_clause {
            // Only add filter if we're not already using an index scan
            if !matches!(plan, LogicalPlan::IndexScan { .. }) {
                plan = LogicalPlan::Filter {
                    input: Box::new(plan),
                    predicate: where_clause.clone(),
                };
            }
        }

        // Add sort if ORDER BY exists
        if let Some(ref order_by) = sel.order_by {
            plan = LogicalPlan::Sort {
                input: Box::new(plan),
                order_by: order_by.clone(),
            };
        }

        // Add limit/offset
        if sel.limit.is_some() || sel.offset.is_some() {
            plan = LogicalPlan::Limit {
                input: Box::new(plan),
                limit: sel.limit.unwrap_or(usize::MAX),
                offset: sel.offset.unwrap_or(0),
            };
        }

        // Add projection
        plan = LogicalPlan::Project {
            input: Box::new(plan),
            expressions: sel.projection.clone(),
            distinct: sel.distinct,
        };

        Ok(plan)
    }

    fn try_extract_index_predicate(
        expr: &ast::Expr,
        table: &str,
        catalog: &Catalog,
    ) -> Option<(String, String, IndexPredicate)> {
        match expr {
            ast::Expr::BinaryOp { op, left, right } => match op {
                ast::BinaryOp::Eq => {
                    if let (Some(col_name), Some(value)) =
                        (Self::extract_column(left), Self::extract_literal(right))
                    {
                        if let Some(idx_name) = catalog.find_index_for_column(table, &col_name) {
                            return Some((
                                idx_name.to_string(),
                                col_name,
                                IndexPredicate::Eq(IndexKey::from_value(&value)),
                            ));
                        }
                    }
                    if let (Some(value), Some(col_name)) =
                        (Self::extract_literal(left), Self::extract_column(right))
                    {
                        if let Some(idx_name) = catalog.find_index_for_column(table, &col_name) {
                            return Some((
                                idx_name.to_string(),
                                col_name,
                                IndexPredicate::Eq(IndexKey::from_value(&value)),
                            ));
                        }
                    }
                }
                ast::BinaryOp::Gt | ast::BinaryOp::Ge | ast::BinaryOp::Lt | ast::BinaryOp::Le => {
                    if let (Some(col_name), Some(value)) =
                        (Self::extract_column(left), Self::extract_literal(right))
                    {
                        if let Some(idx_name) = catalog.find_index_for_column(table, &col_name) {
                            let key = IndexKey::from_value(&value);
                            let (min, max, min_inc, max_inc) = match op {
                                ast::BinaryOp::Gt => (Some(key), None, false, false),
                                ast::BinaryOp::Ge => (Some(key), None, true, false),
                                ast::BinaryOp::Lt => (None, Some(key), false, false),
                                ast::BinaryOp::Le => (None, Some(key), false, true),
                                _ => unreachable!(),
                            };
                            return Some((
                                idx_name.to_string(),
                                col_name,
                                IndexPredicate::Range {
                                    min,
                                    max,
                                    min_inclusive: min_inc,
                                    max_inclusive: max_inc,
                                },
                            ));
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
        None
    }

    fn extract_column(expr: &ast::Expr) -> Option<String> {
        match expr {
            ast::Expr::ColumnRef {
                table: None,
                column,
            } => Some(column.clone()),
            ast::Expr::ColumnRef {
                table: Some(_),
                column,
            } => Some(column.clone()),
            _ => None,
        }
    }

    fn extract_literal(expr: &ast::Expr) -> Option<Value> {
        match expr {
            ast::Expr::Literal(v) => Some(v.clone()),
            _ => None,
        }
    }
}
