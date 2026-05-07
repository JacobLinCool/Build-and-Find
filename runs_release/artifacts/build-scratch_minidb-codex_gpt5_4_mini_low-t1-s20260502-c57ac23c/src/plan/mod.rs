use crate::parser::ast::Statement;

#[derive(Debug, Clone)]
pub enum LogicalPlan {
    Statement(Statement),
}

#[derive(Debug, Clone)]
pub enum PhysicalPlan {
    Statement(Statement),
}
