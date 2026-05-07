use crate::parser::ast::{Delete, Insert, Select, Statement, Update};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LogicalPlan {
    Select(Select),
    Insert(Insert),
    Update(Update),
    Delete(Delete),
    Statement(Statement),
}

impl From<Statement> for LogicalPlan {
    fn from(value: Statement) -> Self {
        match value {
            Statement::Select(select) => Self::Select(select),
            Statement::Insert(insert) => Self::Insert(insert),
            Statement::Update(update) => Self::Update(update),
            Statement::Delete(delete) => Self::Delete(delete),
            statement => Self::Statement(statement),
        }
    }
}
