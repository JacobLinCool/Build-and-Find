//! Abstract syntax tree produced by the parser.

use crate::types::{ColumnType, Value};

/// Top-level SQL statement.
#[derive(Debug, Clone)]
pub enum Statement {
    Select(SelectStmt),
    Insert(InsertStmt),
    Update(UpdateStmt),
    Delete(DeleteStmt),
    CreateTable(CreateTableStmt),
    DropTable(DropTableStmt),
    CreateIndex(CreateIndexStmt),
    DropIndex(DropIndexStmt),
    Begin,
    Commit,
    Rollback,
}

/// `SELECT ... FROM ... [INNER] JOIN ... ON ... WHERE ... ORDER BY ... LIMIT n OFFSET m`
#[derive(Debug, Clone)]
pub struct SelectStmt {
    pub projections: Vec<Projection>,
    pub from: TableRef,
    pub joins: Vec<Join>,
    pub filter: Option<Expression>,
    pub order_by: Vec<OrderTerm>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum Projection {
    /// `*` — selects all source columns.
    AllColumns,
    /// `expr AS alias`
    Expr {
        expr: Expression,
        alias: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct TableRef {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Join {
    pub kind: JoinKind,
    pub right: TableRef,
    pub on: Expression,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
}

#[derive(Debug, Clone)]
pub struct OrderTerm {
    pub expr: Expression,
    pub ascending: bool,
}

/// `INSERT INTO t [(col, ...)] VALUES (v, ...)[, (v, ...)]*`
#[derive(Debug, Clone)]
pub struct InsertStmt {
    pub table: String,
    pub columns: Option<Vec<String>>,
    pub rows: Vec<Vec<Expression>>,
}

#[derive(Debug, Clone)]
pub struct UpdateStmt {
    pub table: String,
    pub assignments: Vec<Assignment>,
    pub filter: Option<Expression>,
}

#[derive(Debug, Clone)]
pub struct Assignment {
    pub column: String,
    pub expr: Expression,
}

#[derive(Debug, Clone)]
pub struct DeleteStmt {
    pub table: String,
    pub filter: Option<Expression>,
}

#[derive(Debug, Clone)]
pub struct CreateTableStmt {
    pub name: String,
    pub columns: Vec<ColumnDef>,
}

#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub name: String,
    pub ty: ColumnType,
    pub not_null: bool,
    pub unique: bool,
    pub primary_key: bool,
    pub default: Option<Expression>,
}

#[derive(Debug, Clone)]
pub struct DropTableStmt {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct CreateIndexStmt {
    pub name: String,
    pub table: String,
    pub column: String,
    pub unique: bool,
}

#[derive(Debug, Clone)]
pub struct DropIndexStmt {
    pub name: String,
}

/// Scalar expression tree.
#[derive(Debug, Clone)]
pub enum Expression {
    Literal(Value),
    ColumnRef {
        table: Option<String>,
        column: String,
    },
    BinaryOp {
        op: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    UnaryOp {
        op: UnaryOperator,
        expr: Box<Expression>,
    },
    IsNull {
        expr: Box<Expression>,
        negated: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    Neg,
    Not,
}
