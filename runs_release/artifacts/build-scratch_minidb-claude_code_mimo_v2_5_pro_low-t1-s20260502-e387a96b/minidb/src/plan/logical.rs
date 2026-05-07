use crate::parser::ast::*;
use crate::types::TableSchema;

#[derive(Debug)]
pub enum LogicalPlan {
    SeqScan {
        table_name: String,
        table_id: u64,
        root_page: u64,
        alias: Option<String>,
        schema: TableSchema,
    },
    IndexScan {
        table_name: String,
        table_id: u64,
        root_page: u64,
        alias: Option<String>,
        schema: TableSchema,
        index_id: u64,
        index_root: u64,
        key_expr: Expr,
    },
    Filter {
        input: Box<LogicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<LogicalPlan>,
        items: Vec<SelectItem>,
    },
    Join {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        on: Option<Expr>,
        join_type: JoinType,
    },
    Sort {
        input: Box<LogicalPlan>,
        keys: Vec<OrderByItem>,
    },
    Limit {
        input: Box<LogicalPlan>,
        limit: Option<Expr>,
        offset: Option<Expr>,
    },
    Values {
        rows: Vec<Vec<Expr>>,
    },
}
