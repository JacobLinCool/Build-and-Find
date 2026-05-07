pub mod logical;
pub mod physical;

pub use logical::LogicalPlan;
pub use physical::{PhysicalOperator, PhysicalPlan};

use crate::catalog::Catalog;
use crate::error::Result;
use crate::parser::ast;

pub fn plan_statement(stmt: &ast::Statement, catalog: &Catalog) -> Result<PhysicalPlan> {
    let logical = LogicalPlan::from_ast(stmt, catalog)?;
    let physical = PhysicalPlan::from_logical(logical, catalog)?;
    Ok(physical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::*;
    use crate::types::*;

    fn make_catalog() -> Catalog {
        let mut cat = Catalog::new();
        let schema = TableSchema {
            name: "users".into(),
            columns: vec![
                ColumnSchema {
                    name: "id".into(),
                    col_type: ColumnType::Int,
                    primary_key: true,
                    not_null: true,
                    unique: false,
                    default: None,
                },
                ColumnSchema {
                    name: "name".into(),
                    col_type: ColumnType::Text,
                    primary_key: false,
                    not_null: false,
                    unique: false,
                    default: None,
                },
            ],
            check_constraints: vec![],
        };
        cat.create_table(schema).unwrap();
        cat
    }

    #[test]
    fn test_plan_select() {
        let cat = make_catalog();
        let stmts = crate::parser::parse("SELECT * FROM users;").unwrap();
        let plan = plan_statement(&stmts[0], &cat).unwrap();
        // Plan should exist
        assert!(format!("{:?}", plan).contains("SeqScan"));
    }

    #[test]
    fn test_plan_insert() {
        let cat = make_catalog();
        let stmts = crate::parser::parse("INSERT INTO users VALUES (1, 'alice');").unwrap();
        let plan = plan_statement(&stmts[0], &cat).unwrap();
        assert!(format!("{:?}", plan).contains("Insert"));
    }
}
