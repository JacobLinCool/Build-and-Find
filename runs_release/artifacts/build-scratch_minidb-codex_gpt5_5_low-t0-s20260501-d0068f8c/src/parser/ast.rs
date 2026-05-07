use crate::types::{Column, Value};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Statement {
    CreateTable {
        name: String,
        columns: Vec<Column>,
        checks: Vec<Expr>,
    },
    DropTable {
        name: String,
    },
    CreateIndex {
        name: String,
        table: String,
        column: String,
    },
    DropIndex {
        name: String,
    },
    Insert {
        table: String,
        columns: Vec<String>,
        values: Vec<Expr>,
    },
    Select(Select),
    Update {
        table: String,
        assignments: Vec<(String, Expr)>,
        selection: Option<Expr>,
    },
    Delete {
        table: String,
        selection: Option<Expr>,
    },
    Begin,
    Commit,
    Rollback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Select {
    pub projection: Vec<SelectItem>,
    pub from: String,
    pub join: Option<Join>,
    pub selection: Option<Expr>,
    pub order: Option<Order>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Join {
    pub right: String,
    pub on: Expr,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub expr: Expr,
    pub asc: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SelectItem {
    Star,
    Expr { expr: Expr, alias: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expr {
    Literal(Value),
    Column(String),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    IsNull {
        expr: Box<Expr>,
        not: bool,
    },
    Func {
        name: String,
        args: Vec<Expr>,
    },
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum UnaryOp {
    Not,
    Neg,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}
