use crate::types::{DataType, Value};

#[derive(Debug, Clone)]
pub enum Stmt {
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
    MetaSchema,
    MetaTables,
    MetaIndexes,
    MetaExit,
    MetaFormat(String),
}

#[derive(Debug, Clone)]
pub struct SelectStmt {
    pub projections: Vec<SelectItem>,
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
    Expr(Expr, Option<String>),
}

#[derive(Debug, Clone)]
pub enum FromClause {
    Table(String, Option<String>),
    Join {
        left: Box<FromClause>,
        right: Box<FromClause>,
        condition: Expr,
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
    pub table: String,
    pub if_not_exists: bool,
    pub columns: Vec<ColumnSpec>,
    pub table_constraints: Vec<TableConstraint>,
}

#[derive(Debug, Clone)]
pub struct ColumnSpec {
    pub name: String,
    pub data_type: DataType,
    pub constraints: Vec<ColumnConstraint>,
}

#[derive(Debug, Clone)]
pub enum ColumnConstraint {
    PrimaryKey,
    NotNull,
    Unique,
    Default(Expr),
    Check(Expr),
}

#[derive(Debug, Clone)]
pub enum TableConstraint {
    PrimaryKey(Vec<String>),
    Unique(Vec<String>),
    Check(Expr),
}

#[derive(Debug, Clone)]
pub struct DropTableStmt {
    pub table: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone)]
pub struct CreateIndexStmt {
    pub index_name: String,
    pub table: String,
    pub column: String,
    pub unique: bool,
    pub if_not_exists: bool,
}

#[derive(Debug, Clone)]
pub struct DropIndexStmt {
    pub index_name: String,
    pub if_exists: bool,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Lit(Value),
    Col(String, Option<String>),
    BinOp(Box<Expr>, BinOp, Box<Expr>),
    UnaryOp(UnaryOp, Box<Expr>),
    IsNull(Box<Expr>),
    IsNotNull(Box<Expr>),
    Function(String, Vec<Expr>),
    Case {
        operand: Option<Box<Expr>>,
        branches: Vec<(Expr, Expr)>,
        else_branch: Option<Box<Expr>>,
    },
    In(Box<Expr>, Vec<Expr>),
    Between(Box<Expr>, Box<Expr>, Box<Expr>),
    Like(Box<Expr>, String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Concat,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}
