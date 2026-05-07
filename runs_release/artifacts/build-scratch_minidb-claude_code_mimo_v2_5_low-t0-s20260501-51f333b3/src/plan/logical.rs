use crate::parser::ast::{Expr, OrderByExpr, SelectColumn};

#[derive(Debug, Clone)]
pub enum LogicalPlan {
    Scan {
        table: String,
        alias: Option<String>,
    },
    Filter {
        input: Box<LogicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<LogicalPlan>,
        columns: Vec<SelectColumn>,
    },
    Join {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
    },
    Sort {
        input: Box<LogicalPlan>,
        order_by: Vec<OrderByExpr>,
    },
    Limit {
        input: Box<LogicalPlan>,
        limit: i64,
        offset: i64,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        values: Vec<Vec<Expr>>,
    },
    Update {
        table: String,
        set_clauses: Vec<(String, Expr)>,
        where_clause: Option<Expr>,
    },
    Delete {
        table: String,
        where_clause: Option<Expr>,
    },
}

impl LogicalPlan {
    pub fn pretty_print(&self, indent: usize) -> String {
        let prefix = "  ".repeat(indent);
        match self {
            LogicalPlan::Scan { table, alias } => {
                format!("{}Scan({})", prefix, alias.as_deref().unwrap_or(table))
            }
            LogicalPlan::Filter { input, predicate } => {
                format!(
                    "{}Filter({:?})\n{}",
                    prefix,
                    predicate,
                    input.pretty_print(indent + 1)
                )
            }
            LogicalPlan::Project { input, columns } => {
                format!(
                    "{}Project({} cols)\n{}",
                    prefix,
                    columns.len(),
                    input.pretty_print(indent + 1)
                )
            }
            LogicalPlan::Join { left, right } => {
                format!(
                    "{}Join\n{}\n{}",
                    prefix,
                    left.pretty_print(indent + 1),
                    right.pretty_print(indent + 1)
                )
            }
            LogicalPlan::Sort { input, order_by } => {
                format!(
                    "{}Sort({} cols)\n{}",
                    prefix,
                    order_by.len(),
                    input.pretty_print(indent + 1)
                )
            }
            LogicalPlan::Limit {
                input,
                limit,
                offset,
            } => {
                format!(
                    "{}Limit({} offset {})\n{}",
                    prefix,
                    limit,
                    offset,
                    input.pretty_print(indent + 1)
                )
            }
            LogicalPlan::Insert { table, .. } => format!("{}Insert({})", prefix, table),
            LogicalPlan::Update { table, .. } => format!("{}Update({})", prefix, table),
            LogicalPlan::Delete { table, .. } => format!("{}Delete({})", prefix, table),
        }
    }
}
