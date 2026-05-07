use crate::parser::ast::*;
use crate::types::TableSchema;

#[derive(Debug, Clone)]
pub enum PhysicalPlan {
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
        input: Box<PhysicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<PhysicalPlan>,
        items: Vec<SelectItem>,
    },
    NestedLoopJoin {
        left: Box<PhysicalPlan>,
        right: Box<PhysicalPlan>,
        on: Option<Expr>,
        join_type: JoinType,
    },
    Sort {
        input: Box<PhysicalPlan>,
        keys: Vec<OrderByItem>,
    },
    Limit {
        input: Box<PhysicalPlan>,
        limit: Option<Expr>,
        offset: Option<Expr>,
    },
    Values {
        rows: Vec<Vec<Expr>>,
    },
    Insert {
        table_name: String,
        table_id: u64,
        columns: Option<Vec<String>>,
        values: Vec<Vec<Expr>>,
    },
    Update {
        table_name: String,
        table_id: u64,
        assignments: Vec<(String, Expr)>,
        filter: Option<Box<PhysicalPlan>>,
    },
    Delete {
        table_name: String,
        table_id: u64,
        filter: Option<Box<PhysicalPlan>>,
    },
}
