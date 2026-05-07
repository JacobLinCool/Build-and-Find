use crate::types::{DataType, Value};

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
    // Meta-commands
    MetaSchema,
    MetaTables,
    MetaIndexes,
    MetaExit,
    MetaFormat(String),
}

#[derive(Debug, Clone)]
pub struct SelectStmt {
    pub distinct: bool,
    pub columns: Vec<SelectItem>,
    pub from: Option<FromClause>,
    pub where_clause: Option<Expr>,
    pub order_by: Vec<OrderByItem>,
    pub limit: Option<Expr>,
    pub offset: Option<Expr>,
}

#[derive(Debug, Clone)]
pub enum SelectItem {
    Star,
    TableStar(String),
    Expr { expr: Expr, alias: Option<String> },
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
pub struct OrderByItem {
    pub expr: Expr,
    pub asc: bool,
}

#[derive(Debug, Clone)]
pub struct InsertStmt {
    pub table: String,
    pub columns: Option<Vec<String>>,
    pub values: Vec<Vec<Expr>>,
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
pub struct CreateTableStmt {
    pub name: String,
    pub if_not_exists: bool,
    pub columns: Vec<ColumnDef>,
    pub checks: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub name: String,
    pub dtype: DataType,
    pub not_null: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct DropTableStmt {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone)]
pub struct CreateIndexStmt {
    pub name: String,
    pub table: String,
    pub column: String,
    pub unique: bool,
    pub if_not_exists: bool,
}

#[derive(Debug, Clone)]
pub struct DropIndexStmt {
    pub name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Value),
    Column(String),
    QualifiedColumn(String, String), // table.col
    BinOp {
        op: BinOp,
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
    Between {
        expr: Box<Expr>,
        low: Box<Expr>,
        high: Box<Expr>,
    },
    InList {
        expr: Box<Expr>,
        list: Vec<Expr>,
        negated: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
    Add,
    Sub,
    Mul,
    Div,
    Concat,
}

#[derive(Debug, Clone)]
pub enum UnaryOp {
    Neg,
    Not,
}
