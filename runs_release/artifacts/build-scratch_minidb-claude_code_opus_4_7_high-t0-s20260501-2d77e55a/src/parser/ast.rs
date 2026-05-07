//! Abstract syntax tree produced by the parser.

use crate::types::{DataType, Value};

#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    Select(Select),
    Insert(Insert),
    Update(Update),
    Delete(Delete),
    CreateTable(CreateTable),
    DropTable(DropTable),
    CreateIndex(CreateIndex),
    DropIndex(DropIndex),
    Begin,
    Commit,
    Rollback,
    Meta(MetaCommand),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetaCommand {
    pub name: String,
    pub arg: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CreateTable {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub checks: Vec<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColumnDef {
    pub name: String,
    pub dtype: DataType,
    pub not_null: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DropTable {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CreateIndex {
    pub name: String,
    pub table: String,
    pub column: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DropIndex {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Insert {
    pub table: String,
    pub columns: Option<Vec<String>>,
    pub rows: Vec<Vec<Expr>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub table: String,
    pub assignments: Vec<(String, Expr)>,
    pub where_clause: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Delete {
    pub table: String,
    pub where_clause: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Select {
    pub projections: Vec<Projection>,
    pub from: FromClause,
    pub where_clause: Option<Expr>,
    pub order_by: Option<Vec<OrderKey>>,
    pub limit: Option<LimitClause>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Projection {
    All,
    AllOf(String),
    Expr { expr: Expr, alias: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum FromClause {
    Table { name: String, alias: Option<String> },
    Join(JoinClause),
}

#[derive(Clone, Debug, PartialEq)]
pub struct JoinClause {
    pub left_table: String,
    pub left_alias: Option<String>,
    pub right_table: String,
    pub right_alias: Option<String>,
    pub on: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrderKey {
    pub expr: Expr,
    pub direction: SortDir,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LimitClause {
    pub limit: i64,
    pub offset: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Literal(Value),
    Column {
        table: Option<String>,
        name: String,
    },
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Neg(Box<Expr>),
    IsNull(Box<Expr>),
    Func {
        name: String,
        args: Vec<Expr>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Eq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Concat,
}
