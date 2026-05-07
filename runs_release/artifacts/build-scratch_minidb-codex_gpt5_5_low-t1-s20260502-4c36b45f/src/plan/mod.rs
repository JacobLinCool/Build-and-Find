use crate::{
    catalog::Catalog,
    parser::ast::{BinaryOp, Expr, From, Select, Statement},
    types::Value,
};

#[derive(Clone, Debug)]
pub enum LogicalPlan {
    Statement(Statement),
}

#[derive(Clone, Debug)]
pub enum PhysicalPlan {
    Statement(Statement),
    SeqScan {
        table: String,
    },
    IndexScan {
        table: String,
        column: String,
        value: Value,
    },
}

pub fn logical(stmt: Statement) -> LogicalPlan {
    LogicalPlan::Statement(stmt)
}

pub fn physical(stmt: Statement, catalog: &Catalog) -> PhysicalPlan {
    if let Statement::Select(Select {
        from: From::Table(table),
        where_clause: Some(expr),
        ..
    }) = &stmt
    {
        if let Some((col, val)) = equality_predicate(expr) {
            if catalog.index_on(table, &col).is_some() {
                return PhysicalPlan::IndexScan {
                    table: table.clone(),
                    column: col,
                    value: val,
                };
            }
        }
    }
    PhysicalPlan::Statement(stmt)
}

fn equality_predicate(expr: &Expr) -> Option<(String, Value)> {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::Eq,
            right,
        } => match (&**left, &**right) {
            (Expr::Column { table: None, name }, Expr::Literal(v)) => {
                Some((name.clone(), v.clone()))
            }
            (Expr::Literal(v), Expr::Column { table: None, name }) => {
                Some((name.clone(), v.clone()))
            }
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logical_wraps_statement() {
        assert!(matches!(
            logical(Statement::Begin),
            LogicalPlan::Statement(_)
        ));
    }
}
