use crate::types::{DataType, Value};

#[derive(Debug, Clone)]
pub enum Statement {
    CreateTable(CreateTable),
    DropTable {
        name: String,
        if_exists: bool,
    },
    CreateIndex {
        name: String,
        table: String,
        column: String,
    },
    DropIndex {
        name: String,
        if_exists: bool,
    },
    Insert(Insert),
    Select(Box<Select>),
    Update(Update),
    Delete(Delete),
    Begin,
    Commit,
    Rollback,
}

#[derive(Debug, Clone)]
pub struct CreateTable {
    pub name: String,
    pub columns: Vec<ColumnDef>,
    pub checks: Vec<Expr>,
    pub if_not_exists: bool,
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
pub struct Insert {
    pub table: String,
    pub columns: Option<Vec<String>>,
    pub rows: Vec<Vec<Expr>>,
}

#[derive(Debug, Clone)]
pub struct Select {
    pub projections: Vec<Projection>,
    pub from: TableRef,
    pub join: Option<Join>,
    pub where_clause: Option<Expr>,
    pub order_by: Vec<OrderBy>,
    pub limit: Option<Expr>,
    pub offset: Option<Expr>,
}

#[derive(Debug, Clone)]
pub enum Projection {
    Star,
    QualifiedStar(String),
    Expr(Expr, Option<String>),
}

#[derive(Debug, Clone)]
pub struct TableRef {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Join {
    pub right: TableRef,
    pub on: Expr,
}

#[derive(Debug, Clone)]
pub struct OrderBy {
    pub expr: Expr,
    pub desc: bool,
}

#[derive(Debug, Clone)]
pub struct Update {
    pub table: String,
    pub assignments: Vec<(String, Expr)>,
    pub where_clause: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct Delete {
    pub table: String,
    pub where_clause: Option<Expr>,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Value),
    Column {
        table: Option<String>,
        name: String,
    },
    UnaryOp {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    BinaryOp {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    IsNull(Box<Expr>),
    IsNotNull(Box<Expr>),
    Function {
        name: String,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Concat,
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
}

use std::fmt;

impl fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Mod => "%",
            BinaryOp::Concat => "||",
            BinaryOp::Eq => "=",
            BinaryOp::Neq => "<>",
            BinaryOp::Lt => "<",
            BinaryOp::Lte => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Gte => ">=",
            BinaryOp::And => "AND",
            BinaryOp::Or => "OR",
        };
        write!(f, "{}", s)
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Literal(v) => match v {
                Value::Null => write!(f, "NULL"),
                Value::Int(i) => write!(f, "{}", i),
                Value::Real(r) => write!(f, "{}", r),
                Value::Text(s) => {
                    let escaped = s.replace('\'', "''");
                    write!(f, "'{}'", escaped)
                }
                Value::Bool(b) => write!(f, "{}", if *b { "TRUE" } else { "FALSE" }),
            },
            Expr::Column {
                table: Some(t),
                name,
            } => write!(f, "{}.{}", t, name),
            Expr::Column { table: None, name } => write!(f, "{}", name),
            Expr::UnaryOp { op, expr } => match op {
                UnaryOp::Neg => write!(f, "(-{})", expr),
                UnaryOp::Not => write!(f, "(NOT {})", expr),
            },
            Expr::BinaryOp { op, left, right } => {
                write!(f, "({} {} {})", left, op, right)
            }
            Expr::IsNull(e) => write!(f, "({} IS NULL)", e),
            Expr::IsNotNull(e) => write!(f, "({} IS NOT NULL)", e),
            Expr::Function { name, args } => {
                write!(f, "{}(", name)?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", a)?;
                }
                write!(f, ")")
            }
        }
    }
}

/// Re-parse an expression from its serialised text form.
pub fn parse_expr_string(s: &str) -> crate::error::Result<Expr> {
    use crate::parser::Parser;
    // Hack: parse "SELECT <expr>;" and pull the projection out.
    let wrapped = format!("SELECT {} FROM __dummy__;", s);
    let stmts = Parser::parse_sql(&wrapped)?;
    if let Some(Statement::Select(sel)) = stmts.into_iter().next() {
        if let Some(Projection::Expr(e, _)) = sel.projections.into_iter().next() {
            return Ok(e);
        }
    }
    Err(crate::error::MiniDbError::Other(
        "could not re-parse stored expression".into(),
    ))
}
