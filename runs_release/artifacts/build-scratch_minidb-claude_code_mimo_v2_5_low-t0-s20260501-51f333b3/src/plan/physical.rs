use crate::parser::ast::{Expr, OrderByExpr, SelectColumn};

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
        predicate: Expr,
    },
    Filter {
        input: Box<PhysicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<PhysicalPlan>,
        columns: Vec<SelectColumn>,
    },
    NestedLoopJoin {
        left: Box<PhysicalPlan>,
        right: Box<PhysicalPlan>,
    },
    Sort {
        input: Box<PhysicalPlan>,
        order_by: Vec<OrderByExpr>,
    },
    Limit {
        input: Box<PhysicalPlan>,
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

impl PhysicalPlan {
    pub fn pretty_print(&self, indent: usize) -> String {
        let prefix = "  ".repeat(indent);
        match self {
            PhysicalPlan::SeqScan { table, .. } => format!("{}SeqScan({})", prefix, table),
            PhysicalPlan::IndexScan {
                table, index_name, ..
            } => {
                format!("{}IndexScan({}, index: {})", prefix, table, index_name)
            }
            PhysicalPlan::Filter { input, predicate } => {
                format!(
                    "{}Filter({:?})\n{}",
                    prefix,
                    predicate,
                    input.pretty_print(indent + 1)
                )
            }
            PhysicalPlan::Project { input, columns } => {
                format!(
                    "{}Project({} cols)\n{}",
                    prefix,
                    columns.len(),
                    input.pretty_print(indent + 1)
                )
            }
            PhysicalPlan::NestedLoopJoin { left, right } => {
                format!(
                    "{}NestedLoopJoin\n{}\n{}",
                    prefix,
                    left.pretty_print(indent + 1),
                    right.pretty_print(indent + 1)
                )
            }
            PhysicalPlan::Sort { input, order_by } => {
                format!(
                    "{}Sort({} cols)\n{}",
                    prefix,
                    order_by.len(),
                    input.pretty_print(indent + 1)
                )
            }
            PhysicalPlan::Limit {
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
            PhysicalPlan::Insert { table, .. } => format!("{}Insert({})", prefix, table),
            PhysicalPlan::Update { table, .. } => format!("{}Update({})", prefix, table),
            PhysicalPlan::Delete { table, .. } => format!("{}Delete({})", prefix, table),
        }
    }
}
