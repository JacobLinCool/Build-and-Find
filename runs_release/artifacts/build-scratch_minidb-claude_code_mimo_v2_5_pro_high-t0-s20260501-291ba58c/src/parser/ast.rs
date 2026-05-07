use crate::types::{ColumnType, Value};

#[derive(Debug, Clone)]
pub enum Statement {
    CreateTable(CreateTableStmt),
    DropTable(DropTableStmt),
    CreateIndex(CreateIndexStmt),
    DropIndex(DropIndexStmt),
    Insert(InsertStmt),
    Select(SelectStmt),
    Update(UpdateStmt),
    Delete(DeleteStmt),
    Begin,
    Commit,
    Rollback,
    MetaSchema,
    MetaTables,
    MetaIndexes,
    MetaExit,
    MetaFormat(String),
}

#[derive(Debug, Clone)]
pub struct CreateTableStmt {
    pub name: String,
    pub columns: Vec<ColumnDefAst>,
    pub check: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct ColumnDefAst {
    pub name: String,
    pub col_type: ColumnType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<Value>,
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
}

#[derive(Debug, Clone)]
pub struct DropIndexStmt {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct InsertStmt {
    pub table: String,
    pub columns: Option<Vec<String>>,
    pub values: Vec<Vec<Expr>>,
}

#[derive(Debug, Clone)]
pub struct SelectStmt {
    pub projections: Vec<Projection>,
    pub from: Option<FromClause>,
    pub where_clause: Option<Expr>,
    pub order_by: Option<Vec<OrderByExpr>>,
    pub limit: Option<Expr>,
    pub offset: Option<Expr>,
}

#[derive(Debug, Clone)]
pub enum Projection {
    Star,
    Column {
        table: Option<String>,
        name: String,
        alias: Option<String>,
    },
    Expr {
        expr: Expr,
        alias: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum FromClause {
    Table {
        name: String,
        alias: Option<String>,
    },
    Join {
        left: Box<FromClause>,
        right: Box<FromClause>,
        on: Expr,
    },
}

#[derive(Debug, Clone)]
pub struct OrderByExpr {
    pub expr: Expr,
    pub asc: bool,
}

#[derive(Debug, Clone)]
pub struct UpdateStmt {
    pub table: String,
    pub assignments: Vec<(String, Expr)>,
    pub where_clause: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct DeleteStmt {
    pub table: String,
    pub where_clause: Option<Expr>,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Value),
    ColumnRef {
        table: Option<String>,
        name: String,
    },
    BinaryOp {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Function {
        name: String,
        args: Vec<Expr>,
    },
    IsNull(Box<Expr>),
    IsNotNull(Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    Concat,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}
