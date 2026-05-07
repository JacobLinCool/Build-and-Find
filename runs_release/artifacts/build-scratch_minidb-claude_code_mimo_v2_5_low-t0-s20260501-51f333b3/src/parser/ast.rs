use crate::types::{ColumnRef, ColumnType, SqlLiteral};

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(SqlLiteral),
    Column(ColumnRef),
    BinaryOp {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnOp,
        expr: Box<Expr>,
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

#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
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
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub enum Statement {
    CreateTable {
        name: String,
        columns: Vec<ColumnDef>,
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
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        values: Vec<Vec<Expr>>,
    },
    Select {
        columns: Vec<SelectColumn>,
        from: FromClause,
        where_clause: Option<Expr>,
        order_by: Vec<OrderByExpr>,
        limit: Option<i64>,
        offset: Option<i64>,
    },
    Update {
        table: String,
        set_clauses: Vec<(String, Expr)>,
        where_clause: Option<Expr>,
    },
    Delete {
        table: String,
        where_clause: Option<Expr>,
    },
    Begin,
    Commit,
    Rollback,
}

#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub name: String,
    pub col_type: ColumnType,
    pub primary_key: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<SqlLiteral>,
    pub check: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SelectColumn {
    Star,
    Expr(Expr, Option<String>),
}

#[derive(Debug, Clone)]
pub enum FromClause {
    Table(String),
    Join {
        left: Box<FromClause>,
        right: String,
        alias: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct OrderByExpr {
    pub expr: Expr,
    pub ascending: bool,
}

#[derive(Debug, Clone)]
pub enum MetaCommand {
    Schema,
    Tables,
    Indexes,
    Exit,
    Format(String),
}

#[derive(Debug, Clone)]
pub enum Program {
    Statement(Statement),
    Meta(MetaCommand),
    Batch(Vec<Program>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expr_display() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Column(ColumnRef::new("id".to_string()))),
            op: BinOp::Eq,
            right: Box::new(Expr::Literal(SqlLiteral::Int(42))),
        };
        assert!(format!("{:?}", expr).contains("BinaryOp"));
    }

    #[test]
    fn test_column_def() {
        let col = ColumnDef {
            name: "id".to_string(),
            col_type: ColumnType::Int,
            primary_key: true,
            not_null: true,
            unique: false,
            default: None,
            check: None,
        };
        assert_eq!(col.name, "id");
        assert!(col.primary_key);
    }
}
