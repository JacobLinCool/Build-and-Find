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
    MetaCommand(MetaCmd),
}

#[derive(Debug, Clone)]
pub struct SelectStmt {
    pub distinct: bool,
    pub projections: Vec<SelectItem>,
    pub from: Option<FromClause>,
    pub where_clause: Option<Expr>,
    pub order_by: Vec<OrderByItem>,
    pub limit: Option<Expr>,
    pub offset: Option<Expr>,
}

#[derive(Debug, Clone)]
pub enum SelectItem {
    Star,                       // *
    TableStar(String),          // t.*
    Expr(Expr, Option<String>), // expr [AS alias]
}

#[derive(Debug, Clone)]
pub enum FromClause {
    Table(TableRef),
    Join {
        left: TableRef,
        right: TableRef,
        condition: Option<Expr>,
        join_type: JoinType,
    },
}

#[derive(Debug, Clone)]
pub struct TableRef {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone)]
pub enum JoinType {
    Inner,
}

#[derive(Debug, Clone)]
pub struct OrderByItem {
    pub expr: Expr,
    pub asc: bool,
}

#[derive(Debug, Clone)]
pub struct InsertStmt {
    pub table: String,
    pub columns: Option<Vec<String>>, // None = positional
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
pub enum MetaCmd {
    Schema,
    Tables,
    Indexes,
    Exit,
    Format(String),
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Value),
    Column(String),                  // col
    QualifiedColumn(String, String), // table.col
    BinaryOp {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    IsNull(Box<Expr>),
    IsNotNull(Box<Expr>),
    FunctionCall {
        name: String,
        args: Vec<Expr>,
    },
    Like {
        expr: Box<Expr>,
        pattern: Box<Expr>,
        negated: bool,
    },
    InList {
        expr: Box<Expr>,
        list: Vec<Expr>,
        negated: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Add,
    Sub,
    Mul,
    Div,
    Concat,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Not,
    Neg,
}
