use super::logical::{IndexPredicate, LogicalPlan};
use crate::catalog::Catalog;
use crate::error::Result;
use crate::parser::ast;

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
        predicate: IndexPredicate,
    },
    Filter {
        input: Box<PhysicalPlan>,
        predicate: ast::Expr,
    },
    Project {
        input: Box<PhysicalPlan>,
        expressions: Vec<ast::Expr>,
        distinct: bool,
    },
    Sort {
        input: Box<PhysicalPlan>,
        order_by: Vec<ast::OrderByItem>,
    },
    Limit {
        input: Box<PhysicalPlan>,
        limit: usize,
        offset: usize,
    },
    NestedLoopJoin {
        left: Box<PhysicalPlan>,
        right: Box<PhysicalPlan>,
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

pub trait PhysicalOperator: Send + Sync {
    fn open(&mut self) -> Result<()>;
    fn next(&mut self) -> Result<Option<Vec<crate::types::Value>>>;
    fn close(&mut self) -> Result<()>;
    fn schema(&self) -> Vec<(String, crate::types::ColumnType)>;
}

impl PhysicalPlan {
    pub fn from_logical(logical: LogicalPlan, _catalog: &Catalog) -> Result<Self> {
        Ok(match logical {
            LogicalPlan::SeqScan { table, alias } => PhysicalPlan::SeqScan { table, alias },
            LogicalPlan::IndexScan {
                table,
                index_name,
                column,
                predicate,
            } => PhysicalPlan::IndexScan {
                table,
                index_name,
                column,
                predicate,
            },
            LogicalPlan::Filter { input, predicate } => PhysicalPlan::Filter {
                input: Box::new(PhysicalPlan::from_logical(*input, _catalog)?),
                predicate,
            },
            LogicalPlan::Project {
                input,
                expressions,
                distinct,
            } => PhysicalPlan::Project {
                input: Box::new(PhysicalPlan::from_logical(*input, _catalog)?),
                expressions,
                distinct,
            },
            LogicalPlan::Sort { input, order_by } => PhysicalPlan::Sort {
                input: Box::new(PhysicalPlan::from_logical(*input, _catalog)?),
                order_by,
            },
            LogicalPlan::Limit {
                input,
                limit,
                offset,
            } => PhysicalPlan::Limit {
                input: Box::new(PhysicalPlan::from_logical(*input, _catalog)?),
                limit,
                offset,
            },
            LogicalPlan::NestedLoopJoin { left, right, on } => PhysicalPlan::NestedLoopJoin {
                left: Box::new(PhysicalPlan::from_logical(*left, _catalog)?),
                right: Box::new(PhysicalPlan::from_logical(*right, _catalog)?),
                on,
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
        })
    }
}
