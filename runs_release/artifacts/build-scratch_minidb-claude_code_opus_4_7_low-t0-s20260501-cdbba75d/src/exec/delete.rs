use crate::parser::ast::Expr;

pub struct Delete {
    pub table: String,
    pub where_clause: Option<Expr>,
}
