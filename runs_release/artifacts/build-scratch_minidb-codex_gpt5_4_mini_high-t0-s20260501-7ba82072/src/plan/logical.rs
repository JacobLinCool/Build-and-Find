use serde::{Deserialize, Serialize};

use crate::parser::ast::Statement;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LogicalPlan {
    Statement(Statement),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn wraps_statement() {
        let stmt = parse("BEGIN;").unwrap().remove(0);
        let plan = LogicalPlan::Statement(stmt);
        assert!(matches!(plan, LogicalPlan::Statement(_)));
    }
}
