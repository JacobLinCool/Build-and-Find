use crate::parser::ast::*;

#[derive(Clone, Debug)]
pub enum LogicalPlan {
    Statement(Statement),
}

#[derive(Clone, Debug)]
pub enum PhysicalPlan {
    Statement(Statement),
}

pub fn lower(stmt: Statement) -> PhysicalPlan {
    PhysicalPlan::Statement(stmt)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lowers() {
        let p = lower(Statement::Begin);
        matches!(p, PhysicalPlan::Statement(_));
    }
}
