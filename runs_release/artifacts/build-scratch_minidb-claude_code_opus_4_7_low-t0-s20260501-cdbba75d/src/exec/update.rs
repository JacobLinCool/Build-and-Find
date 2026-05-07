use crate::parser::ast::Expr;

pub struct Update {
    pub table: String,
    pub sets: Vec<(String, Expr)>,
    pub where_clause: Option<Expr>,
}
