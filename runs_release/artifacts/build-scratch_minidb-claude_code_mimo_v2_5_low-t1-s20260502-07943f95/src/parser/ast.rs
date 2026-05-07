pub use crate::types::{RowId, Value};

#[derive(Debug, Clone)]
pub enum Statement {
    Select(Select),
    Insert(Insert),
    Update(Update),
    Delete(Delete),
    CreateTable(CreateTable),
    DropTable(String),
    CreateIndex(CreateIndex),
    DropIndex(String),
    Begin,
    Commit,
    Rollback,
}

#[derive(Debug, Clone)]
pub struct Select {
    pub distinct: bool,
    pub projection: Vec<Expr>,
    pub from: FromClause,
    pub where_clause: Option<Expr>,
    pub order_by: Option<Vec<OrderByItem>>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Insert {
    pub table: String,
    pub columns: Option<Vec<String>>,
    pub values: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct Update {
    pub table: String,
    pub assignments: Vec<Assignment>,
    pub where_clause: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct Delete {
    pub table: String,
    pub where_clause: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct CreateTable {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub check_constraints: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CreateIndex {
    pub name: String,
    pub table: String,
    pub column: String,
}

#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub name: String,
    pub col_type: ColumnType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

#[derive(Debug, Clone)]
pub enum ColumnType {
    Int,
    BigInt,
    Real,
    Text,
    Bool,
}

impl ColumnType {
    pub fn to_type_def(&self) -> crate::types::ColumnType {
        match self {
            ColumnType::Int => crate::types::ColumnType::Int,
            ColumnType::BigInt => crate::types::ColumnType::BigInt,
            ColumnType::Real => crate::types::ColumnType::Real,
            ColumnType::Text => crate::types::ColumnType::Text,
            ColumnType::Bool => crate::types::ColumnType::Bool,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Assignment {
    pub column: String,
    pub expr: Expr,
}

#[derive(Debug, Clone)]
pub struct OrderByItem {
    pub expr: Expr,
    pub desc: bool,
}

#[derive(Debug, Clone)]
pub enum FromClause {
    Table(TableRef),
    Join(JoinClause),
}

#[derive(Debug, Clone)]
pub struct TableRef {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone)]
pub struct JoinClause {
    pub left_table: String,
    pub left_alias: Option<String>,
    pub right_table: String,
    pub right_alias: Option<String>,
    pub on: Expr,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Value),
    ColumnRef {
        table: Option<String>,
        column: String,
    },
    Star,
    BinaryOp {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    IsNull(Box<Expr>),
    IsNotNull(Box<Expr>),
    Function {
        name: String,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    And,
    Or,
    Concat,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}
