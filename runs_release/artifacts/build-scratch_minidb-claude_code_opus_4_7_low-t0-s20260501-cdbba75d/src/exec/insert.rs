//! `Insert` is a DML operator. Unlike streaming SELECT operators, it does not
//! produce rows; instead it returns the number of rows affected when executed
//! against the database engine.

use crate::parser::ast::Expr;

pub struct Insert {
    pub table: String,
    pub columns: Option<Vec<String>>,
    pub rows: Vec<Vec<Expr>>,
}
