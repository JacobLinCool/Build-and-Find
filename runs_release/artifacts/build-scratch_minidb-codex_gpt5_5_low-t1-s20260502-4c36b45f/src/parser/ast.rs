use serde::{Deserialize, Serialize};

use crate::{catalog::TableSchema, types::Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Statement {
    CreateTable(TableSchema),
    DropTable(String),
    CreateIndex {
        name: String,
        table: String,
        column: String,
    },
    DropIndex(String),
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        values: Vec<Expr>,
    },
    Select(Select),
    Update {
        table: String,
        assigns: Vec<(String, Expr)>,
        where_clause: Option<Expr>,
    },
    Delete {
        table: String,
        where_clause: Option<Expr>,
    },
    Begin,
    Commit,
    Rollback,
    Meta(MetaCommand),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MetaCommand {
    Schema,
    Tables,
    Indexes,
    Exit,
    Format(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Select {
    pub projection: Vec<SelectItem>,
    pub from: From,
    pub where_clause: Option<Expr>,
    pub order_by: Vec<OrderBy>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum From {
    Table(String),
    Join {
        left: String,
        right: String,
        on: Expr,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SelectItem {
    Star,
    Expr { expr: Expr, alias: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderBy {
    pub expr: Expr,
    pub desc: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Literal(Value),
    Column {
        table: Option<String>,
        name: String,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Function {
        name: String,
        args: Vec<Expr>,
    },
    IsNull {
        expr: Box<Expr>,
        negated: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Not,
    Neg,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
