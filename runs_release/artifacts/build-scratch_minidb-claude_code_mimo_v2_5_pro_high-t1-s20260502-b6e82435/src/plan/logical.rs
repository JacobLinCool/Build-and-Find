use crate::parser::ast::{BinaryOp, UnaryOp};
use crate::types::Value;

#[derive(Debug, Clone)]
pub enum LogicalPlan {
    SeqScan {
        table: String,
        alias: Option<String>,
        filter: Option<CompiledExpr>,
    },
    IndexScan {
        table: String,
        alias: Option<String>,
        index_name: String,
        key: CompiledExpr,
    },
    Filter {
        input: Box<LogicalPlan>,
        predicate: CompiledExpr,
    },
    Project {
        input: Box<LogicalPlan>,
        exprs: Vec<(CompiledExpr, Option<String>)>,
    },
    NestedLoopJoin {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        condition: CompiledExpr,
        join_type: JoinType,
    },
    Sort {
        input: Box<LogicalPlan>,
        keys: Vec<(CompiledExpr, bool)>, // (expr, ascending)
    },
    Limit {
        input: Box<LogicalPlan>,
        count: usize,
        offset: usize,
    },
    Insert {
        table: String,
        rows: Vec<Vec<CompiledExpr>>,
        columns: Vec<String>,
    },
    Update {
        table: String,
        assignments: Vec<(String, CompiledExpr)>,
        filter: Option<CompiledExpr>,
    },
    Delete {
        table: String,
        filter: Option<CompiledExpr>,
    },
    Empty,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JoinType {
    Inner,
    Left,
    Right,
    Cross,
}

#[derive(Debug, Clone)]
pub enum CompiledExpr {
    Literal(Value),
    ColumnRef {
        table: Option<String>,
        name: String,
        index: Option<usize>,
    },
    BinaryOp {
        left: Box<CompiledExpr>,
        op: BinaryOp,
        right: Box<CompiledExpr>,
    },
    UnaryOp {
        op: UnaryOp,
        expr: Box<CompiledExpr>,
    },
    FunctionCall {
        name: String,
        args: Vec<CompiledExpr>,
    },
    IsNull(Box<CompiledExpr>),
    IsNotNull(Box<CompiledExpr>),
    Coalesce(Vec<CompiledExpr>),
    Case {
        when_clauses: Vec<(CompiledExpr, CompiledExpr)>,
        else_clause: Option<Box<CompiledExpr>>,
    },
}
