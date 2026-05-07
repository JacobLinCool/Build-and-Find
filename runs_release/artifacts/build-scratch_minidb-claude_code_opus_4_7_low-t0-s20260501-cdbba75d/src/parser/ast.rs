use crate::types::{DataType, Value};

#[derive(Debug, Clone)]
pub enum Statement {
    Select(Select),
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        rows: Vec<Vec<Expr>>,
    },
    Update {
        table: String,
        sets: Vec<(String, Expr)>,
        where_clause: Option<Expr>,
    },
    Delete {
        table: String,
        where_clause: Option<Expr>,
    },
    CreateTable {
        name: String,
        columns: Vec<ColumnDef>,
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
    Begin,
    Commit,
    Rollback,
}

#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: DataType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct Select {
    pub projections: Vec<Projection>,
    pub table: String,
    pub table_alias: Option<String>,
    pub join: Option<Join>,
    pub where_clause: Option<Expr>,
    pub order_by: Vec<(Expr, bool)>, // (expr, asc)
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Join {
    pub table: String,
    pub alias: Option<String>,
    pub on: Expr,
}

#[derive(Debug, Clone)]
pub enum Projection {
    Star,
    QualifiedStar(String),
    Expr(Expr, Option<String>),
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Value),
    Column {
        table: Option<String>,
        name: String,
    },
    BinaryOp {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnOp,
        expr: Box<Expr>,
    },
    IsNull(Box<Expr>),
    FunctionCall {
        name: String,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Concat,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}
